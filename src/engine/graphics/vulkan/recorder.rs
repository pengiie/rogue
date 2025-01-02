use std::{collections::HashMap, sync::Arc};

use epaint::image;

use crate::{
    common::color::{Color, ColorSpaceSrgb},
    engine::graphics::backend::{
        Binding, ComputePass, ComputePipeline, GfxFilterMode, GraphicsBackendComputePass,
        GraphicsBackendRecorder, Image, ResourceId, UniformData,
    },
};

use super::device::{VulkanComputePipeline, VulkanContext};

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
        dst_stage: ash::vk::PipelineStageFlags,
    ) {
        const UNDEFINED_LAYOUT_ACCESS: (ash::vk::ImageLayout, ash::vk::AccessFlags) = (
            ash::vk::ImageLayout::UNDEFINED,
            ash::vk::AccessFlags::empty(),
        );

        let mut src_stage = ash::vk::PipelineStageFlags::empty();

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
                src_stage = src_stage.max(match *old_layout {
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

        unsafe {
            self.ctx.device().cmd_pipeline_barrier(
                self.command_buffer,
                src_stage,
                dst_stage,
                ash::vk::DependencyFlags::empty(),
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
}

pub struct VulkanComputePass<'a> {
    recorder: &'a mut VulkanRecorder,
    pipeline_id: ResourceId<ComputePipeline>,
    pipeline: VulkanComputePipeline,
    uniforms_bound: bool,
}

impl GraphicsBackendComputePass for VulkanComputePass<'_> {
    fn bind_uniforms(&mut self, uniform_data: UniformData) {
        self.uniforms_bound = true;

        let mut image_transitions = Vec::new();
        for binding in uniform_data.bindings() {
            match binding {
                Binding::Image { image } => image_transitions.push(VulkanImageTransition {
                    image_id: *image,
                    new_layout: ash::vk::ImageLayout::GENERAL,
                    new_access_flags: ash::vk::AccessFlags::SHADER_READ,
                }),
                Binding::Sampler {} => todo!(),
                Binding::Buffer {} => todo!(),
            }
        }
        self.recorder.transition_images(
            &image_transitions,
            ash::vk::PipelineStageFlags::COMPUTE_SHADER,
        );

        self.recorder.ctx.bind_uniforms(
            self.recorder.command_buffer,
            self.pipeline_id.as_untyped(),
            ash::vk::PipelineBindPoint::COMPUTE,
            uniform_data,
        );
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        assert!(self.uniforms_bound);
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
