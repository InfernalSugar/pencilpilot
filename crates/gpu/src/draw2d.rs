//! The interface's renderer: a list of quads (rounded rectangles, glyphs, pictures) drawn
//! into a frame with one shader (`shaders/quads.wgsl`), glyphs and pictures from an atlas.

use std::ffi::c_void;

use ash::vk;

use super::{COLOR_RANGE, Canvas, Compositor, Ctx, DmabufImage, Error, Gpu, Result};

/// The atlas is this many pixels a side (RGBA8, premultiplied).
pub const ATLAS_SIZE: u32 = 2048;

const SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/quads.spv"));

/// One thing to draw. Colours are straight (not premultiplied) RGBA, 0–1; the builders
/// premultiply.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    rect: [f32; 4],
    fill: [f32; 4],
    border_color: [f32; 4],
    uv: [f32; 4],
    params: [f32; 4],
    clip: [f32; 4],
}

fn premul(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

const NO_CLIP: [f32; 4] = [-1e9, -1e9, 1e9, 1e9];

impl Quad {
    /// The canvas (the picture given to [`Renderer2d::draw_layered`]), shown over the
    /// rectangle (x, y, w, h, frame pixels) through `map`: frame pixel → canvas pixel as
    /// `[a, b, c, d, tx, ty]` (`canvas = (a·x + b·y + tx, c·x + d·y + ty)`). Outside the
    /// canvas, `outside` shows. `size` is the canvas's size; `sharp` shows its pixels as
    /// squares (zoomed in) rather than smoothed.
    pub fn canvas(
        rect: [f32; 4],
        map: [f32; 6],
        size: [f32; 2],
        outside: [f32; 4],
        sharp: bool,
    ) -> Quad {
        Quad {
            rect,
            fill: premul(outside),
            border_color: [map[4], map[5], size[0], size[1]],
            uv: [map[0], map[1], map[2], map[3]],
            params: [0.0, if sharp { 1.0 } else { 0.0 }, 0.0, 4.0],
            clip: NO_CLIP,
        }
    }

    /// A canvas quad showing the second picture (see [`Renderer2d::draw_canvases`]).
    pub fn second(mut self) -> Quad {
        self.params[2] = 1.0;
        self
    }

    /// Its rectangle (x, y, w, h), for checks.
    pub fn rect_xywh(&self) -> [f32; 4] {
        self.rect
    }

    /// Its fill colour (premultiplied), for checks.
    pub fn fill_rgba(&self) -> [f32; 4] {
        self.fill
    }

    /// A filled rectangle (pixels).
    pub fn rect(x: f32, y: f32, w: f32, h: f32, fill: [f32; 4]) -> Quad {
        Quad {
            rect: [x, y, w, h],
            fill: premul(fill),
            border_color: [0.0; 4],
            uv: [0.0; 4],
            params: [0.0; 4],
            clip: NO_CLIP,
        }
    }
    /// Rounded corners.
    pub fn radius(mut self, r: f32) -> Quad {
        self.params[0] = r;
        self
    }
    /// A border inside the edge.
    pub fn border(mut self, width: f32, color: [f32; 4]) -> Quad {
        self.params[1] = width;
        self.border_color = premul(color);
        self
    }
    /// Make it a soft shadow (its fill the shadow's colour) fading over `blur` pixels.
    pub fn shadow(mut self, blur: f32) -> Quad {
        self.params[2] = blur.max(0.01);
        self
    }
    /// A glyph: the atlas's alpha at `uv` (u0, v0, u1, v1 in 0–1), in `color`.
    pub fn glyph(x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: [f32; 4]) -> Quad {
        Quad {
            rect: [x, y, w, h],
            fill: premul(color),
            border_color: [0.0; 4],
            uv,
            params: [0.0, 0.0, 0.0, 1.0],
            clip: NO_CLIP,
        }
    }
    /// A picture from the atlas (premultiplied there), at `opacity`.
    pub fn image(x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], opacity: f32) -> Quad {
        Quad {
            rect: [x, y, w, h],
            fill: [opacity; 4],
            border_color: [0.0; 4],
            uv,
            params: [0.0, 0.0, 0.0, 2.0],
            clip: NO_CLIP,
        }
    }
    /// Scaled by `s` (logical to device pixels): position, size, corners, border, shadow
    /// and clip.
    pub fn scaled(mut self, s: f32) -> Quad {
        for v in &mut self.rect {
            *v *= s;
        }
        if self.params[3] == 4.0 {
            // a canvas: its map now takes the scaled pixels; its corners scale
            for v in &mut self.uv {
                *v /= s;
            }
            self.params[0] *= s;
            if self.clip != NO_CLIP {
                for v in &mut self.clip {
                    *v *= s;
                }
            }
            return self;
        }
        self.params[0] *= s;
        self.params[1] *= s;
        self.params[2] *= s;
        if self.clip != NO_CLIP {
            for v in &mut self.clip {
                *v *= s;
            }
        }
        self
    }
    /// Moved by (dx, dy), its clip too.
    pub fn moved(mut self, dx: f32, dy: f32) -> Quad {
        self.rect[0] += dx;
        self.rect[1] += dy;
        if self.params[3] == 4.0 {
            // a canvas: the same canvas pixel shows (dx, dy) further on
            let [a, b, c, d] = self.uv;
            self.border_color[0] -= a * dx + b * dy;
            self.border_color[1] -= c * dx + d * dy;
        }
        if self.clip != NO_CLIP {
            self.clip = [
                self.clip[0] + dx,
                self.clip[1] + dy,
                self.clip[2] + dx,
                self.clip[3] + dy,
            ];
        }
        self
    }
    /// A rectangle filled with a gradient from `from` to `to` (straight RGBA), along
    /// (dx, dy): (1, 0) runs left to right, (0, 1) top to bottom. Takes `radius`, not a
    /// border or shadow.
    pub fn gradient(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        from: [f32; 4],
        to: [f32; 4],
        dir: (f32, f32),
    ) -> Quad {
        let len = (dir.0 * dir.0 + dir.1 * dir.1).sqrt().max(1e-6);
        Quad {
            rect: [x, y, w, h],
            fill: premul(from),
            border_color: premul(to),
            uv: [dir.0 / len, dir.1 / len, 0.0, 0.0],
            params: [0.0, 0.0, 0.0, 3.0],
            clip: NO_CLIP,
        }
    }

    /// Only inside this rectangle (x0, y0, x1, y1).
    pub fn clip(mut self, c: [f32; 4]) -> Quad {
        self.clip = c;
        self
    }
}

/// Draws quad lists into frames.
pub struct Renderer2d {
    device: ash::Device,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    sampler: vk::Sampler,
    atlas: vk::Image,
    atlas_memory: vk::DeviceMemory,
    atlas_view: vk::ImageView,
    /// what bindings 2 and 3 show: a picture's serial (0: the atlas; u64::MAX: nothing yet)
    bound_canvas: [u64; 2],
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_ptr: *mut u8,
    pending: Vec<[u32; 4]>,
    atlas_ready: bool,
    instances: vk::Buffer,
    instances_memory: vk::DeviceMemory,
    instances_ptr: *mut Quad,
    capacity: usize,
}

impl Renderer2d {
    pub fn new(gpu: &Gpu) -> Result<Renderer2d> {
        let d = &gpu.device;
        unsafe {
            let words = ash::util::read_spv(&mut std::io::Cursor::new(SPIRV))
                .map_err(|_| Error::Missing("the shader"))?;
            let module = d
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
                .ctx("vkCreateShaderModule")?;
            let bindings = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                // the canvas and a second picture (the atlas stands in when there's none)
                vk::DescriptorSetLayoutBinding::default()
                    .binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(3)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            ];
            let set_layout = d
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .ctx("vkCreateDescriptorSetLayout")?;
            let push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                .size(8)];
            let set_layouts = [set_layout];
            let layout = d
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&set_layouts)
                        .push_constant_ranges(&push),
                    None,
                )
                .ctx("vkCreatePipelineLayout")?;

            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(module)
                    .name(c"vs"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(module)
                    .name(c"fs"),
            ];
            let binding = [vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(std::mem::size_of::<Quad>() as u32)
                .input_rate(vk::VertexInputRate::INSTANCE)];
            let attrs: Vec<_> = (0..6)
                .map(|i| {
                    vk::VertexInputAttributeDescription::default()
                        .location(i)
                        .binding(0)
                        .format(vk::Format::R32G32B32A32_SFLOAT)
                        .offset(i * 16)
                })
                .collect();
            let vertex = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&binding)
                .vertex_attribute_descriptions(&attrs);
            let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
                .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
            let viewport = vk::PipelineViewportStateCreateInfo::default()
                .viewport_count(1)
                .scissor_count(1);
            let raster = vk::PipelineRasterizationStateCreateInfo::default()
                .polygon_mode(vk::PolygonMode::FILL)
                .cull_mode(vk::CullModeFlags::NONE)
                .line_width(1.0);
            let multisample = vk::PipelineMultisampleStateCreateInfo::default()
                .rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let blend = [vk::PipelineColorBlendAttachmentState::default()
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::ONE)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD)
                .color_write_mask(vk::ColorComponentFlags::RGBA)];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend);
            let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
            let formats = [vk::Format::B8G8R8A8_UNORM];
            let mut rendering =
                vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
            let info = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&vertex)
                .input_assembly_state(&assembly)
                .viewport_state(&viewport)
                .rasterization_state(&raster)
                .multisample_state(&multisample)
                .color_blend_state(&blend)
                .dynamic_state(&dynamic)
                .layout(layout)
                .push_next(&mut rendering);
            let pipeline = d
                .create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
                .map_err(|(_, e)| Error::Vk("vkCreateGraphicsPipelines", e))?[0];
            d.destroy_shader_module(module, None);

            // the atlas
            let atlas = d
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::R8G8B8A8_UNORM)
                        .extent(vk::Extent3D {
                            width: ATLAS_SIZE,
                            height: ATLAS_SIZE,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .ctx("vkCreateImage")?;
            let req = d.get_image_memory_requirements(atlas);
            let ty = gpu
                .memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)
                .ok_or(Error::Missing("GPU memory for the atlas"))?;
            let atlas_memory = d
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(ty),
                    None,
                )
                .ctx("vkAllocateMemory")?;
            d.bind_image_memory(atlas, atlas_memory, 0)
                .ctx("vkBindImageMemory")?;
            let atlas_view = d
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(atlas)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(vk::Format::R8G8B8A8_UNORM)
                        .subresource_range(COLOR_RANGE),
                    None,
                )
                .ctx("vkCreateImageView")?;
            let sampler = d
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .ctx("vkCreateSampler")?;
            let sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: 3,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLER,
                    descriptor_count: 1,
                },
            ];
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
            let img = [vk::DescriptorImageInfo::default()
                .image_view(atlas_view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let smp = [vk::DescriptorImageInfo::default().sampler(sampler)];
            d.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&img),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .image_info(&smp),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(2)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&img),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(3)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&img),
                ],
                &[],
            );
            let size = (ATLAS_SIZE * ATLAS_SIZE * 4) as u64;
            let (staging, staging_memory, staging_ptr) =
                host_buffer(gpu, size, vk::BufferUsageFlags::TRANSFER_SRC)?;
            let (instances, instances_memory, instances_ptr) = host_buffer(
                gpu,
                1024 * std::mem::size_of::<Quad>() as u64,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            Ok(Renderer2d {
                device: d.clone(),
                set_layout,
                layout,
                pipeline,
                pool,
                set,
                sampler,
                atlas,
                atlas_memory,
                atlas_view,
                bound_canvas: [0; 2],
                staging,
                staging_memory,
                staging_ptr: staging_ptr as *mut u8,
                pending: Vec::new(),
                atlas_ready: false,
                instances,
                instances_memory,
                instances_ptr: instances_ptr as *mut Quad,
                capacity: 1024,
            })
        }
    }

    /// Put pixels (premultiplied RGBA8, rows packed) into the atlas at (x, y); they go up
    /// with the next draw.
    pub fn atlas_put(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) {
        assert!(x + w <= ATLAS_SIZE && y + h <= ATLAS_SIZE && rgba.len() == (w * h * 4) as usize);
        for row in 0..h {
            let dst = ((y + row) * ATLAS_SIZE + x) as usize * 4;
            let src = (row * w) as usize * 4;
            // SAFETY: inside the mapped staging buffer, which mirrors the atlas
            unsafe {
                std::ptr::copy_nonoverlapping(
                    rgba[src..].as_ptr(),
                    self.staging_ptr.add(dst),
                    w as usize * 4,
                );
            }
        }
        self.pending.push([x, y, w, h]);
    }

    /// Draw `quads`, in order, into `frame` over `clear`, and hand it to the compositor.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        frame: &DmabufImage,
        clear: [f32; 4],
        quads: &[Quad],
    ) -> Result<()> {
        self.draw_layered(gpu, frame, clear, quads, None)
    }

    /// Draw `quads`, in order, into `frame` over `clear`, with `canvas` (its changes go up
    /// first) available to [`Quad::canvas`] quads, and hand it to the compositor.
    pub fn draw_layered(
        &mut self,
        gpu: &Gpu,
        frame: &DmabufImage,
        clear: [f32; 4],
        quads: &[Quad],
        canvas: Option<&mut Canvas>,
    ) -> Result<()> {
        self.draw_canvases(gpu, frame, clear, quads, canvas, None)
    }

    /// [`Renderer2d::draw_layered`] with a second picture, `second`, for
    /// [`Quad::canvas`] quads marked [`Quad::second`] (e.g. pixels being moved).
    pub fn draw_canvases(
        &mut self,
        gpu: &Gpu,
        frame: &DmabufImage,
        clear: [f32; 4],
        quads: &[Quad],
        canvas: Option<&mut Canvas>,
        second: Option<&mut Canvas>,
    ) -> Result<()> {
        self.draw_composited(gpu, frame, clear, quads, canvas, second, None)
    }

    /// [`Renderer2d::draw_canvases`], with the layers composited on the GPU first: what
    /// `layers` composites goes into `canvas` (after the canvas's own uploads, if any).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_composited(
        &mut self,
        gpu: &Gpu,
        frame: &DmabufImage,
        clear: [f32; 4],
        quads: &[Quad],
        canvas: Option<&mut Canvas>,
        second: Option<&mut Canvas>,
        mut layers: Option<&mut Compositor>,
    ) -> Result<()> {
        if quads.len() > self.capacity {
            let cap = quads.len().next_power_of_two();
            unsafe {
                self.device.destroy_buffer(self.instances, None);
                self.device.free_memory(self.instances_memory, None);
            }
            let (b, m, p) = host_buffer(
                gpu,
                (cap * std::mem::size_of::<Quad>()) as u64,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            (
                self.instances,
                self.instances_memory,
                self.instances_ptr,
                self.capacity,
            ) = (b, m, p as *mut Quad, cap);
        }
        // SAFETY: the buffer holds `capacity` quads; the GPU isn't reading it (run waits)
        unsafe { std::ptr::copy_nonoverlapping(quads.as_ptr(), self.instances_ptr, quads.len()) };
        let d = &self.device;
        let view = unsafe {
            d.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(frame.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::B8G8R8A8_UNORM)
                    .subresource_range(COLOR_RANGE),
                None,
            )
        }
        .ctx("vkCreateImageView")?;
        let regions: Vec<vk::BufferImageCopy> = self
            .pending
            .drain(..)
            .map(|[x, y, w, h]| {
                vk::BufferImageCopy::default()
                    .buffer_offset(((y * ATLAS_SIZE + x) * 4) as u64)
                    .buffer_row_length(ATLAS_SIZE)
                    .image_subresource(super::COLOR_LAYERS)
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
        let first = !self.atlas_ready;
        self.atlas_ready = true;
        // the canvas quads sample: bind these pictures (or the atlas, when there's none)
        // (by serial: a new picture can get a destroyed one's handles back)
        for (binding, want, serial) in [
            (
                2,
                canvas.as_ref().map_or(self.atlas_view, |c| c.view),
                canvas.as_ref().map_or(0, |c| c.serial),
            ),
            (
                3,
                second.as_ref().map_or(self.atlas_view, |c| c.view),
                second.as_ref().map_or(0, |c| c.serial),
            ),
        ] {
            let bound = &mut self.bound_canvas[binding as usize - 2];
            if serial == *bound {
                continue;
            }
            let img = [vk::DescriptorImageInfo::default()
                .image_view(want)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            // SAFETY: the set isn't in use (each draw waits for the GPU)
            unsafe {
                self.device.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default()
                        .dst_set(self.set)
                        .dst_binding(binding)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&img)],
                    &[],
                )
            };
            *bound = serial;
        }
        let has_canvas = canvas.is_some();
        let pictures: Vec<_> = canvas
            .into_iter()
            .chain(second)
            .map(|c| {
                let regions: Vec<vk::BufferImageCopy> = c
                    .pending
                    .drain(..)
                    .map(|[x, y, w, h]| {
                        vk::BufferImageCopy::default()
                            .buffer_offset((y as u64 * c.width as u64 + x as u64) * 4)
                            .buffer_row_length(c.width)
                            .image_subresource(super::COLOR_LAYERS)
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
                let was_ready = c.ready;
                c.ready = true;
                (c.image, vec![(c.staging, regions)], was_ready)
            })
            .collect();
        let mut pictures = pictures;
        if !has_canvas {
            layers = None;
        }
        let fam = gpu.family;
        let (atlas, staging, pipeline, layout, set, instances) = (
            self.atlas,
            self.staging,
            self.pipeline,
            self.layout,
            self.set,
            self.instances,
        );
        let (fw, fh) = (frame.width, frame.height);
        let res = gpu.run(|d, cmd| unsafe {
            // the atlas: cleared the first time, then what's new
            if first || !regions.is_empty() {
                let from = if first {
                    vk::ImageLayout::UNDEFINED
                } else {
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                };
                Gpu::barrier(
                    d,
                    cmd,
                    atlas,
                    (
                        from,
                        vk::AccessFlags::SHADER_READ,
                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                );
                if first {
                    d.cmd_clear_color_image(
                        cmd,
                        atlas,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearColorValue { float32: [0.0; 4] },
                        &[COLOR_RANGE],
                    );
                    Gpu::barrier(
                        d,
                        cmd,
                        atlas,
                        (
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            vk::AccessFlags::TRANSFER_WRITE,
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
                }
                if !regions.is_empty() {
                    d.cmd_copy_buffer_to_image(
                        cmd,
                        staging,
                        atlas,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &regions,
                    );
                }
                Gpu::barrier(
                    d,
                    cmd,
                    atlas,
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                    (
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::AccessFlags::SHADER_READ,
                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                );
            }
            // the frame, back from the compositor, drawn, handed back
            Gpu::barrier(
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
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    fam,
                ),
            );
            let area = vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: fw,
                    height: fh,
                },
            };
            // a rendering pass drawing quads[from..to]: the first clears, the second keeps
            let pass = |from: usize, to: usize, load: vk::AttachmentLoadOp| {
                let attach = [vk::RenderingAttachmentInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .load_op(load)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .clear_value(vk::ClearValue {
                        color: vk::ClearColorValue { float32: clear },
                    })];
                d.cmd_begin_rendering(
                    cmd,
                    &vk::RenderingInfo::default()
                        .render_area(area)
                        .layer_count(1)
                        .color_attachments(&attach),
                );
                d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                d.cmd_set_viewport(
                    cmd,
                    0,
                    &[vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: fw as f32,
                        height: fh as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                );
                d.cmd_set_scissor(cmd, 0, &[area]);
                d.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    layout,
                    0,
                    &[set],
                    &[],
                );
                let screen = [fw as f32, fh as f32];
                let bytes = std::slice::from_raw_parts(screen.as_ptr() as *const u8, 8);
                d.cmd_push_constants(cmd, layout, vk::ShaderStageFlags::VERTEX, 0, bytes);
                d.cmd_bind_vertex_buffers(cmd, 0, &[instances], &[0]);
                if to > from {
                    d.cmd_draw(cmd, 4, (to - from) as u32, 0, from as u32);
                }
                d.cmd_end_rendering(cmd);
            };
            // the layers composited on the GPU go into the canvas, after its own changes
            if let Some(comp) = layers {
                comp.record(d, cmd);
                let regions = comp.regions();
                if !regions.is_empty() {
                    pictures[0].1.push((comp.out.buffer, regions));
                }
            }
            // the pictures' changes go up, then they're read by the shader
            for (cimg, copies, was_ready) in &pictures {
                if copies.iter().all(|c| c.1.is_empty()) && *was_ready {
                    continue;
                }
                let from = if *was_ready {
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                } else {
                    vk::ImageLayout::UNDEFINED
                };
                Gpu::barrier(
                    d,
                    cmd,
                    *cimg,
                    (
                        from,
                        vk::AccessFlags::SHADER_READ,
                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                );
                for (i, (src, regions)) in copies.iter().enumerate() {
                    if regions.is_empty() {
                        continue;
                    }
                    if i > 0 {
                        // (what comes after lands over what came before)
                        Gpu::barrier(
                            d,
                            cmd,
                            *cimg,
                            (
                                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                vk::AccessFlags::TRANSFER_WRITE,
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
                    }
                    d.cmd_copy_buffer_to_image(
                        cmd,
                        *src,
                        *cimg,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        regions,
                    );
                }
                Gpu::barrier(
                    d,
                    cmd,
                    *cimg,
                    (
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                    (
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::AccessFlags::SHADER_READ,
                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                        vk::QUEUE_FAMILY_IGNORED,
                    ),
                );
            }
            pass(0, quads.len(), vk::AttachmentLoadOp::CLEAR);
            Gpu::barrier(
                d,
                cmd,
                frame.image,
                (
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    fam,
                ),
                (
                    vk::ImageLayout::GENERAL,
                    vk::AccessFlags::empty(),
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::QUEUE_FAMILY_FOREIGN_EXT,
                ),
            );
        });
        unsafe { self.device.destroy_image_view(view, None) };
        res
    }
}

impl Drop for Renderer2d {
    fn drop(&mut self) {
        // SAFETY: made by this device, destroyed once, after the GPU is idle
        unsafe {
            let d = &self.device;
            let _ = d.device_wait_idle();
            d.destroy_buffer(self.instances, None);
            d.free_memory(self.instances_memory, None);
            d.destroy_buffer(self.staging, None);
            d.free_memory(self.staging_memory, None);
            d.destroy_sampler(self.sampler, None);
            d.destroy_image_view(self.atlas_view, None);
            d.destroy_image(self.atlas, None);
            d.free_memory(self.atlas_memory, None);
            d.destroy_descriptor_pool(self.pool, None);
            d.destroy_pipeline(self.pipeline, None);
            d.destroy_pipeline_layout(self.layout, None);
            d.destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}

/// A host-visible, coherent buffer, mapped.
fn host_buffer(
    gpu: &Gpu,
    size: u64,
    usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory, *mut c_void)> {
    let d = &gpu.device;
    unsafe {
        let b = d
            .create_buffer(
                &vk::BufferCreateInfo::default().size(size).usage(usage),
                None,
            )
            .ctx("vkCreateBuffer")?;
        let req = d.get_buffer_memory_requirements(b);
        let ty = gpu
            .memory_type(
                req.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )
            .ok_or(Error::Missing("host-visible memory"))?;
        let m = d
            .allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty),
                None,
            )
            .ctx("vkAllocateMemory")?;
        d.bind_buffer_memory(b, m, 0).ctx("vkBindBufferMemory")?;
        let p = d
            .map_memory(m, 0, size, vk::MemoryMapFlags::empty())
            .ctx("vkMapMemory")?;
        Ok((b, m, p))
    }
}
