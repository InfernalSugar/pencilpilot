//! Layers composited on the GPU: each drawing's tiles kept in GPU memory (an atlas of
//! 64 × 64 tiles, premultiplied linear fix15 as the brush engine paints them), and the
//! picture made from them in a compute shader (`shaders/layers.wgsl`) with the brush
//! engine's own arithmetic, so it matches the CPU's picture to within one level a channel.
//!
//! - **Sources:** anything with tiles (a drawing, a layer's mask) by an id the caller picks.
//!   Their tiles go up once ([`Compositor::put_tile`]) and stay until forgotten or put
//!   away for room (the least recently shown first), so going to another frame mostly
//!   points the shader at other tiles.
//! - **A frame:** the layers, bottom first, each a source with its opacity, blend mode,
//!   visibility, clipping and mask, and the onion skin's sources
//!   ([`Compositor::set_frame`]). Only tiles whose sources, settings or pixels changed are
//!   composited again.
//! - **The picture** goes into the [`Canvas`] the interface shows, with the next draw
//!   ([`crate::Renderer2d::draw_composited`]).

use std::collections::HashMap;
use std::ffi::c_void;

use ash::vk;

use super::{Ctx, Error, Gpu, Result};

const SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/layers.spv"));

/// Tiles are this many pixels a side.
pub const TILE: u32 = 64;
/// One tile's bytes (8 a pixel).
pub const TILE_BYTES: u64 = (TILE * TILE * 8) as u64;
/// Most layers in a frame, and onion skin entries.
pub const MAX_LAYERS: usize = 32;
pub const MAX_ONION: usize = 4;
const EMPTY: u32 = u32::MAX;
/// The frame description's parts (words; see `shaders/layers.wgsl`).
const LAYERS_AT: usize = 16;
const ONIONS_AT: usize = 144;
const DIRTY_AT: usize = 160;
/// A tile to composite whose prefix (see [`Compositor::set_frame`]) is made again.
const REBUILD: u32 = 1 << 31;
/// Layers unchanged for this many frames in a row, from the bottom, are kept composited
/// (the prefix), when there are at least `PREFIX_MIN` of them.
const STABLE_FRAMES: u64 = 4;
const PREFIX_MIN: usize = 3;
/// Tiles uploaded in one go, at most (16 MB of staging).
const STAGING_TILES: usize = 512;

/// A pixel as the brush engine keeps it: premultiplied linear fix15 RGBA, 64 × 64 a tile.
pub type TilePx = [[[u16; 4]; 64]; 64];

/// One layer of a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuLayer {
    /// its pixels (a source)
    pub source: u64,
    /// its mask (a source: white shows, black hides, unpainted shows)
    pub mask: Option<u64>,
    pub opacity: f32,
    /// the brush engine's [`Blend`] as a number: normal, multiply, screen, overlay, darken,
    /// lighten, add
    pub blend: u32,
    pub visible: bool,
    pub clip: bool,
}

/// One onion skin entry: a source's lines tinted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnionLayer {
    pub source: u64,
    /// sRGB 0–1
    pub tint: [f32; 3],
    /// 0–255
    pub opacity: u32,
}

/// A source's tiles: the slot each tile position is in (or none), and what's in it.
struct Source {
    slots: Vec<u32>,
    hashes: Vec<u64>,
    count: u32,
    /// the frame it was last wanted in
    used: u64,
}

/// The layers on the GPU, composited into a picture.
pub struct Compositor {
    device: ash::Device,
    pub width: u32,
    pub height: u32,
    tiles_x: u32,
    ntiles: usize,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    atlas: Buf,
    /// slots the atlas holds, and the most it may grow to
    capacity: u32,
    max_slots: u32,
    free: Vec<u32>,
    top: u32,
    sources: HashMap<u64, Source>,
    stamp: u64,
    staging: Buf,
    /// what the staging buffer holds: the slot each of its tiles goes to (and where each
    /// slot's is, so a slot is written once a go)
    staged: Vec<u32>,
    staged_at: HashMap<u32, usize>,
    frame: Buf,
    frame_words: usize,
    lut: Buf,
    pub(crate) out: Buf,
    /// the frame's layers and onion skin, and the slots each tile showed with
    layers: Vec<GpuLayer>,
    onion: Vec<OnionLayer>,
    /// the sources this frame shows (a change to another's tiles needs no compositing)
    in_frame: std::collections::HashSet<u64>,
    shown: Vec<u32>,
    dirty: Vec<bool>,
    any_dirty: bool,
    /// tiles composited, waiting to go into the canvas (tile indices)
    pub(crate) done: Vec<u32>,
    /// for stats: tiles uploaded so far
    pub uploads: u64,
    /// the bottom layers that haven't changed lately, kept composited (each pixel's colour
    /// and the alpha clipped layers show through, as the shader has them after those
    /// layers): how many, where it's up to date, and since when each layer is unchanged
    prefix: Buf,
    prefix_k: usize,
    prefix_ok: Vec<bool>,
    changed_at: [u64; MAX_LAYERS],
    frames: u64,
    /// sources whose tiles changed since the last frame
    touched: std::collections::HashSet<u64>,
}

/// A buffer and its memory (mapped when host-visible).
pub(crate) struct Buf {
    pub(crate) buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut c_void,
    size: u64,
}

impl Buf {
    fn new(gpu: &Gpu, size: u64, usage: vk::BufferUsageFlags, host: bool) -> Result<Buf> {
        let d = &gpu.device;
        unsafe {
            let buffer = d
                .create_buffer(
                    &vk::BufferCreateInfo::default().size(size).usage(usage),
                    None,
                )
                .ctx("vkCreateBuffer")?;
            let req = d.get_buffer_memory_requirements(buffer);
            let want = if host {
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
            } else {
                vk::MemoryPropertyFlags::DEVICE_LOCAL
            };
            let Some(ty) = gpu.memory_type(req.memory_type_bits, want) else {
                d.destroy_buffer(buffer, None);
                return Err(Error::Missing("memory for the GPU layers"));
            };
            let memory = match d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty),
                None,
            ) {
                Ok(m) => m,
                Err(e) => {
                    d.destroy_buffer(buffer, None);
                    return Err(Error::Vk("vkAllocateMemory", e));
                }
            };
            d.bind_buffer_memory(buffer, memory, 0)
                .ctx("vkBindBufferMemory")?;
            let ptr = if host {
                d.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())
                    .ctx("vkMapMemory")?
            } else {
                std::ptr::null_mut()
            };
            Ok(Buf {
                buffer,
                memory,
                ptr,
                size,
            })
        }
    }

    fn destroy(&self, d: &ash::Device) {
        // SAFETY: made by this device; the caller waited for the GPU
        unsafe {
            if !self.ptr.is_null() {
                d.unmap_memory(self.memory);
            }
            d.destroy_buffer(self.buffer, None);
            d.free_memory(self.memory, None);
        }
    }
}

/// A quick hash of a tile's pixels (to tell whether a tile that may have changed did).
pub fn tile_hash(px: &TilePx) -> u64 {
    // SAFETY: a tile is 32 768 bytes of u16s: 4096 u64s, aligned (u16 alignment is
    // enough for an unaligned read, done as such)
    let words =
        unsafe { std::slice::from_raw_parts(px.as_ptr() as *const u8, TILE_BYTES as usize) };
    let mut h = [
        0x9E37_79B9_7F4A_7C15u64,
        0xC2B2_AE3D_27D4_EB4F,
        0x1656_67B1_9E37_79F9,
        0x27D4_EB2F_1656_67C5,
    ];
    for c in words.as_chunks::<32>().0 {
        for (k, lane) in h.iter_mut().enumerate() {
            let v = u64::from_le_bytes(c[k * 8..k * 8 + 8].try_into().unwrap());
            *lane = (*lane ^ v)
                .wrapping_mul(0x0000_0100_0000_01B3)
                .rotate_left(29);
        }
    }
    h.iter().fold(0xCBF2_9CE4_8422_2325, |a, &l| {
        (a ^ l).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }) | 1
}

/// Whether nothing is painted in a tile.
fn blank(px: &TilePx) -> bool {
    px.iter().flatten().all(|p| p[3] == 0)
}

impl Compositor {
    /// A compositor for a `width` × `height` picture. `lut` is the brush engine's table from
    /// linear fix15 (0..=32768) to sRGB 0–1. `budget` caps all the GPU memory it takes (bytes):
    /// the tiles get what the picture, the kept bottom layers and the uploads leave.
    pub fn new(gpu: &Gpu, width: u32, height: u32, lut: &[f32], budget: u64) -> Result<Compositor> {
        if lut.len() != 32769 {
            return Err(Error::Missing("the colour table"));
        }
        let d = &gpu.device;
        let tiles_x = width.div_ceil(TILE);
        let ntiles = (tiles_x * height.div_ceil(TILE)) as usize;
        let max_range = unsafe { gpu.instance.get_physical_device_properties(gpu.physical) }
            .limits
            .max_storage_buffer_range as u64;
        // (at most a quarter of the GPU's own memory)
        let heap = (0..gpu.memory.memory_heap_count as usize)
            .map(|i| gpu.memory.memory_heaps[i])
            .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
            .map(|h| h.size)
            .max()
            .unwrap_or(budget * 4);
        let picture = width as u64 * height as u64 * 4;
        // the picture, the kept bottom layers (16 bytes a pixel), the staging buffer, the
        // frame's description and the colour table
        let fixed = picture * 5
            + STAGING_TILES as u64 * TILE_BYTES
            + (DIRTY_AT + ntiles * (1 + 2 * MAX_LAYERS + MAX_ONION)) as u64 * 4
            + 32769 * 4;
        let max_slots = (budget.min(heap / 4).saturating_sub(fixed).min(max_range) / TILE_BYTES)
            .max(ntiles as u64) as u32;
        let capacity = (2 * ntiles as u32).clamp(256, max_slots);
        let frame_words = DIRTY_AT + ntiles * (1 + 2 * MAX_LAYERS + MAX_ONION);
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let atlas = Buf::new(
            gpu,
            capacity as u64 * TILE_BYTES,
            storage | vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
            false,
        )?;
        let staging = Buf::new(
            gpu,
            STAGING_TILES as u64 * TILE_BYTES,
            vk::BufferUsageFlags::TRANSFER_SRC,
            true,
        )?;
        let frame = Buf::new(gpu, frame_words as u64 * 4, storage, true)?;
        let lut_buf = Buf::new(gpu, lut.len() as u64 * 4, storage, true)?;
        // SAFETY: the mapped table is as big as `lut`
        unsafe {
            std::ptr::copy_nonoverlapping(lut.as_ptr(), lut_buf.ptr as *mut f32, lut.len());
        }
        let out = Buf::new(
            gpu,
            width as u64 * height as u64 * 4,
            storage | vk::BufferUsageFlags::TRANSFER_SRC,
            false,
        )?;
        unsafe {
            let words = ash::util::read_spv(&mut std::io::Cursor::new(SPIRV))
                .map_err(|_| Error::Missing("the layers shader"))?;
            let module = d
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
                .ctx("vkCreateShaderModule")?;
            let bindings: Vec<_> = (0..5)
                .map(|b| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(b)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::COMPUTE)
                })
                .collect();
            let set_layout = d
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .ctx("vkCreateDescriptorSetLayout")?;
            let set_layouts = [set_layout];
            let layout = d
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts),
                    None,
                )
                .ctx("vkCreatePipelineLayout")?;
            let stage = vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::COMPUTE)
                .module(module)
                .name(c"main");
            let pipeline = d
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    &[vk::ComputePipelineCreateInfo::default()
                        .stage(stage)
                        .layout(layout)],
                    None,
                )
                .map_err(|(_, e)| Error::Vk("vkCreateComputePipelines", e))?[0];
            d.destroy_shader_module(module, None);
            let sizes = [vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 5,
            }];
            let pool = d
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(1)
                        .pool_sizes(&sizes),
                    None,
                )
                .ctx("vkCreateDescriptorPool")?;
            let set = d
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(pool)
                        .set_layouts(&set_layouts),
                )
                .ctx("vkAllocateDescriptorSets")?[0];
            let c = Compositor {
                device: d.clone(),
                width,
                height,
                tiles_x,
                ntiles,
                set_layout,
                layout,
                pipeline,
                pool,
                set,
                atlas,
                capacity,
                max_slots,
                free: Vec::new(),
                top: 0,
                sources: HashMap::new(),
                stamp: 1,
                staging,
                staged: Vec::new(),
                staged_at: HashMap::new(),
                frame,
                frame_words,
                lut: lut_buf,
                out,
                layers: Vec::new(),
                onion: Vec::new(),
                in_frame: Default::default(),
                shown: vec![EMPTY - 1; ntiles * (2 * MAX_LAYERS + MAX_ONION)],
                dirty: vec![true; ntiles],
                any_dirty: true,
                done: Vec::new(),
                uploads: 0,
                prefix: Buf::new(gpu, 16, storage, false)?,
                prefix_k: 0,
                prefix_ok: vec![false; ntiles],
                changed_at: [0; MAX_LAYERS],
                frames: 0,
                touched: Default::default(),
            };
            for b in 0..5 {
                c.bind(b);
            }
            Ok(c)
        }
    }

    fn bind(&self, binding: u32) {
        let buf = match binding {
            0 => &self.atlas,
            1 => &self.frame,
            2 => &self.lut,
            3 => &self.out,
            _ => &self.prefix,
        };
        let info = [vk::DescriptorBufferInfo::default()
            .buffer(buf.buffer)
            .offset(0)
            .range(buf.size)];
        // SAFETY: the set isn't in use (every submission is waited for)
        unsafe {
            self.device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::default()
                    .dst_set(self.set)
                    .dst_binding(binding)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&info)],
                &[],
            )
        };
    }

    /// GPU memory this holds (bytes): the tiles, the picture, the frame's description.
    pub fn gpu_bytes(&self) -> u64 {
        self.atlas.size
            + self.out.size
            + self.frame.size
            + self.lut.size
            + self.staging.size
            + self.prefix.size
    }

    /// Tiles held now.
    pub fn tiles_held(&self) -> u32 {
        self.top - self.free.len() as u32
    }

    /// Bottom layers kept composited now (the prefix: see [`Compositor::set_frame`]).
    pub fn prefix_layers(&self) -> usize {
        self.prefix_k
    }

    /// Sources held now.
    pub fn sources_held(&self) -> usize {
        self.sources.len()
    }

    /// A new frame begins: what it wants is marked as it's asked for ([`Compositor::has`],
    /// [`Compositor::put_tile`]), and only sources not wanted in it make room for others.
    pub fn begin_frame(&mut self) {
        self.stamp += 1;
    }

    /// Whether a source is here (all its tiles); it's wanted in this frame.
    pub fn has(&mut self, id: u64) -> bool {
        let stamp = self.stamp;
        self.sources.get_mut(&id).map(|s| s.used = stamp).is_some()
    }

    /// Whether `tiles` more fit without putting anything away (with a picture's worth of
    /// tiles left over, for what a frame changes).
    pub fn room(&self, tiles: usize) -> bool {
        self.tiles_held() as u64 + tiles as u64 + self.ntiles as u64 <= self.max_slots as u64
    }

    /// Room for `tiles` more and some to spare, made now (the atlas grown, if it may): for
    /// work between frames, so a frame that shows new drawings doesn't wait for it.
    pub fn reserve(&mut self, gpu: &Gpu, tiles: usize) -> Result<()> {
        let spare = (tiles + 2048) as u64;
        let free = (self.capacity - self.top) as u64 + self.free.len() as u64;
        if free < spare && self.capacity < self.max_slots {
            let want = self.capacity as u64 + spare - free;
            let to = want
                .max(self.capacity as u64 + (self.capacity / 4) as u64)
                .min(self.max_slots as u64) as u32;
            self.grow(gpu, to)?;
        }
        Ok(())
    }

    /// Whether a source is here (without counting it as wanted in this frame).
    pub fn holds(&self, id: u64) -> bool {
        self.sources.contains_key(&id)
    }

    /// A source with no tiles yet (its tiles follow with [`Compositor::put_tile`]).
    pub fn start(&mut self, id: u64) {
        let (stamp, n) = (self.stamp, self.ntiles);
        if let Some(s) = self.sources.get_mut(&id) {
            s.used = stamp;
            return;
        }
        self.sources.insert(
            id,
            Source {
                slots: vec![EMPTY; n],
                hashes: vec![0; n],
                count: 0,
                used: stamp,
            },
        );
    }

    /// The hash of a source's tile as it was put (None: not here, or blank).
    pub fn tile_hash_of(&self, id: u64, key: (i32, i32)) -> Option<u64> {
        let t = self.index(key)?;
        let s = self.sources.get(&id)?;
        (s.slots[t] != EMPTY).then_some(s.hashes[t])
    }

    fn index(&self, (tx, ty): (i32, i32)) -> Option<usize> {
        let th = self.ntiles as u32 / self.tiles_x;
        (tx >= 0 && ty >= 0 && (tx as u32) < self.tiles_x && (ty as u32) < th)
            .then(|| ty as usize * self.tiles_x as usize + tx as usize)
    }

    /// A source's tile at `key` (None or blank: nothing there). Tiles off the picture are
    /// left out; a tile as it already is isn't uploaded again. Returns whether it changed;
    /// an error when there's no room even with every source not wanted now put away.
    pub fn put_tile(
        &mut self,
        gpu: &Gpu,
        id: u64,
        key: (i32, i32),
        px: Option<&TilePx>,
    ) -> Result<bool> {
        self.start(id);
        let Some(t) = self.index(key) else {
            return Ok(false);
        };
        let px = px.filter(|p| !blank(p));
        let had = self.sources[&id].slots[t];
        let Some(px) = px else {
            if had == EMPTY {
                return Ok(false);
            }
            let s = self.sources.get_mut(&id).unwrap();
            s.slots[t] = EMPTY;
            s.count -= 1;
            self.free.push(had);
            if self.in_frame.contains(&id) {
                self.mark(t);
                self.touched.insert(id);
            }
            return Ok(true);
        };
        let h = tile_hash(px);
        if had != EMPTY && self.sources[&id].hashes[t] == h {
            return Ok(false);
        }
        let slot = if had != EMPTY {
            had
        } else {
            let slot = self.slot(gpu)?;
            let s = self.sources.get_mut(&id).unwrap();
            s.slots[t] = slot;
            s.count += 1;
            slot
        };
        self.sources.get_mut(&id).unwrap().hashes[t] = h;
        let i = match self.staged_at.get(&slot) {
            Some(&i) => i,
            None => {
                if self.staged.len() == STAGING_TILES {
                    self.upload_now(gpu)?;
                }
                self.staged.push(slot);
                self.staged_at.insert(slot, self.staged.len() - 1);
                self.staged.len() - 1
            }
        };
        let at = i as u64 * TILE_BYTES;
        // SAFETY: inside the mapped staging buffer (STAGING_TILES tiles)
        unsafe {
            std::ptr::copy_nonoverlapping(
                px.as_ptr() as *const u8,
                (self.staging.ptr as *mut u8).add(at as usize),
                TILE_BYTES as usize,
            );
        }
        self.uploads += 1;
        if self.in_frame.contains(&id) {
            self.mark(t);
            self.touched.insert(id);
        }
        Ok(true)
    }

    /// Tile `t` is composited again, its prefix too.
    fn mark(&mut self, t: usize) {
        self.dirty[t] = true;
        self.prefix_ok[t] = false;
        self.any_dirty = true;
    }

    /// A free slot: a new one, a freed one, or (the atlas full) the atlas grown, or the
    /// sources not wanted in this frame put away, least recently wanted first.
    fn slot(&mut self, gpu: &Gpu) -> Result<u32> {
        if let Some(s) = self.free.pop() {
            return Ok(s);
        }
        if self.top < self.capacity {
            self.top += 1;
            return Ok(self.top - 1);
        }
        if self.capacity < self.max_slots {
            // (a quarter more at a time: doubling would leave up to half of it unused)
            let to = (self.capacity + (self.capacity / 4).max(1024)).min(self.max_slots);
            self.grow(gpu, to)?;
            return self.slot(gpu);
        }
        let stamp = self.stamp;
        let mut old: Vec<(u64, u64)> = self
            .sources
            .iter()
            .filter(|(_, s)| s.used < stamp && s.count > 0)
            .map(|(id, s)| (s.used, *id))
            .collect();
        old.sort_unstable();
        // a quarter of the atlas at a time (or what there is)
        let want = (self.capacity / 4).max(1) as usize;
        for (_, id) in old {
            self.forget(id);
            if self.free.len() >= want {
                break;
            }
        }
        self.free
            .pop()
            .ok_or(Error::Missing("room for the GPU layers"))
    }

    /// The atlas made bigger (what it holds copied over).
    fn grow(&mut self, gpu: &Gpu, to: u32) -> Result<()> {
        self.upload_now(gpu)?;
        let bigger = Buf::new(
            gpu,
            to as u64 * TILE_BYTES,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC,
            false,
        )?;
        let (from, into, size) = (self.atlas.buffer, bigger.buffer, self.atlas.size);
        gpu.run(|d, cmd| unsafe {
            d.cmd_copy_buffer(cmd, from, into, &[vk::BufferCopy::default().size(size)]);
        })?;
        let old = std::mem::replace(&mut self.atlas, bigger);
        old.destroy(&self.device);
        self.capacity = to;
        self.bind(0);
        Ok(())
    }

    /// What's staged goes up now (the staging buffer is full, or the atlas changes).
    fn upload_now(&mut self, gpu: &Gpu) -> Result<()> {
        if self.staged.is_empty() {
            return Ok(());
        }
        gpu.run(|d, cmd| unsafe { self.record_uploads(d, cmd) })?;
        self.staged.clear();
        self.staged_at.clear();
        Ok(())
    }

    /// SAFETY: `cmd` is recording, on this device.
    unsafe fn record_uploads(&self, d: &ash::Device, cmd: vk::CommandBuffer) {
        if self.staged.is_empty() {
            return;
        }
        let regions: Vec<vk::BufferCopy> = self
            .staged
            .iter()
            .enumerate()
            .map(|(i, &slot)| {
                vk::BufferCopy::default()
                    .src_offset(i as u64 * TILE_BYTES)
                    .dst_offset(slot as u64 * TILE_BYTES)
                    .size(TILE_BYTES)
            })
            .collect();
        unsafe {
            // (the shader may still read what was there: the run before waited for it)
            d.cmd_copy_buffer(cmd, self.staging.buffer, self.atlas.buffer, &regions);
        }
    }

    /// A source put away: its tiles' slots are free. (Tiles showing it are composited again.)
    pub fn forget(&mut self, id: u64) {
        if let Some(s) = self.sources.remove(&id) {
            for (t, &slot) in s.slots.iter().enumerate() {
                if slot != EMPTY {
                    self.free.push(slot);
                    self.mark(t);
                }
            }
            // staged uploads into its slots are dropped: those slots may be reused, and
            // a later upload into the same slot comes after them in order (fine)
        }
    }

    /// Every source put away.
    pub fn forget_all(&mut self) {
        let ids: Vec<u64> = self.sources.keys().copied().collect();
        for id in ids {
            self.forget(id);
        }
        self.invalidate_all();
    }

    /// These tiles are composited again.
    pub fn invalidate(&mut self, keys: &[(i32, i32)]) {
        for &k in keys {
            if let Some(t) = self.index(k) {
                self.mark(t);
            }
        }
    }

    /// The whole picture is composited again.
    pub fn invalidate_all(&mut self) {
        self.dirty.fill(true);
        self.prefix_ok.fill(false);
        self.any_dirty = true;
    }

    /// This frame's layers (bottom first) and onion skin (farthest first). Every source
    /// must be here ([`Compositor::has`]); a missing one shows as nothing. `gpu` makes the
    /// prefix's memory the first time it's wanted.
    ///
    /// The prefix: the bottom layers unchanged for a few frames (the background, still
    /// layers under the animation) are kept composited, so a frame change composites only
    /// the layers above them. It's exact: the shader goes on from what it kept.
    pub fn set_frame(&mut self, gpu: &Gpu, layers: &[GpuLayer], onion: &[OnionLayer]) {
        let layers = &layers[..layers.len().min(MAX_LAYERS)];
        let onion = &onion[..onion.len().min(MAX_ONION)];
        // (frames are counted when something changes: a pause doesn't make the layers that
        // change on every other frame look still)
        let now = self.frames + 1;
        let mut changed = [false; MAX_LAYERS];
        for (l, c) in changed.iter_mut().enumerate().take(layers.len()) {
            *c = self.touched.contains(&layers[l].source)
                || layers[l].mask.is_some_and(|m| self.touched.contains(&m));
        }
        self.touched.clear();
        if layers != self.layers.as_slice() || onion != self.onion.as_slice() {
            let settings = |a: &GpuLayer| (a.opacity, a.blend, a.visible, a.clip);
            if layers.len() != self.layers.len() {
                changed = [true; MAX_LAYERS];
                self.invalidate_all();
            } else {
                for (l, (a, b)) in layers.iter().zip(&self.layers).enumerate() {
                    if settings(a) != settings(b) {
                        changed[l] = true;
                        // (its tiles composited again; the prefix goes if it's in it)
                        self.dirty.fill(true);
                        self.any_dirty = true;
                        if l < self.prefix_k {
                            self.prefix_ok.fill(false);
                        }
                    }
                }
            }
            let same_onion = onion.len() == self.onion.len()
                && onion
                    .iter()
                    .zip(&self.onion)
                    .all(|(a, b)| (a.tint, a.opacity) == (b.tint, b.opacity));
            if !same_onion {
                self.dirty.fill(true);
                self.any_dirty = true;
            }
            self.layers = layers.to_vec();
            self.onion = onion.to_vec();
            self.in_frame = layers
                .iter()
                .flat_map(|l| [Some(l.source), l.mask])
                .flatten()
                .chain(onion.iter().map(|o| o.source))
                .collect();
        }
        // the slots every tile shows with now, against what it showed with
        let n = self.ntiles;
        let empty = vec![EMPTY; n];
        let lists: Vec<(usize, Option<usize>, Option<u64>)> = (0..MAX_LAYERS)
            .map(|l| (l, Some(l), self.layers.get(l).map(|x| x.source)))
            .chain((0..MAX_LAYERS).map(|l| {
                (
                    MAX_LAYERS + l,
                    Some(l),
                    self.layers.get(l).and_then(|x| x.mask),
                )
            }))
            .chain((0..MAX_ONION).map(|e| {
                (
                    2 * MAX_LAYERS + e,
                    None,
                    self.onion.get(e).map(|x| x.source),
                )
            }))
            .collect();
        for (row, layer, src) in lists {
            let slots = src
                .and_then(|id| self.sources.get(&id))
                .map_or(&empty, |s| &s.slots);
            let shown = &mut self.shown[row * n..(row + 1) * n];
            for t in 0..n {
                if shown[t] != slots[t] {
                    shown[t] = slots[t];
                    self.dirty[t] = true;
                    self.any_dirty = true;
                    if let Some(l) = layer {
                        changed[l] = true;
                        if l < self.prefix_k {
                            self.prefix_ok[t] = false;
                        }
                    }
                }
            }
        }
        if changed.iter().any(|c| *c) {
            self.frames = now;
        }
        for (l, c) in changed.iter().enumerate() {
            if *c {
                self.changed_at[l] = now;
            }
        }
        let now = self.frames;
        // the prefix: the bottom layers unchanged lately
        let mut k = (0..self.layers.len())
            .take_while(|&l| self.changed_at[l] + STABLE_FRAMES <= now)
            .count();
        if k < PREFIX_MIN {
            k = 0;
        }
        if k > 0 && self.prefix.size < self.out.size * 4 {
            match Buf::new(
                gpu,
                self.out.size * 4,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                false,
            ) {
                Ok(b) => {
                    let old = std::mem::replace(&mut self.prefix, b);
                    old.destroy(&self.device);
                    self.bind(4);
                }
                Err(_) => k = 0,
            }
        }
        if k != self.prefix_k {
            self.prefix_k = k;
            self.prefix_ok.fill(false);
        }
    }

    /// Whether anything is to be composited. (Tiles staged ahead of being shown go up with
    /// the next frame drawn, whenever that is.)
    pub fn pending(&self) -> bool {
        self.any_dirty
    }

    /// The frame's description written for the shader; the tiles to composite listed.
    fn write_frame(&mut self) -> u32 {
        let n = self.ntiles;
        let words =
            unsafe { std::slice::from_raw_parts_mut(self.frame.ptr as *mut u32, self.frame_words) };
        let mut nd = 0usize;
        if self.any_dirty {
            for t in 0..n {
                if self.dirty[t] {
                    let rebuild = self.prefix_k > 0 && !self.prefix_ok[t];
                    words[DIRTY_AT + nd] = t as u32 | if rebuild { REBUILD } else { 0 };
                    nd += 1;
                    self.dirty[t] = false;
                    self.prefix_ok[t] = true;
                }
            }
            self.any_dirty = false;
        }
        words[0] = self.width;
        words[1] = self.height;
        words[2] = self.tiles_x;
        words[3] = n as u32;
        words[4] = self.layers.len() as u32;
        words[5] = self.onion.len() as u32;
        words[6] = nd as u32;
        words[7] = self.prefix_k as u32;
        for (l, x) in self.layers.iter().enumerate() {
            let at = LAYERS_AT + l * 4;
            words[at] = x.opacity.to_bits();
            words[at + 1] = x.blend;
            words[at + 2] = x.visible as u32 | (x.clip as u32) << 1;
        }
        for (e, o) in self.onion.iter().enumerate() {
            let at = ONIONS_AT + e * 4;
            words[at] = o.tint[0].to_bits();
            words[at + 1] = o.tint[1].to_bits();
            words[at + 2] = o.tint[2].to_bits();
            words[at + 3] = (o.opacity.min(255) as f32 / 255.0).to_bits();
        }
        // the tables: the slots as `set_frame` left them
        let tables = DIRTY_AT + n;
        words[tables..tables + self.shown.len()].copy_from_slice(&self.shown);
        self.done = words[DIRTY_AT..DIRTY_AT + nd]
            .iter()
            .map(|t| t & !REBUILD)
            .collect();
        nd as u32
    }

    /// Record this frame's uploads and compositing into `cmd` (the picture ends in `out`,
    /// ready to be copied; [`Compositor::done`] lists its tiles).
    ///
    /// SAFETY: `cmd` is recording on this device; nothing else uses these buffers meanwhile.
    pub(crate) unsafe fn record(&mut self, d: &ash::Device, cmd: vk::CommandBuffer) {
        let uploads = !self.staged.is_empty();
        unsafe { self.record_uploads(d, cmd) };
        self.staged.clear();
        self.staged_at.clear();
        let nd = self.write_frame();
        if nd == 0 {
            return;
        }
        let barrier = |src: vk::AccessFlags, dst: vk::AccessFlags, from, to| unsafe {
            d.cmd_pipeline_barrier(
                cmd,
                from,
                to,
                vk::DependencyFlags::empty(),
                &[vk::MemoryBarrier::default()
                    .src_access_mask(src)
                    .dst_access_mask(dst)],
                &[],
                &[],
            )
        };
        if uploads {
            barrier(
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
            );
        }
        unsafe {
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline);
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.layout,
                0,
                &[self.set],
                &[],
            );
            d.cmd_dispatch(cmd, 8, 8, nd);
        }
        barrier(
            vk::AccessFlags::SHADER_WRITE,
            vk::AccessFlags::TRANSFER_READ,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::TRANSFER,
        );
    }

    /// The tiles composited last, as a copy from `out` into an image of the picture's size:
    /// one rectangle around them (`out` holds the whole picture; one copy is much quicker
    /// than one a tile).
    pub(crate) fn regions(&self) -> Vec<vk::BufferImageCopy> {
        if self.done.is_empty() {
            return Vec::new();
        }
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for &t in &self.done {
            let (x, y) = ((t % self.tiles_x) * TILE, (t / self.tiles_x) * TILE);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max((x + TILE).min(self.width));
            y1 = y1.max((y + TILE).min(self.height));
        }
        vec![
            vk::BufferImageCopy::default()
                .buffer_offset((y0 as u64 * self.width as u64 + x0 as u64) * 4)
                .buffer_row_length(self.width)
                .image_subresource(super::COLOR_LAYERS)
                .image_offset(vk::Offset3D {
                    x: x0 as i32,
                    y: y0 as i32,
                    z: 0,
                })
                .image_extent(vk::Extent3D {
                    width: x1 - x0,
                    height: y1 - y0,
                    depth: 1,
                }),
        ]
    }

    /// Composite what's pending now and wait (for measuring).
    pub fn composite_pending(&mut self, gpu: &Gpu) -> Result<()> {
        let r = gpu.run(|d, cmd| unsafe { self.record(d, cmd) });
        self.done.clear();
        r
    }

    /// Composite every tile now and wait (for measuring: how long the GPU takes).
    pub fn composite_all(&mut self, gpu: &Gpu) -> Result<()> {
        self.invalidate_all();
        let r = gpu.run(|d, cmd| unsafe { self.record(d, cmd) });
        self.done.clear();
        r
    }

    /// The picture as it is on the GPU now (straight RGBA8, rows packed): for checks.
    /// Composites what's pending first.
    pub fn read_back(&mut self, gpu: &Gpu) -> Result<Vec<u8>> {
        let size = self.out.size;
        let host = Buf::new(gpu, size, vk::BufferUsageFlags::TRANSFER_DST, true)?;
        let out = self.out.buffer;
        let res = gpu.run(|d, cmd| unsafe {
            self.record(d, cmd);
            d.cmd_copy_buffer(
                cmd,
                out,
                host.buffer,
                &[vk::BufferCopy::default().size(size)],
            );
        });
        self.done.clear();
        let px = res.map(|_| {
            // SAFETY: the mapped buffer holds `size` bytes, written and waited for
            let b = unsafe { std::slice::from_raw_parts(host.ptr as *const u8, size as usize) };
            b.as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect()
        });
        host.destroy(&self.device);
        px
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        // SAFETY: made by this device, destroyed once, after the GPU is idle
        unsafe {
            let d = &self.device;
            let _ = d.device_wait_idle();
            for b in [
                &self.atlas,
                &self.staging,
                &self.frame,
                &self.lut,
                &self.out,
                &self.prefix,
            ] {
                b.destroy(d);
            }
            d.destroy_descriptor_pool(self.pool, None);
            d.destroy_pipeline(self.pipeline, None);
            d.destroy_pipeline_layout(self.layout, None);
            d.destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}
