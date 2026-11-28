//! The native app's GPU, through Vulkan (the `ash` bindings, the system's loader opened at
//! run time): a device on the best GPU, and images the compositor can show as they are
//! (exported as dmabufs, tiled with a DRM format modifier both sides support).
//!
//! No Vulkan on the machine (no loader, or no device) is an error, not a crash: tests skip.

#![cfg(target_os = "linux")]

use std::ffi::CStr;
use std::os::fd::{FromRawFd, OwnedFd};

use ash::{ext, khr, vk};

#[derive(Debug)]
pub enum Error {
    /// no Vulkan loader on this machine
    NoLoader(String),
    /// no GPU device that can do what's needed
    NoDevice(String),
    /// a Vulkan call failed
    Vk(&'static str, vk::Result),
    Missing(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoLoader(e) => write!(f, "no Vulkan loader: {e}"),
            Error::NoDevice(e) => write!(f, "no usable GPU: {e}"),
            Error::Vk(call, r) => write!(f, "{call} failed ({r})"),
            Error::Missing(what) => write!(f, "missing: {what}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

trait Ctx<T> {
    fn ctx(self, call: &'static str) -> Result<T>;
}
impl<T> Ctx<T> for std::result::Result<T, vk::Result> {
    fn ctx(self, call: &'static str) -> Result<T> {
        self.map_err(|e| Error::Vk(call, e))
    }
}

/// The device extensions needed to hand images to the compositor.
mod draw2d;
mod layers;
pub use draw2d::{ATLAS_SIZE, Quad, Renderer2d};
pub use layers::{
    Compositor, GpuLayer, MAX_LAYERS as MAX_GPU_LAYERS, OnionLayer, TILE_BYTES, TilePx, tile_hash,
};

const DEVICE_EXTENSIONS: &[&CStr] = &[
    khr::external_memory_fd::NAME,
    ext::external_memory_dma_buf::NAME,
    ext::image_drm_format_modifier::NAME,
    ext::queue_family_foreign::NAME,
    ext::physical_device_drm::NAME,
];

/// What the chosen GPU is.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub discrete: bool,
    pub api_version: (u32, u32, u32),
    /// its DRM render node (major, minor): matched against the compositor's main device
    pub render_node: Option<(i64, i64)>,
}

/// A GPU, ready to work: one queue that does graphics and transfers.
pub struct Gpu {
    _entry: ash::Entry,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: ash::Device,
    memory_fd: khr::external_memory_fd::Device,
    modifier_fns: ext::image_drm_format_modifier::Device,
    queue: vk::Queue,
    family: u32,
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    memory: vk::PhysicalDeviceMemoryProperties,
    pub info: GpuInfo,
}

impl Gpu {
    /// The best GPU here (a discrete one first) that can export images to the compositor.
    pub fn new() -> Result<Gpu> {
        // SAFETY: the loader is the system's; every call below follows the Vulkan spec
        let entry = unsafe { ash::Entry::load() }.map_err(|e| Error::NoLoader(e.to_string()))?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"PencilPilot")
            .engine_name(c"PencilPilot")
            .api_version(vk::make_api_version(0, 1, 3, 0));
        let instance = match unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app),
                None,
            )
        } {
            Ok(i) => i,
            // a loader with no driver behind it (as on machines without a GPU)
            Err(vk::Result::ERROR_INCOMPATIBLE_DRIVER) => {
                return Err(Error::NoDevice("no Vulkan driver".into()));
            }
            Err(e) => return Err(Error::Vk("vkCreateInstance", e)),
        };
        let mut devices =
            unsafe { instance.enumerate_physical_devices() }.ctx("vkEnumeratePhysicalDevices")?;
        let rank =
            |p: vk::PhysicalDevice| match unsafe { instance.get_physical_device_properties(p) }
                .device_type
            {
                vk::PhysicalDeviceType::DISCRETE_GPU => 0,
                vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
                vk::PhysicalDeviceType::CPU => 3,
                _ => 2,
            };
        devices.sort_by_key(|&p| rank(p));
        let mut why = Vec::new();
        for physical in devices {
            match Self::open(&entry, &instance, physical) {
                Ok((device, family, info)) => {
                    return Self::finish(entry, instance, physical, device, family, info);
                }
                Err(e) => why.push(e.to_string()),
            }
        }
        unsafe { instance.destroy_instance(None) };
        Err(Error::NoDevice(if why.is_empty() {
            "no devices".into()
        } else {
            why.join("; ")
        }))
    }

    fn open(
        _entry: &ash::Entry,
        instance: &ash::Instance,
        physical: vk::PhysicalDevice,
    ) -> Result<(ash::Device, u32, GpuInfo)> {
        let props = unsafe { instance.get_physical_device_properties(physical) };
        let name = props
            .device_name_as_c_str()
            .map_or("?".into(), |n| n.to_string_lossy().into_owned());
        let exts = unsafe { instance.enumerate_device_extension_properties(physical) }
            .ctx("vkEnumerateDeviceExtensionProperties")?;
        for want in DEVICE_EXTENSIONS {
            if !exts
                .iter()
                .any(|e| e.extension_name_as_c_str() == Ok(*want))
            {
                return Err(Error::NoDevice(format!(
                    "{name} has no {}",
                    want.to_string_lossy()
                )));
            }
        }
        let family = unsafe { instance.get_physical_device_queue_family_properties(physical) }
            .iter()
            .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .ok_or_else(|| Error::NoDevice(format!("{name} has no graphics queue")))?
            as u32;
        let prio = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&prio)];
        let ext_ptrs: Vec<_> = DEVICE_EXTENSIONS.iter().map(|e| e.as_ptr()).collect();
        // dynamic rendering (Vulkan 1.3): the interface draws without render-pass objects
        let mut v13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let dci = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&ext_ptrs)
            .push_next(&mut v13);
        let device =
            unsafe { instance.create_device(physical, &dci, None) }.ctx("vkCreateDevice")?;

        let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
        let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
        unsafe { instance.get_physical_device_properties2(physical, &mut p2) };
        let v = props.api_version;
        let info = GpuInfo {
            name,
            discrete: props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU,
            api_version: (
                vk::api_version_major(v),
                vk::api_version_minor(v),
                vk::api_version_patch(v),
            ),
            render_node: (drm.has_render != 0).then_some((drm.render_major, drm.render_minor)),
        };
        Ok((device, family, info))
    }

    fn finish(
        entry: ash::Entry,
        instance: ash::Instance,
        physical: vk::PhysicalDevice,
        device: ash::Device,
        family: u32,
        info: GpuInfo,
    ) -> Result<Gpu> {
        let queue = unsafe { device.get_device_queue(family, 0) };
        let pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(family),
                None,
            )
        }
        .ctx("vkCreateCommandPool")?;
        let cmd = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }
        .ctx("vkAllocateCommandBuffers")?[0];
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .ctx("vkCreateFence")?;
        let memory = unsafe { instance.get_physical_device_memory_properties(physical) };
        let memory_fd = khr::external_memory_fd::Device::new(&instance, &device);
        let modifier_fns = ext::image_drm_format_modifier::Device::new(&instance, &device);
        Ok(Gpu {
            _entry: entry,
            instance,
            physical,
            device,
            memory_fd,
            modifier_fns,
            queue,
            family,
            pool,
            cmd,
            fence,
            memory,
            info,
        })
    }

    /// The DRM format modifiers this GPU can render BGRA8 into and export, single-plane
    /// (most compositors take those everywhere).
    pub fn modifiers(&self) -> Vec<u64> {
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
        let mut fp = vk::FormatProperties2::default().push_next(&mut list);
        unsafe {
            self.instance.get_physical_device_format_properties2(
                self.physical,
                vk::Format::B8G8R8A8_UNORM,
                &mut fp,
            )
        };
        let mut mods = vec![
            vk::DrmFormatModifierPropertiesEXT::default();
            list.drm_format_modifier_count as usize
        ];
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
            .drm_format_modifier_properties(&mut mods);
        let mut fp = vk::FormatProperties2::default().push_next(&mut list);
        unsafe {
            self.instance.get_physical_device_format_properties2(
                self.physical,
                vk::Format::B8G8R8A8_UNORM,
                &mut fp,
            )
        };
        let need = vk::FormatFeatureFlags::TRANSFER_DST | vk::FormatFeatureFlags::COLOR_ATTACHMENT;
        mods.iter()
            .filter(|m| {
                m.drm_format_modifier_plane_count == 1
                    && m.drm_format_modifier_tiling_features.contains(need)
            })
            .map(|m| m.drm_format_modifier)
            .collect()
    }

    fn memory_type(&self, bits: u32, want: vk::MemoryPropertyFlags) -> Option<u32> {
        (0..self.memory.memory_type_count).find(|&t| {
            bits & (1 << t) != 0
                && self.memory.memory_types[t as usize]
                    .property_flags
                    .contains(want)
        })
    }

    /// Record commands, run them, and wait until they're done.
    fn run(&self, record: impl FnOnce(&ash::Device, vk::CommandBuffer)) -> Result<()> {
        let d = &self.device;
        unsafe {
            d.reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty())
                .ctx("vkResetCommandBuffer")?;
            d.begin_command_buffer(
                self.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .ctx("vkBeginCommandBuffer")?;
            record(d, self.cmd);
            d.end_command_buffer(self.cmd).ctx("vkEndCommandBuffer")?;
            let cmds = [self.cmd];
            d.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&cmds)],
                self.fence,
            )
            .ctx("vkQueueSubmit")?;
            d.wait_for_fences(&[self.fence], true, 5_000_000_000)
                .ctx("vkWaitForFences")?;
            d.reset_fences(&[self.fence]).ctx("vkResetFences")?;
        }
        Ok(())
    }

    /// (layout, access, stage, queue family) → (layout, access, stage, queue family)
    fn barrier(
        d: &ash::Device,
        cmd: vk::CommandBuffer,
        image: vk::Image,
        from: (
            vk::ImageLayout,
            vk::AccessFlags,
            vk::PipelineStageFlags,
            u32,
        ),
        to: (
            vk::ImageLayout,
            vk::AccessFlags,
            vk::PipelineStageFlags,
            u32,
        ),
    ) {
        let b = vk::ImageMemoryBarrier::default()
            .src_access_mask(from.1)
            .dst_access_mask(to.1)
            .old_layout(from.0)
            .new_layout(to.0)
            .src_queue_family_index(from.3)
            .dst_queue_family_index(to.3)
            .image(image)
            .subresource_range(COLOR_RANGE);
        unsafe {
            d.cmd_pipeline_barrier(
                cmd,
                from.2,
                to.2,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[b],
            )
        };
    }

    /// Fill an image with one colour (RGBA, 0–1) and hand it to the compositor.
    pub fn clear(&self, image: &DmabufImage, rgba: [f32; 4]) -> Result<()> {
        let fam = self.family;
        self.run(|d, cmd| {
            Self::barrier(
                d,
                cmd,
                image.image,
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::AccessFlags::empty(),
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::QUEUE_FAMILY_IGNORED,
                ),
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::QUEUE_FAMILY_IGNORED,
                ),
            );
            let c = vk::ClearColorValue { float32: rgba };
            unsafe {
                d.cmd_clear_color_image(
                    cmd,
                    image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &c,
                    &[COLOR_RANGE],
                )
            };
            // released to the compositor (a "foreign" queue family)
            Self::barrier(
                d,
                cmd,
                image.image,
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::PipelineStageFlags::TRANSFER,
                    fam,
                ),
                (
                    vk::ImageLayout::GENERAL,
                    vk::AccessFlags::empty(),
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::QUEUE_FAMILY_FOREIGN_EXT,
                ),
            );
        })
    }

    /// An image's pixels, read back (BGRA, rows packed): for tests.
    pub fn read_back(&self, image: &DmabufImage) -> Result<Vec<u8>> {
        let d = &self.device;
        let size = (image.width * image.height * 4) as u64;
        unsafe {
            let buf = d
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(size)
                        .usage(vk::BufferUsageFlags::TRANSFER_DST),
                    None,
                )
                .ctx("vkCreateBuffer")?;
            let req = d.get_buffer_memory_requirements(buf);
            let ty = self
                .memory_type(
                    req.memory_type_bits,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
                .ok_or(Error::Missing("host-visible memory"))?;
            let mem = d
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(ty),
                    None,
                )
                .ctx("vkAllocateMemory")?;
            d.bind_buffer_memory(buf, mem, 0)
                .ctx("vkBindBufferMemory")?;
            let fam = self.family;
            self.run(|d, cmd| {
                // taken back from the compositor
                Self::barrier(
                    d,
                    cmd,
                    image.image,
                    (
                        vk::ImageLayout::GENERAL,
                        vk::AccessFlags::empty(),
                        vk::PipelineStageFlags::TOP_OF_PIPE,
                        vk::QUEUE_FAMILY_FOREIGN_EXT,
                    ),
                    (
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::AccessFlags::TRANSFER_READ,
                        vk::PipelineStageFlags::TRANSFER,
                        fam,
                    ),
                );
                let region = vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .image_extent(vk::Extent3D {
                        width: image.width,
                        height: image.height,
                        depth: 1,
                    });
                d.cmd_copy_image_to_buffer(
                    cmd,
                    image.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buf,
                    &[region],
                );
                Self::barrier(
                    d,
                    cmd,
                    image.image,
                    (
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::AccessFlags::TRANSFER_READ,
                        vk::PipelineStageFlags::TRANSFER,
                        fam,
                    ),
                    (
                        vk::ImageLayout::GENERAL,
                        vk::AccessFlags::empty(),
                        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                        vk::QUEUE_FAMILY_FOREIGN_EXT,
                    ),
                );
            })?;
            let p = d
                .map_memory(mem, 0, size, vk::MemoryMapFlags::empty())
                .ctx("vkMapMemory")?;
            let out = std::slice::from_raw_parts(p as *const u8, size as usize).to_vec();
            d.unmap_memory(mem);
            d.destroy_buffer(buf, None);
            d.free_memory(mem, None);
            Ok(out)
        }
    }
}

const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: everything made by this Gpu, destroyed once, after the GPU is idle
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// An image in GPU memory the compositor can show as it is: a dmabuf with a DRM format
/// modifier, one plane, BGRA8 (DRM's ARGB8888 / XRGB8888). Drop it before its Gpu.
pub struct DmabufImage {
    device: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub width: u32,
    pub height: u32,
    /// the modifier the driver chose, from those offered
    pub modifier: u64,
    /// the plane's offset and row stride in the dmabuf, in bytes
    pub offset: u32,
    pub stride: u32,
}

impl DmabufImage {
    /// A new image, its tiling one of `allowed` (the compositor's modifiers; empty: any of
    /// the GPU's).
    pub fn new(gpu: &Gpu, width: u32, height: u32, allowed: &[u64]) -> Result<DmabufImage> {
        let mut mods = gpu.modifiers();
        if !allowed.is_empty() {
            mods.retain(|m| allowed.contains(m));
        }
        if mods.is_empty() {
            return Err(Error::Missing(
                "a DRM format modifier the GPU and the compositor share",
            ));
        }
        let d = &gpu.device;
        let mut list =
            vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&mods);
        let mut ext = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let ici = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(
                vk::ImageUsageFlags::TRANSFER_DST
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::COLOR_ATTACHMENT,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut ext)
            .push_next(&mut list);
        let image = unsafe { d.create_image(&ici, None) }.ctx("vkCreateImage")?;
        let req = unsafe { d.get_image_memory_requirements(image) };
        let Some(ty) = gpu
            .memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)
            .or_else(|| gpu.memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::empty()))
        else {
            unsafe { d.destroy_image(image, None) };
            return Err(Error::Missing("memory for the image"));
        };
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let mut export = vk::ExportMemoryAllocateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mai = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(ty)
            .push_next(&mut export)
            .push_next(&mut dedicated);
        let memory = match unsafe { d.allocate_memory(&mai, None) } {
            Ok(m) => m,
            Err(e) => {
                unsafe { d.destroy_image(image, None) };
                return Err(Error::Vk("vkAllocateMemory", e));
            }
        };
        // from here, dropping `img` frees both
        let mut img = DmabufImage {
            device: d.clone(),
            image,
            memory,
            width,
            height,
            modifier: 0,
            offset: 0,
            stride: 0,
        };
        unsafe { d.bind_image_memory(image, memory, 0) }.ctx("vkBindImageMemory")?;
        let mut mp = vk::ImageDrmFormatModifierPropertiesEXT::default();
        unsafe {
            gpu.modifier_fns
                .get_image_drm_format_modifier_properties(image, &mut mp)
        }
        .ctx("vkGetImageDrmFormatModifierPropertiesEXT")?;
        let layout = unsafe {
            d.get_image_subresource_layout(
                image,
                vk::ImageSubresource {
                    aspect_mask: vk::ImageAspectFlags::MEMORY_PLANE_0_EXT,
                    mip_level: 0,
                    array_layer: 0,
                },
            )
        };
        img.modifier = mp.drm_format_modifier;
        img.offset = layout.offset as u32;
        img.stride = layout.row_pitch as u32;
        Ok(img)
    }

    /// A dmabuf descriptor for the image's memory, to give the compositor.
    pub fn export(&self, gpu: &Gpu) -> Result<OwnedFd> {
        let info = vk::MemoryGetFdInfoKHR::default()
            .memory(self.memory)
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let fd = unsafe { gpu.memory_fd.get_memory_fd(&info) }.ctx("vkGetMemoryFdKHR")?;
        // SAFETY: a new descriptor, ours to own
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

impl Drop for DmabufImage {
    fn drop(&mut self) {
        // SAFETY: made by this device; the Gpu outlives its images
        unsafe {
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

/// DRM's fourcc for BGRA8 in memory (little-endian ARGB8888), without and with alpha.
pub const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
pub const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");

/// The picture being drawn, kept on the GPU. Changed rectangles go up from the CPU (where
/// the brush engine paints) and each frame copies the picture into the image on screen.
pub struct Canvas {
    device: ash::Device,
    image: vk::Image,
    view: vk::ImageView,
    /// this picture, among every one made (a new one can get an old one's handles back)
    pub(crate) serial: u64,
    memory: vk::DeviceMemory,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    mapped: *mut u8,
    pub width: u32,
    pub height: u32,
    /// rectangles written to the staging buffer, not yet copied to the image
    pending: Vec<[u32; 4]>,
    /// the image has been given its first contents
    ready: bool,
}

static CANVAS_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Canvas {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Result<Canvas> {
        let d = &gpu.device;
        unsafe {
            let image = d
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::B8G8R8A8_UNORM)
                        .extent(vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(
                            vk::ImageUsageFlags::TRANSFER_DST
                                | vk::ImageUsageFlags::TRANSFER_SRC
                                | vk::ImageUsageFlags::SAMPLED,
                        )
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .ctx("vkCreateImage")?;
            let req = d.get_image_memory_requirements(image);
            let ty = gpu
                .memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)
                .ok_or(Error::Missing("GPU memory for the canvas"))?;
            let memory = d
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(ty),
                    None,
                )
                .ctx("vkAllocateMemory")?;
            d.bind_image_memory(image, memory, 0)
                .ctx("vkBindImageMemory")?;
            let view = d
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(vk::Format::B8G8R8A8_UNORM)
                        .subresource_range(COLOR_RANGE),
                    None,
                )
                .ctx("vkCreateImageView")?;
            let size = width as u64 * height as u64 * 4;
            let staging = d
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(size)
                        .usage(vk::BufferUsageFlags::TRANSFER_SRC),
                    None,
                )
                .ctx("vkCreateBuffer")?;
            let req = d.get_buffer_memory_requirements(staging);
            let ty = gpu
                .memory_type(
                    req.memory_type_bits,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
                .ok_or(Error::Missing("host-visible memory"))?;
            let staging_memory = d
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(ty),
                    None,
                )
                .ctx("vkAllocateMemory")?;
            d.bind_buffer_memory(staging, staging_memory, 0)
                .ctx("vkBindBufferMemory")?;
            let mapped = d
                .map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty())
                .ctx("vkMapMemory")? as *mut u8;
            Ok(Canvas {
                device: d.clone(),
                image,
                view,
                serial: CANVAS_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                memory,
                staging,
                staging_memory,
                mapped,
                width,
                height,
                pending: Vec::new(),
                ready: false,
            })
        }
    }

    /// Take a rectangle of `rgba` (the whole picture, straight RGBA8, `width` × `height`):
    /// it goes up with the next frame.
    pub fn upload(&mut self, rgba: &[u8], x: u32, y: u32, w: u32, h: u32) {
        let (x1, y1) = ((x + w).min(self.width), (y + h).min(self.height));
        if x >= x1 || y >= y1 {
            return;
        }
        assert_eq!(rgba.len(), (self.width * self.height * 4) as usize);
        let stride = self.width as usize * 4;
        for row in y..y1 {
            let a = row as usize * stride + x as usize * 4;
            let b = row as usize * stride + x1 as usize * 4;
            // SAFETY: inside the mapped staging buffer, the same size and layout as `rgba`
            let dst = unsafe { std::slice::from_raw_parts_mut(self.mapped.add(a), b - a) };
            // RGBA → BGRA (the screen's order)
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(rgba[a..b].as_chunks::<4>().0)
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        }
        self.pending.push([x, y, x1 - x, y1 - y]);
    }

    /// The whole picture, to start with (or after a big change).
    pub fn upload_all(&mut self, rgba: &[u8]) {
        self.pending.clear();
        self.upload(rgba, 0, 0, self.width, self.height);
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: made by this device; the Gpu outlives its canvas
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.unmap_memory(self.staging_memory);
            self.device.destroy_buffer(self.staging, None);
            self.device.free_memory(self.staging_memory, None);
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

impl Gpu {
    /// A frame: what changed on the canvas goes up, the canvas is copied into `frame` (the
    /// rest filled with `around`), and the frame is handed to the compositor. Returns when
    /// the GPU has finished.
    pub fn show(&self, canvas: &mut Canvas, frame: &DmabufImage, around: [f32; 4]) -> Result<()> {
        let fam = self.family;
        let regions: Vec<vk::BufferImageCopy> = canvas
            .pending
            .drain(..)
            .map(|[x, y, w, h]| {
                vk::BufferImageCopy::default()
                    .buffer_offset((y as u64 * canvas.width as u64 + x as u64) * 4)
                    .buffer_row_length(canvas.width)
                    .image_subresource(COLOR_LAYERS)
                    .image_offset(vk::Offset3D {
                        x: x as i32,
                        y: y as i32,
                        z: 0,
                    })
                    .image_extent(vk::Extent3D {
                        width: w,
                        height: h,
                        depth: 1,
                    })
            })
            .collect();
        let was_ready = canvas.ready;
        canvas.ready = true;
        let (cw, ch) = (
            canvas.width.min(frame.width),
            canvas.height.min(frame.height),
        );
        let fills = frame.width > canvas.width || frame.height > canvas.height;
        self.run(|d, cmd| unsafe {
            // the canvas: take the changes
            let from = if was_ready {
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL
            } else {
                vk::ImageLayout::UNDEFINED
            };
            if !regions.is_empty() {
                Self::barrier(
                    d,
                    cmd,
                    canvas.image,
                    (
                        from,
                        vk::AccessFlags::TRANSFER_READ,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                );
                d.cmd_copy_buffer_to_image(
                    cmd,
                    canvas.staging,
                    canvas.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &regions,
                );
                Self::barrier(
                    d,
                    cmd,
                    canvas.image,
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                    (
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::AccessFlags::TRANSFER_READ,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                );
            }
            // the frame: back from the compositor (its old contents dropped), filled, handed back
            Self::barrier(
                d,
                cmd,
                frame.image,
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::AccessFlags::empty(),
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::QUEUE_FAMILY_FOREIGN_EXT,
                ),
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::PipelineStageFlags::TRANSFER,
                    fam,
                ),
            );
            if fills {
                d.cmd_clear_color_image(
                    cmd,
                    frame.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue { float32: around },
                    &[COLOR_RANGE],
                );
                Self::barrier(
                    d,
                    cmd,
                    frame.image,
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        fam,
                    ),
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        fam,
                    ),
                );
            }
            if was_ready || !regions.is_empty() {
                let copy = vk::ImageCopy::default()
                    .src_subresource(COLOR_LAYERS)
                    .dst_subresource(COLOR_LAYERS)
                    .extent(vk::Extent3D {
                        width: cw,
                        height: ch,
                        depth: 1,
                    });
                d.cmd_copy_image(
                    cmd,
                    canvas.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    frame.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[copy],
                );
            }
            Self::barrier(
                d,
                cmd,
                frame.image,
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::PipelineStageFlags::TRANSFER,
                    fam,
                ),
                (
                    vk::ImageLayout::GENERAL,
                    vk::AccessFlags::empty(),
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::QUEUE_FAMILY_FOREIGN_EXT,
                ),
            );
        })
    }
}

const COLOR_LAYERS: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};
