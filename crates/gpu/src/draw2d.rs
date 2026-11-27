//! The interface's renderer: a list of quads (rounded rectangles, glyphs, pictures) drawn
//! into a frame with one shader (`shaders/quads.wgsl`), glyphs and pictures from an atlas.

use std::ffi::c_void;

use ash::vk;

use super::{COLOR_RANGE, Canvas, Ctx, DmabufImage, Error, Gpu, Result};

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
                    descriptor_count: 1,
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
        self.draw_layered(gpu, frame, clear, quads, quads.len(), None)
    }

    /// Draw `quads[..below]`, then the canvas copied into `at` (x, y, w, h in the frame's
    /// pixels; its changes go up first), then the rest of `quads` over it.
    pub fn draw_layered(
        &mut self,
        gpu: &Gpu,
        frame: &DmabufImage,
        clear: [f32; 4],
        quads: &[Quad],
        below: usize,
        canvas: Option<(&mut Canvas, [i32; 4])>,
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
        let below = below.min(quads.len());
        let under = canvas.map(|(c, at)| {
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
            // the part of the canvas that fits at `at` inside the frame
            let (x, y) = (at[0].max(0), at[1].max(0));
            let w = (at[2].min(c.width as i32))
                .min(frame.width as i32 - x)
                .max(0) as u32;
            let h = (at[3].min(c.height as i32))
                .min(frame.height as i32 - y)
                .max(0) as u32;
            (c.image, c.staging, regions, was_ready, [x, y], [w, h])
        });
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
            pass(0, below, vk::AttachmentLoadOp::CLEAR);
            if let Some((cimg, cstaging, cregions, was_ready, [x, y], [w, h])) = &under {
                let cfrom = if *was_ready {
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                } else {
                    vk::ImageLayout::UNDEFINED
                };
                if !cregions.is_empty() {
                    Gpu::barrier(
                        d,
                        cmd,
                        *cimg,
                        (
                            cfrom,
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
                        *cstaging,
                        *cimg,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        cregions,
                    );
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
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            vk::AccessFlags::TRANSFER_READ,
                            vk::PipelineStageFlags::TRANSFER,
                            vk::QUEUE_FAMILY_IGNORED,
                        ),
                    );
                }
                if (*was_ready || !cregions.is_empty()) && *w > 0 && *h > 0 {
                    Gpu::barrier(
                        d,
                        cmd,
                        frame.image,
                        (
                            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                            vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                            vk::QUEUE_FAMILY_IGNORED,
                        ),
                        (
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            vk::AccessFlags::TRANSFER_WRITE,
                            vk::PipelineStageFlags::TRANSFER,
                            vk::QUEUE_FAMILY_IGNORED,
                        ),
                    );
                    let copy = vk::ImageCopy::default()
                        .src_subresource(super::COLOR_LAYERS)
                        .dst_subresource(super::COLOR_LAYERS)
                        .dst_offset(vk::Offset3D { x: *x, y: *y, z: 0 })
                        .extent(vk::Extent3D {
                            width: *w,
                            height: *h,
                            depth: 1,
                        });
                    d.cmd_copy_image(
                        cmd,
                        *cimg,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        frame.image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[copy],
                    );
                    Gpu::barrier(
                        d,
                        cmd,
                        frame.image,
                        (
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            vk::AccessFlags::TRANSFER_WRITE,
                            vk::PipelineStageFlags::TRANSFER,
                            vk::QUEUE_FAMILY_IGNORED,
                        ),
                        (
                            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                            vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                                | vk::AccessFlags::COLOR_ATTACHMENT_READ,
                            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                            vk::QUEUE_FAMILY_IGNORED,
                        ),
                    );
                }
            }
            if quads.len() > below || under.is_some() {
                pass(below, quads.len(), vk::AttachmentLoadOp::LOAD);
            }
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
