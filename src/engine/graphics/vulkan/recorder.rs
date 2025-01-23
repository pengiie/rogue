use std::{collections::HashMap, sync::Arc};

use epaint::image;
use nalgebra::Vector3;

use crate::{
    common::color::{Color, ColorSpaceSrgb},
    engine::graphics::backend::{
        Binding, Buffer, ComputePass, ComputePipeline, GfxFilterMode, GfxImageInfo,
        GfxRenderPassAttachment, GraphicsBackendComputePass, GraphicsBackendRecorder,
        GraphicsBackendRenderPass, Image, RasterPipeline, RenderPass, ResourceId, ShaderWriter,
    },
};

use super::device::{VulkanComputePipeline, VulkanContext, VulkanRasterPipeline};

pub struct VulkanRecorder {
    ctx: Arc<VulkanContext>,
    command_buffer: ash::vk::CommandBuffer,
    image_layouts: HashMap<ResourceId<Image>, (ash::vk::ImageLayout, ash::vk::AccessFlags)>,
}

pub struct VulkanImageTransition {
    pub image_id: ResourceId<Image>,
    pub new_layout: ash::vk::ImageLayout,
    pub new_access_flags: ash::vk::AccessFlags,
}

impl VulkanRecorder {
    pub fn new(ctx: &Arc<VulkanContext>, command_buffer: ash::vk::CommandBuffer) -> Self {
        Self {
            ctx: ctx.clone(),
            command_buffer,
            image_layouts: HashMap::new(),
        }
    }

    pub fn begin(&self) {
        unsafe {
            self.ctx.device().begin_command_buffer(
                self.command_buffer,
                &ash::vk::CommandBufferBeginInfo::default()
                    .flags(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        };
    }

    pub fn finish(&self) {
        unsafe { self.ctx.device().end_command_buffer(self.command_buffer) };
    }

    pub fn wait_event(&self, event: ash::vk::Event) {
        unsafe {
            self.ctx.device().cmd_wait_events(
                self.command_buffer,
                &[event],
                ash::vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                ash::vk::PipelineStageFlags::TOP_OF_PIPE,
                &[],
                &[],
                &[],
            )
        }
    }

    pub fn set_event(&self, event: ash::vk::Event) {
        unsafe {
            self.ctx.device().cmd_set_event(
                self.command_buffer,
                event,
                ash::vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            )
        }
    }

    pub fn transition_images(
        &mut self,
        transitions: &[VulkanImageTransition],
        src_stage: Option<ash::vk::PipelineStageFlags>,
        dst_stage: ash::vk::PipelineStageFlags,
    ) {
        const UNDEFINED_LAYOUT_ACCESS: (ash::vk::ImageLayout, ash::vk::AccessFlags) = (
            ash::vk::ImageLayout::UNDEFINED,
            ash::vk::AccessFlags::empty(),
        );

        let mut inferred_src_stage = ash::vk::PipelineStageFlags::empty();

        let mut image_memory_barriers = vec![];
        for VulkanImageTransition {
            image_id,
            new_layout,
            new_access_flags,
        } in transitions
        {
            let (old_layout, access_flags) = self
                .image_layouts
                .get(&image_id)
                .unwrap_or(&UNDEFINED_LAYOUT_ACCESS);

            if *old_layout != *new_layout || !access_flags.contains(*new_access_flags) {
                inferred_src_stage = inferred_src_stage.max(match *old_layout {
                    ash::vk::ImageLayout::UNDEFINED => ash::vk::PipelineStageFlags::TOP_OF_PIPE,
                    ash::vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                    | ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL => {
                        ash::vk::PipelineStageFlags::TRANSFER
                    }
                    // TODO: Track the previous pipeline stage.
                    ash::vk::ImageLayout::GENERAL => ash::vk::PipelineStageFlags::ALL_COMMANDS,
                    _ => todo!(),
                });

                let image = self.ctx.resource_manager().get_image(*image_id);
                let image_memory_barrier = ash::vk::ImageMemoryBarrier::default()
                    .image(image.image)
                    .subresource_range(image.full_subresource_range())
                    .old_layout(*old_layout)
                    .new_layout(*new_layout)
                    .src_access_mask(*access_flags)
                    .dst_access_mask(*new_access_flags);

                image_memory_barriers.push(image_memory_barrier);

                self.image_layouts
                    .insert(*image_id, (*new_layout, *new_access_flags));
            }
        }

        let dependency_flags = if let Some(stage) = src_stage {
            // TODO: Remove when we reorder our image transitions for dynamic rendering since this
            // is quite janky.
            if stage == ash::vk::PipelineStageFlags::FRAGMENT_SHADER {
                ash::vk::DependencyFlags::BY_REGION
            } else {
                ash::vk::DependencyFlags::empty()
            }
        } else {
            ash::vk::DependencyFlags::empty()
        };

        unsafe {
            self.ctx.device().cmd_pipeline_barrier(
                self.command_buffer,
                src_stage.unwrap_or(inferred_src_stage),
                dst_stage,
                dependency_flags,
                &[],
                &[],
                &image_memory_barriers,
            )
        };
    }

    pub fn command_buffer(&self) -> ash::vk::CommandBuffer {
        self.command_buffer
    }
}

impl GraphicsBackendRecorder for VulkanRecorder {
    fn clear_color(&mut self, image_id: ResourceId<Image>, color: Color<ColorSpaceSrgb>) {
        let image = self.ctx.resource_manager().get_image(image_id);
        let mut clear_color_value = ash::vk::ClearColorValue::default();
        clear_color_value.float32 = [color.r(), color.g(), color.b(), 1.0];
        let image_subresource_range = image.full_subresource_range();
        self.transition_images(
            &[VulkanImageTransition {
                image_id,
                new_layout: ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                new_access_flags: ash::vk::AccessFlags::TRANSFER_WRITE,
            }],
            None,
            ash::vk::PipelineStageFlags::TRANSFER,
        );

        unsafe {
            self.ctx.device().cmd_clear_color_image(
                self.command_buffer,
                image.image,
                ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &clear_color_value,
                &[image_subresource_range],
            )
        };
    }

    fn blit(
        &mut self,
        src_id: ResourceId<Image>,
        dst_id: ResourceId<Image>,
        filter_mode: GfxFilterMode,
    ) {
        let src_image = self.ctx.resource_manager().get_image(src_id);
        let dst_image = self.ctx.resource_manager().get_image(dst_id);

        self.transition_images(
            &[
                VulkanImageTransition {
                    image_id: src_id,
                    new_layout: ash::vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    new_access_flags: ash::vk::AccessFlags::TRANSFER_READ,
                },
                VulkanImageTransition {
                    image_id: dst_id,
                    new_layout: ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    new_access_flags: ash::vk::AccessFlags::TRANSFER_WRITE,
                },
            ],
            None,
            ash::vk::PipelineStageFlags::TRANSFER,
        );

        const ZERO_IMAGE_OFFSET: ash::vk::Offset3D = ash::vk::Offset3D { x: 0, y: 0, z: 0 };
        let regions = [ash::vk::ImageBlit::default()
            .src_offsets([ZERO_IMAGE_OFFSET, src_image.full_offset_3d()])
            .src_subresource(src_image.full_subresource_layer())
            .dst_offsets([ZERO_IMAGE_OFFSET, dst_image.full_offset_3d()])
            .dst_subresource(dst_image.full_subresource_layer())];
        unsafe {
            self.ctx.device().cmd_blit_image(
                self.command_buffer,
                src_image.image,
                ash::vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst_image.image,
                ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &regions,
                filter_mode.into(),
            )
        }
    }

    fn begin_compute_pass<'a>(
        &mut self,
        compute_pipeline_id: ResourceId<ComputePipeline>,
    ) -> ComputePass {
        let compute_pipeline = self.ctx.get_compute_pipeline(compute_pipeline_id);
        unsafe {
            self.ctx.device().cmd_bind_pipeline(
                self.command_buffer,
                ash::vk::PipelineBindPoint::COMPUTE,
                compute_pipeline.pipeline,
            )
        };
        Box::new(VulkanComputePass {
            recorder: self,
            pipeline_id: compute_pipeline_id,
            pipeline: compute_pipeline,
            uniforms_bound: false,
        })
    }

    fn begin_render_pass(
        &mut self,
        raster_pipeline_id: ResourceId<RasterPipeline>,
        color_attachments: &[GfxRenderPassAttachment],
        depth_attachment: Option<GfxRenderPassAttachment>,
    ) -> RenderPass {
        let raster_pipeline = self.ctx.get_raster_pipeline(raster_pipeline_id);
        unsafe {
            self.ctx.device().cmd_bind_pipeline(
                self.command_buffer,
                ash::vk::PipelineBindPoint::GRAPHICS,
                raster_pipeline.pipeline,
            )
        };

        let mut image_transitions = Vec::new();
        let mut render_area: Option<ash::vk::Rect2D> = None;
        let color_attachments = color_attachments
            .into_iter()
            .map(|attachment| {
                image_transitions.push(VulkanImageTransition {
                    image_id: attachment.image,
                    new_layout: ash::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    new_access_flags: ash::vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                });

                let image_info = self.ctx.get_image(&attachment.image);
                if let Some(render_area) = &render_area {
                    assert_eq!(
                        render_area.extent, image_info.info.extent,
                        "Each color attachment should have the same extent in a raster pipeline."
                    );
                } else {
                    render_area = Some(ash::vk::Rect2D {
                        offset: ash::vk::Offset2D { x: 0, y: 0 },
                        extent: image_info.info.extent,
                    })
                }
                ash::vk::RenderingAttachmentInfo::default()
                    .image_view(
                        image_info
                            .view
                            .expect("Color attachments should have an image view."),
                    )
                    .image_layout(ash::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .store_op(ash::vk::AttachmentStoreOp::STORE)
                    .load_op(attachment.load_op.into())
            })
            .collect::<Vec<_>>();
        let render_area = render_area.unwrap();

        let mut rendering_info = ash::vk::RenderingInfo::default()
            .color_attachments(&color_attachments)
            .render_area(render_area)
            .layer_count(1);
        let mut depth_attachment_info = ash::vk::RenderingAttachmentInfo::default();
        if let Some(depth_attachment) = depth_attachment {
            image_transitions.push(VulkanImageTransition {
                image_id: depth_attachment.image,
                new_layout: ash::vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                new_access_flags: ash::vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                    | ash::vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            });

            // Render area should be known by now.
            let image_info = self.ctx.get_image(&depth_attachment.image);
            assert_eq!(
                render_area.extent,
                image_info.info.extent,
                "Depth attachment should have the same extent as color attachments in a raster pipeline."
            );

            depth_attachment_info = ash::vk::RenderingAttachmentInfo::default()
                .image_view(
                    image_info
                        .view
                        .expect("Color attachments should have an image view."),
                )
                .image_layout(ash::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .store_op(ash::vk::AttachmentStoreOp::STORE)
                .load_op(depth_attachment.load_op.into());
            rendering_info = rendering_info.depth_attachment(&depth_attachment_info);
        }

        self.transition_images(
            &image_transitions,
            None,
            ash::vk::PipelineStageFlags::ALL_GRAPHICS,
        );

        unsafe {
            self.ctx
                .device()
                .cmd_begin_rendering(self.command_buffer, &rendering_info)
        };

        let viewports = [ash::vk::Viewport::default()
            .x(0.0)
            .y(0.0)
            .width(render_area.extent.width as f32)
            .height(render_area.extent.height as f32)
            .min_depth(0.0)
            .max_depth(1.0)];
        unsafe {
            self.ctx
                .device()
                .cmd_set_viewport(self.command_buffer, 0, &viewports)
        };

        Box::new(VulkanRenderPass {
            recorder: self,
            pipeline_id: raster_pipeline_id,
            pipeline: raster_pipeline,
            uniforms_bound: false,
        })
    }

    fn get_image_info(&self, image: &ResourceId<Image>) -> GfxImageInfo {
        self.ctx.get_image_info(image)
    }
}

pub struct VulkanComputePass<'a> {
    recorder: &'a mut VulkanRecorder,
    pipeline_id: ResourceId<ComputePipeline>,
    pipeline: VulkanComputePipeline,
    uniforms_bound: bool,
}

impl GraphicsBackendComputePass for VulkanComputePass<'_> {
    fn bind_uniforms(&mut self, writer_fn: &mut dyn FnMut(&mut ShaderWriter)) {
        self.uniforms_bound = true;

        let pipeline_layout = &self
            .recorder
            .ctx
            .get_pipeline_layout(self.pipeline_id.as_untyped());
        let mut writer = ShaderWriter::new(&pipeline_layout.shader_bindings, false);
        writer_fn(&mut writer);
        writer.validate();

        let uniform_data = writer.take_set_data();
        let uniform_bind_info = self
            .recorder
            .ctx
            .bind_uniforms(self.pipeline_id.as_untyped(), uniform_data);

        let mut image_transitions = uniform_bind_info
            .expected_image_layouts
            .into_iter()
            .map(|(image_id, (layout, access))| VulkanImageTransition {
                image_id,
                new_layout: layout,
                new_access_flags: access,
            })
            .collect::<Vec<_>>();
        self.recorder.transition_images(
            &image_transitions,
            None,
            ash::vk::PipelineStageFlags::COMPUTE_SHADER,
        );

        unsafe {
            self.recorder.ctx.device().cmd_bind_descriptor_sets(
                self.recorder.command_buffer,
                ash::vk::PipelineBindPoint::COMPUTE,
                uniform_bind_info.pipeline_layout,
                uniform_bind_info.first_set,
                &uniform_bind_info.descriptor_sets,
                &[],
            )
        };
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        assert!(
            self.uniforms_bound,
            "Tried to dispatch without binding uniforms."
        );
        unsafe {
            self.recorder
                .ctx
                .device()
                .cmd_dispatch(self.recorder.command_buffer, x, y, z)
        };
    }

    fn workgroup_size(&self) -> nalgebra::Vector3<u32> {
        self.pipeline.workgroup_size
    }
}

pub struct VulkanRenderPass<'a> {
    recorder: &'a mut VulkanRecorder,
    pipeline_id: ResourceId<RasterPipeline>,
    pipeline: VulkanRasterPipeline,
    uniforms_bound: bool,
}

impl GraphicsBackendRenderPass for VulkanRenderPass<'_> {
    fn bind_uniforms(&mut self, writer_fn: &mut dyn FnMut(&mut ShaderWriter)) {
        self.uniforms_bound = true;

        let pipeline_layout = &self
            .recorder
            .ctx
            .get_pipeline_layout(self.pipeline_id.as_untyped());
        let mut writer = ShaderWriter::new(&pipeline_layout.shader_bindings, false);
        writer_fn(&mut writer);
        writer.validate();

        let uniform_data = writer.take_set_data();
        let uniform_bind_info = self
            .recorder
            .ctx
            .bind_uniforms(self.pipeline_id.as_untyped(), uniform_data);

        let mut image_transitions = uniform_bind_info
            .expected_image_layouts
            .into_iter()
            .map(|(image_id, (layout, access))| VulkanImageTransition {
                image_id,
                new_layout: layout,
                new_access_flags: access,
            })
            .collect::<Vec<_>>();

        // TODO: We require VK_KHR_dynamic_rendering_local_read to allow for pipeline barriers
        // within a dynamic rendering pass, maybe postpone all commands and execute them later so
        // we can transition images before we begin dynamic rendering.
        self.recorder.transition_images(
            &image_transitions,
            Some(ash::vk::PipelineStageFlags::FRAGMENT_SHADER),
            ash::vk::PipelineStageFlags::FRAGMENT_SHADER,
        );

        unsafe {
            self.recorder.ctx.device().cmd_bind_descriptor_sets(
                self.recorder.command_buffer,
                ash::vk::PipelineBindPoint::GRAPHICS,
                uniform_bind_info.pipeline_layout,
                uniform_bind_info.first_set,
                &uniform_bind_info.descriptor_sets,
                &[],
            )
        };
    }

    fn bind_vertex_buffer(&mut self, vertex_buffer: ResourceId<Buffer>, offset: u64) {
        let buffer = self.recorder.ctx.get_buffer(vertex_buffer);
        unsafe {
            self.recorder.ctx.device().cmd_bind_vertex_buffers(
                self.recorder.command_buffer,
                0,
                &[buffer.buffer],
                &[offset],
            )
        };
    }

    fn bind_index_buffer(&mut self, index_buffer: ResourceId<Buffer>, offset: u64) {
        let buffer = self.recorder.ctx.get_buffer(index_buffer);
        unsafe {
            self.recorder.ctx.device().cmd_bind_index_buffer(
                self.recorder.command_buffer,
                buffer.buffer,
                offset,
                ash::vk::IndexType::UINT32,
            )
        };
    }

    fn set_scissor(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let scissors = [ash::vk::Rect2D::default()
            .offset(ash::vk::Offset2D {
                x: x as i32,
                y: y as i32,
            })
            .extent(ash::vk::Extent2D { width, height })];
        unsafe {
            self.recorder
                .ctx
                .device()
                .cmd_set_scissor(self.recorder.command_buffer, 0, &scissors)
        };
    }

    fn draw_indexed(&mut self, index_count: u32) {
        unsafe {
            self.recorder.ctx.device().cmd_draw_indexed(
                self.recorder.command_buffer,
                index_count,
                1,
                0,
                0,
                0,
            )
        };
    }
}

impl Drop for VulkanRenderPass<'_> {
    fn drop(&mut self) {
        unsafe {
            self.recorder
                .ctx
                .device()
                .cmd_end_rendering(self.recorder.command_buffer);
        }
    }
}
