use std::{borrow::BorrowMut, collections::HashMap, time::Duration};

use log::{debug, info};
use rogue_macros::Resource;

use crate::engine::{
    asset::asset::{AssetFile, AssetHandle, AssetId, AssetLoader, AssetPath, Assets},
    resource::{Res, ResMut},
};

use super::{
    device::DeviceResource,
    shader::{RawShader, Shader},
};

#[derive(Resource)]
pub struct RenderPipelineManager {
    pipelines: HashMap<PipelineId, PipelineData>,
    pipeline_layouts: HashMap<PipelineId, wgpu::PipelineLayout>,
    queued_pipelines: Vec<(PipelineId, PipelineCreateInfo)>,
    reset_temporal_effects: bool,
    id_counter: u64,
}

impl RenderPipelineManager {
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
            pipeline_layouts: HashMap::new(),
            queued_pipelines: Vec::new(),
            reset_temporal_effects: false,
            id_counter: 0,
        }
    }

    pub fn update_pipelines(
        mut pipeline_manager: ResMut<RenderPipelineManager>,
        mut assets: ResMut<Assets>,
        device: Res<DeviceResource>,
    ) {
        // Every frame this is the default.
        pipeline_manager.reset_temporal_effects = false;

        // Update pipeline for any updated shaders.
        for pipeline_id in pipeline_manager
            .pipelines
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
        {
            let pipeline_layout = pipeline_manager.pipeline_layouts.get(&pipeline_id).unwrap();
            let mut is_dirty = false;
            let pipeline_data = pipeline_manager.pipelines.get(&pipeline_id).unwrap();
            let shaders = pipeline_manager
                .pipelines
                .get(&pipeline_id)
                .unwrap()
                .shaders
                .clone();

            let pipeline = match &pipeline_data.create_info {
                PipelineCreateInfo::Render {
                    name,
                    vertex_path,
                    vertex_defines,
                    fragment_path,
                    fragment_defines,
                } => todo!(),
                PipelineCreateInfo::Compute {
                    name,
                    shader_path,
                    shader_defines,
                } => 'compute_pipeline_creation: {
                    let shader_id = shaders.first().unwrap();
                    if (assets.is_asset_touched::<AssetFile>(shader_id)) {
                        assets.update_asset::<RawShader, AssetFile>(shader_id);
                        let raw_shader = assets.get_asset(shader_id).unwrap();
                        let shader = match Shader::process_raw(raw_shader, shader_defines.clone()) {
                            Ok(shader) => shader,
                            Err(err) => {
                                log::error!(
                                    "Failed to preprocess pipeline pipeline_{}: {}",
                                    name,
                                    err
                                );
                                break 'compute_pipeline_creation None;
                            }
                        };
                        let shader_module = match shader.create_module(&device) {
                            Ok(shader_module) => shader_module,
                            Err(err) => {
                                log::error!(
                                    "Failed to compile pipeline shaders for pipeline_{}: {}",
                                    name,
                                    err
                                );
                                break 'compute_pipeline_creation None;
                            }
                        };

                        let compute_pipeline =
                            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                                label: Some(&format!("pipeline_{}", name)),
                                layout: Some(pipeline_layout),
                                module: &shader_module,
                                entry_point: "main",
                                compilation_options: wgpu::PipelineCompilationOptions::default(),
                                cache: None,
                            });

                        Some(Pipeline::Compute(compute_pipeline))
                    } else {
                        None
                    }
                }
            };

            if let Some(pipeline) = pipeline {
                debug!(
                    "Updating pipeline pipeline_{}",
                    pipeline_data.create_info.name()
                );
                pipeline_manager
                    .pipelines
                    .get_mut(&pipeline_id)
                    .unwrap()
                    .pipeline = pipeline;

                // Pipeline compilation may have affected any temporal effects so those should be
                // reset.
                pipeline_manager.reset_temporal_effects = true;
            }
        }

        // Process any queued pipelines, enqueus shader asset loading and constructs pipeline
        // layouts.
        for (pipeline_id, create_info) in pipeline_manager
            .queued_pipelines
            .drain(..)
            .collect::<Vec<_>>()
        {
            let pipeline_layout = pipeline_manager
                .pipeline_layouts
                .get(&pipeline_id)
                .expect("Pipeline layout should have been made when the pipeline was requested.");
            let mut shaders = Vec::new();
            let pipeline = match &create_info {
                PipelineCreateInfo::Render {
                    name,
                    vertex_path,
                    vertex_defines,
                    fragment_path,
                    fragment_defines,
                } => todo!(),
                PipelineCreateInfo::Compute {
                    name,
                    shader_path,
                    shader_defines,
                } => {
                    // TODO: Change logic that handles this when asset loading becomes async.
                    let shader_handle =
                        assets.load_asset::<RawShader, AssetFile>(shader_path.clone());
                    shaders.push(shader_handle.clone());
                    let raw_shader = assets.get_asset(&shader_handle).unwrap();
                    let shader = Shader::process_raw(raw_shader, shader_defines.clone())
                        .expect("First time creation of shader couldn't preprocess.");
                    let shader_module = shader
                        .create_module(&device)
                        .expect("First tiem creation of shader could'nt compile.");

                    let compute_pipeline =
                        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                            label: Some(&format!("pipeline_{}", name)),
                            layout: Some(pipeline_layout),
                            module: &shader_module,
                            entry_point: "main",
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                            cache: None,
                        });

                    Pipeline::Compute(compute_pipeline)
                }
            };

            let pipeline_data = PipelineData {
                pipeline,
                create_info,
                shaders,
            };

            pipeline_manager
                .pipelines
                .insert(pipeline_id, pipeline_data);
        }
    }

    pub fn load_pipeline(
        &mut self,
        device: &DeviceResource,
        create_pipeline_info: PipelineCreateInfo,
        bind_group_layouts: &[&wgpu::BindGroupLayout],
    ) -> PipelineId {
        let id = self.next_id();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("pipeline_layout_{}", create_pipeline_info.name())),
            bind_group_layouts,
            push_constant_ranges: &[],
        });
        self.pipeline_layouts.insert(id, pipeline_layout);
        self.queued_pipelines.push((id, create_pipeline_info));

        id
    }

    fn next_id(&mut self) -> PipelineId {
        let id = self.id_counter;
        self.id_counter += 1;

        PipelineId { id }
    }

    pub fn update_pipeline(&mut self, pipeline_id: PipelineId) {}

    pub fn get_render_pipeline(&self, id: PipelineId) -> Option<&wgpu::RenderPipeline> {
        self.pipelines
            .get(&id)
            .map(|pipeline| match &pipeline.pipeline {
                Pipeline::Render(render_pipeline) => render_pipeline,
                Pipeline::Compute(_) => panic!(
                    "Should not call get_render_pipline with an id that is a compute pipeline."
                ),
            })
    }

    pub fn get_compute_pipeline(&self, id: PipelineId) -> Option<&wgpu::ComputePipeline> {
        self.pipelines
            .get(&id)
            .map(|pipeline| match &pipeline.pipeline {
                Pipeline::Compute(compute_pipeline) => compute_pipeline,
                Pipeline::Render(_) => panic!(
                    "Should not call get_compute_pipline with an id that is a render pipeline."
                ),
            })
    }

    pub fn should_reset_temporal_effects(&self) -> bool {
        self.reset_temporal_effects
    }
}

pub struct PipelineData {
    pipeline: Pipeline,
    create_info: PipelineCreateInfo,
    shaders: Vec<AssetHandle>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PipelineId {
    id: u64,
}

impl PipelineId {
    pub const fn null() -> Self {
        Self { id: u64::MAX }
    }

    pub fn is_null(&self) -> bool {
        *self == Self::null()
    }
}

pub struct PipelineHandle {
    id: PipelineId,
}

enum Pipeline {
    Render(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}

pub enum PipelineCreateInfo {
    Render {
        name: String,
        vertex_path: AssetPath,
        vertex_defines: HashMap<String, bool>,
        fragment_path: AssetPath,
        fragment_defines: HashMap<String, bool>,
    },
    Compute {
        name: String,
        shader_path: AssetPath,
        shader_defines: HashMap<String, bool>,
    },
}

impl PipelineCreateInfo {
    pub fn name(&self) -> &str {
        match self {
            PipelineCreateInfo::Render { name, .. } => name,
            PipelineCreateInfo::Compute { name, .. } => name,
        }
    }
}
