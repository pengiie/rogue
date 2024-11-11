use std::{borrow::BorrowMut, collections::HashMap, time::Duration};

use anyhow::anyhow;
use log::{debug, info};
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
    shader::{RawShader, Shader},
};

#[derive(Resource)]
pub struct RenderPipelineManager {
    pipelines: HashMap<PipelineId, PipelineData>,
    pipeline_layouts: HashMap<PipelineId, wgpu::PipelineLayout>,
    queued_pipelines: Vec<QueuedPipeline>,
    processing_pipelines: HashMap<PipelineId, ProcessingPipeline>,
    reset_temporal_effects: bool,
    id_counter: u64,
}

impl RenderPipelineManager {
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
            pipeline_layouts: HashMap::new(),
            queued_pipelines: Vec::new(),
            processing_pipelines: HashMap::new(),
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

        pipeline_manager.check_for_updates(&mut assets);
        pipeline_manager.process_pipelines(&mut assets, &device);

        // Update pipeline for any updated shaders.
        // for pipeline_id in pipeline_manager
        //     .pipelines
        //     .iter()
        //     .map(|(id, _)| *id)
        //     .collect::<Vec<_>>()
        // {
        //     let pipeline_layout = pipeline_manager.pipeline_layouts.get(&pipeline_id).unwrap();
        //     let mut is_dirty = false;
        //     let pipeline_data = pipeline_manager.pipelines.get(&pipeline_id).unwrap();
        //     let shaders = pipeline_manager
        //         .pipelines
        //         .get(&pipeline_id)
        //         .unwrap()
        //         .shaders
        //         .clone();

        //     let pipeline = match &pipeline_data.create_info {
        //         PipelineCreateInfo::Render {
        //             name,
        //             vertex_path,
        //             vertex_defines,
        //             fragment_path,
        //             fragment_defines,
        //         } => todo!(),
        //         PipelineCreateInfo::Compute {
        //             name,
        //             shader_path,
        //             shader_defines,
        //         } => 'compute_pipeline_creation: {
        //             let shader_id = shaders.first().unwrap();
        //             if (assets.is_asset_touched::<AssetFile>(shader_id)) {
        //                 assets.update_asset::<RawShader, AssetFile, AssetFile>(shader_id);
        //                 let raw_shader = assets.get_asset(shader_id).unwrap();
        //                 let shader = match Shader::process_raw(raw_shader, shader_defines.clone()) {
        //                     Ok(shader) => shader,
        //                     Err(err) => {
        //                         log::error!(
        //                             "Failed to preprocess pipeline pipeline_{}: {}",
        //                             name,
        //                             err
        //                         );
        //                         break 'compute_pipeline_creation None;
        //                     }
        //                 };
        //                 let shader_module = match shader.create_module(&device) {
        //                     Ok(shader_module) => shader_module,
        //                     Err(err) => {
        //                         log::error!(
        //                             "Failed to compile pipeline shaders for pipeline_{}: {}",
        //                             name,
        //                             err
        //                         );
        //                         break 'compute_pipeline_creation None;
        //                     }
        //                 };

        //                 let compute_pipeline =
        //                     device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        //                         label: Some(&format!("pipeline_{}", name)),
        //                         layout: Some(pipeline_layout),
        //                         module: &shader_module,
        //                         entry_point: "main",
        //                         compilation_options: wgpu::PipelineCompilationOptions::default(),
        //                         cache: None,
        //                     });

        //                 Some(Pipeline::Compute(compute_pipeline))
        //             } else {
        //                 None
        //             }
        //         }
        //     };

        //     if let Some(pipeline) = pipeline {
        //         debug!(
        //             "Updating pipeline pipeline_{}",
        //             pipeline_data.create_info.name()
        //         );
        //         pipeline_manager
        //             .pipelines
        //             .get_mut(&pipeline_id)
        //             .unwrap()
        //             .pipeline = pipeline;

        //         // Pipeline compilation may have affected any temporal effects so those should be
        //         // reset.
        //         pipeline_manager.reset_temporal_effects = true;
        //     }
        // }
    }

    fn check_for_updates(&mut self, assets: &mut Assets) {
        for (pipeline_id, data) in &self.pipelines {
            let mut should_reload = false;
            for shader in &data.shaders {
                if assets.is_asset_touched::<AssetFile, AssetFile>(shader) {
                    should_reload = true;
                    break;
                }
            }
            if should_reload {
                assert!(
                    self.pipeline_layouts.contains_key(&pipeline_id),
                    "Pipeline id is invalid"
                );

                // TODO: Replace this with some hash on the pipeline id so we dont iter the
                // whole list to see if it's already queued.
                let is_queued = self.queued_pipelines.iter().any(|x| pipeline_id == &x.id);
                let is_processing = self
                    .processing_pipelines
                    .iter()
                    .any(|(x, _)| pipeline_id == x);
                if !is_queued && !is_processing {
                    let qp = QueuedPipeline {
                        id: *pipeline_id,
                        create_info: data.create_info.clone(),
                        existing_shader_handles: Some(data.shaders.clone()),
                    };
                    self.queued_pipelines.push(qp);

                    // Enqueue asset updates as well.
                    for shader in &data.shaders {
                        assets.update_asset::<RawShader, AssetFile, AssetFile>(shader);
                    }
                }
            }
        }
    }

    fn process_pipelines(&mut self, assets: &mut Assets, device: &DeviceResource) {
        // Process any queued pipelines, enqueues shader asset loading.
        for QueuedPipeline {
            id: pipeline_id,
            create_info,
            existing_shader_handles,
        } in self.queued_pipelines.drain(..).collect::<Vec<_>>()
        {
            let pipeline_layout = self
                .pipeline_layouts
                .get(&pipeline_id)
                .expect("Pipeline layout should have been made when the pipeline was requested.");
            let shaders = existing_shader_handles
                .or_else(|| {
                    Some(match &create_info {
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
                            let profile = Instant::now();
                            let shader_handle = assets
                                .load_asset::<RawShader, AssetFile, AssetFile>(shader_path.clone());
                            debug!(
                                "Shader loaded in {}ms",
                                profile.elapsed().as_secs_f32() * 1000.0
                            );

                            vec![shader_handle]
                        }
                    })
                })
                .unwrap();

            self.processing_pipelines.insert(
                pipeline_id,
                ProcessingPipeline {
                    create_info,
                    shader_handles: shaders,
                },
            );
        }

        // Check all currently processing pipelines and see if the shader assets are ready to build
        // the pipeline.
        let mut to_remove_ids = Vec::new();
        for (id, processing_pipeline) in &self.processing_pipelines {
            if processing_pipeline.is_ready(assets) {
                let is_preexisting = self.pipelines.contains_key(id);
                to_remove_ids.push(*id);

                let pipeline = match &processing_pipeline.create_info {
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
                        let shader_handle = &processing_pipeline.shader_handles[0];
                        let raw_shader = assets.get_asset(shader_handle).unwrap();
                        let shader_module =
                            match Shader::process_raw(raw_shader, shader_defines.clone()) {
                                Ok(shader) => shader.create_module(&device)
                                    as anyhow::Result<wgpu::ShaderModule>,
                                Err(err) => Err(anyhow!(err)),
                            };
                        let Ok(shader_module) = shader_module else {
                            if is_preexisting {
                                // Keep using the known good pipeline.
                                break;
                            }

                            panic!("Failed to create first-time shader for pipeline {}", name);
                        };

                        let pipeline_layout = self.pipeline_layouts.get(id).unwrap();
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

                self.pipelines.insert(
                    *id,
                    PipelineData {
                        pipeline,
                        create_info: processing_pipeline.create_info.clone(),
                        shaders: processing_pipeline.shader_handles.clone(),
                    },
                );
                self.reset_temporal_effects = true;
            }
        }
        for id in to_remove_ids {
            self.processing_pipelines.remove(&id);
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
        self.queued_pipelines.push(QueuedPipeline {
            id,
            create_info: create_pipeline_info,
            existing_shader_handles: None,
        });

        id
    }

    fn next_id(&mut self) -> PipelineId {
        let id = self.id_counter;
        self.id_counter += 1;

        PipelineId(id)
    }

    /// Enqueues a pipeline with the create info supplied.
    pub fn update_pipeline(
        &mut self,
        pipeline_id: PipelineId,
        create_pipeline_info: PipelineCreateInfo,
    ) {
    }

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

create_id_type!(PipelineId);

enum Pipeline {
    Render(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}

#[derive(Clone)]
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

pub struct QueuedPipeline {
    id: PipelineId,
    create_info: PipelineCreateInfo,
    existing_shader_handles: Option<Vec<AssetHandle>>,
}

pub struct ProcessingPipeline {
    create_info: PipelineCreateInfo,
    shader_handles: Vec<AssetHandle>,
}

impl ProcessingPipeline {
    /// Returns true if all the shader assets have been loaded.
    fn is_ready(&self, assets: &Assets) -> bool {
        for handle in &self.shader_handles {
            if assets.is_asset_loading(handle) {
                return false;
            }
        }

        return true;
    }
}
