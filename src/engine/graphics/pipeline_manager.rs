use std::{
    borrow::BorrowMut,
    collections::{HashMap, VecDeque},
    time::Duration,
};

use anyhow::anyhow;
use log::{debug, error, info};
use rogue_macros::Resource;

use crate::{
    common::id::create_id_type,
    engine::{
        asset::asset::{AssetFile, AssetHandle, AssetId, AssetLoader, AssetPath, Assets},
        resource::{Res, ResMut},
        window::time::Instant,
    },
};

use super::{
    device::DeviceResource,
    shader::{Shader, ShaderCompilationOptions, ShaderCompiler, ShaderStage},
};

#[derive(Resource)]
pub struct RenderPipelineManager {
    queued_compute_pipelines: VecDeque<QueuedComputePipeline>,
    compute_pipelines: HashMap<PipelineId, ComputePipelineData>,
    pipeline_layouts: HashMap<PipelineId, PipelineLayout>,
    reset_temporal_effects: bool,
    id_counter: u64,
}

impl RenderPipelineManager {
    pub fn new() -> Self {
        Self {
            queued_compute_pipelines: VecDeque::new(),
            compute_pipelines: HashMap::new(),
            pipeline_layouts: HashMap::new(),
            reset_temporal_effects: false,
            id_counter: 0,
        }
    }

    pub fn update_pipelines(
        mut pipeline_manager: ResMut<RenderPipelineManager>,
        shader_compiler: Res<ShaderCompiler>,
        device: Res<DeviceResource>,
        assets: Res<Assets>,
    ) {
        // Recompile pipelines.
        pipeline_manager.reset_temporal_effects = false;
        if assets.is_assets_dir_modified() {
            pipeline_manager.reset_temporal_effects = true;
            debug!(
                "Asset directory modified, recompiling {} pipelines",
                pipeline_manager.compute_pipelines.len()
            );
            for id in pipeline_manager
                .compute_pipelines
                .keys()
                .map(|id| *id)
                .collect::<Vec<_>>()
            {
                let ComputePipelineData {
                    pipeline,
                    create_info: ci,
                } = pipeline_manager.compute_pipelines.get(&id).unwrap();

                let compute_pipeline = match pipeline_manager.create_compute_pipeline(
                    &device,
                    &shader_compiler,
                    id,
                    ci,
                ) {
                    Ok(pipeline) => pipeline,
                    Err(err) => {
                        error!(
                            "Failed to update pipeline {}, info: {:?}",
                            ci.pipeline_name, err
                        );
                        continue;
                    }
                };

                pipeline_manager
                    .compute_pipelines
                    .get_mut(&id)
                    .unwrap()
                    .pipeline = compute_pipeline;
            }
        }

        if let Some(QueuedComputePipeline {
            pipeline_id: id,
            compute_pipeline_create_info: ci,
        }) = pipeline_manager.queued_compute_pipelines.pop_front()
        {
            match pipeline_manager.create_compute_pipeline(&device, &shader_compiler, id, &ci) {
                Ok(compute_pipeline) => {
                    pipeline_manager.compute_pipelines.insert(
                        id,
                        ComputePipelineData {
                            pipeline: compute_pipeline,
                            create_info: ci,
                        },
                    );
                }
                Err(err) => {
                    panic!(
                        "Failed to compile pipeline {}, info: {:?}",
                        ci.pipeline_name, err
                    );
                }
            };
        }
    }

    fn create_compute_pipeline(
        &self,
        device: &DeviceResource,
        shader_compiler: &ShaderCompiler,
        pipeline_id: PipelineId,
        create_pipeline_info: &ComputePipelineCreateInfo,
    ) -> anyhow::Result<ComputePipeline> {
        let ci = create_pipeline_info;
        let shader = shader_compiler.compile_shader(ShaderCompilationOptions {
            module: ci.shader_module.clone(),
            entry_point: ci.shader_entry_point.clone(),
            stage: ShaderStage::Compute,
        })?;

        // let shader_module = shader
        //     .create_wgpu_module(&device)
        //     .expect("Failed to create shader module");

        let layout = self.pipeline_layouts.get(&pipeline_id).unwrap();
        Ok(
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(&format!("compute_pipeline_{}", ci.pipeline_name)),
                layout: Some(layout),
                module: &shader_module,
                entry_point: &ci.shader_entry_point,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            }),
        )
    }

    pub fn load_compute_pipeline(
        &mut self,
        device: &DeviceResource,
        create_pipeline_info: ComputePipelineCreateInfo,
        bind_group_layouts: &[&wgpu::BindGroupLayout],
    ) -> PipelineId {
        let id = self.next_id();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!(
                "compute_pipeline_layout_{}",
                create_pipeline_info.pipeline_name
            )),
            bind_group_layouts,
            push_constant_ranges: &[],
        });

        self.pipeline_layouts.insert(id, pipeline_layout);
        self.queued_compute_pipelines
            .push_back(QueuedComputePipeline {
                pipeline_id: id,
                compute_pipeline_create_info: create_pipeline_info,
            });

        id
    }

    fn next_id(&mut self) -> PipelineId {
        let id = self.id_counter;
        self.id_counter += 1;

        PipelineId(id)
    }

    pub fn get_compute_pipeline(&self, pipeline_id: PipelineId) -> Option<&wgpu::ComputePipeline> {
        self.compute_pipelines
            .get(&pipeline_id)
            .map(|pipeline_data| &pipeline_data.pipeline)
    }

    pub fn should_reset_temporal_effects(&self) -> bool {
        self.reset_temporal_effects
    }
}

pub struct ComputePipelineData {
    pipeline: wgpu::ComputePipeline,
    create_info: ComputePipelineCreateInfo,
}

create_id_type!(PipelineId);

pub struct ComputePipelineCreateInfo {
    pub pipeline_name: String,
    pub shader_module: String,
    pub shader_entry_point: String,
    pub defines: HashMap<String, ShaderDefine>,
}

pub enum ShaderDefine {
    Bool(bool),
    Int(i32),
    UInt(u32),
    Slang(&'static str),
}

pub struct QueuedComputePipeline {
    pipeline_id: PipelineId,
    compute_pipeline_create_info: ComputePipelineCreateInfo,
}
