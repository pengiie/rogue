use std::{collections::HashMap, sync::Arc};

use crate::{
    common::color::{Color, ColorSpaceSrgb},
    engine::graphics::backend::{GraphicsBackendRecorder, Image, ResourceId},
};

use super::device::VulkanContext;

pub struct VulkanRecorder {
    ctx: Arc<VulkanContext>,
    command_buffer: ash::vk::CommandBuffer,
    image_layouts: HashMap<ResourceId<Image>, (ash::vk::ImageLayout, ash::vk::AccessFlags)>,
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

    pub fn transition_image(
        &mut self,
        image_id: ResourceId<Image>,
        expected_layout: ash::vk::ImageLayout,
        expected_access_flags: ash::vk::AccessFlags,
        dst_stage: ash::vk::PipelineStageFlags,
    ) {
        const UNDEFINED_LAYOUT_ACCESS: (ash::vk::ImageLayout, ash::vk::AccessFlags) = (
            ash::vk::ImageLayout::UNDEFINED,
            ash::vk::AccessFlags::empty(),
        );
        let (image_layout, access_flags) = self
            .image_layouts
            .get(&image_id)
            .unwrap_or(&UNDEFINED_LAYOUT_ACCESS);
        if *image_layout != expected_layout {
            let src_stage = match *image_layout {
                ash::vk::ImageLayout::UNDEFINED => ash::vk::PipelineStageFlags::TOP_OF_PIPE,
                _ => todo!(),
            };
            let image = self.ctx.resource_manager().get_image(image_id);
            let image_memory_barrier = ash::vk::ImageMemoryBarrier::default()
                .image(image.image)
                .subresource_range(image.full_subresource_range())
                .old_layout(*image_layout)
                .new_layout(expected_layout)
                .src_access_mask(*access_flags)
                .dst_access_mask(expected_access_flags);
            unsafe {
                self.ctx.device().cmd_pipeline_barrier(
                    self.command_buffer,
                    src_stage,
                    dst_stage,
                    ash::vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[image_memory_barrier],
                )
            };
        }
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
        self.transition_image(
            image_id,
            ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            ash::vk::AccessFlags::TRANSFER_WRITE,
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

    fn blit(&mut self, src: ResourceId<Image>, dst: ResourceId<Image>) {
        todo!()
    }

    fn begin_compute_pass(
        &mut self,
        compute_pipeline: ResourceId<crate::engine::graphics::backend::ComputePipeline>,
    ) -> crate::engine::graphics::backend::ComputePass {
        todo!()
    }
}
