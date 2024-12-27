use std::collections::HashMap;

use ash::vk::ComputePipelineCreateInfo;

use crate::engine::graphics::{
    backend::{ComputePipeline, ResourceId, Untyped},
    shader::ShaderCompiler,
};

pub struct VulkanPipelineManager {
    pipeline_layouts: HashMap<u32, ash::vk::PipelineLayout>,
    compute_pipelines: HashMap<u32, ash::vk::Pipeline>,
}

impl VulkanPipelineManager {
    pub fn new() -> Self {
        Self {
            pipeline_layouts: HashMap::new(),
            compute_pipelines: HashMap::new(),
        }
    }

    pub fn create_compute_pipeline(
        &mut self,
        resource_id: ResourceId<ComputePipeline>,
        create_info: ComputePipelineCreateInfo,
    ) {
        todo!()
    }

    pub fn is_pipeline_ready(&self, resource_id: ResourceId<Untyped>) -> bool {
        todo!()
    }

    pub fn update_pipeline(&mut self, shader_compiler: &mut ShaderCompiler) -> anyhow::Result<()> {
        Ok(())
    }
}

struct VulkanComputePipeline {}

struct VulkanRasterPipeline {}
