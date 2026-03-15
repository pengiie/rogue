use std::collections::{HashMap, HashSet};

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    consts,
    event::{EventReader, Events},
    graphics::{
        backend::GraphicsBackendRecorder,
        frame_graph::{
            FrameGraphBuilder, FrameGraphComputeInfo, FrameGraphContext, FrameGraphResource, Pass,
        },
        renderer::Renderer,
    },
    resource::{Res, ResMut},
    voxel::{voxel_registry::VoxelModelId, voxel_registry_gpu::VoxelModelRegistryGpu},
    world::region_map::{ChunkEvent, ChunkEventType, ChunkId, RegionMap},
};

struct VoxelBakerGraphConstants {
    bake_pass_name: &'static str,
    bake_chunk_compute_pipeline_name: &'static str,
    bake_chunk_compute_pipeline_info: FrameGraphComputeInfo<'static>,
    bake_model_compute_pipeline_name: &'static str,
    bake_model_compute_pipeline_info: FrameGraphComputeInfo<'static>,
}

#[derive(Resource)]
pub struct VoxelBakerGpu {
    bake_pass: Option<FrameGraphResource<Pass>>,

    chunk_bake_requests: HashMap<ChunkId, ModelBakeRequest>,
    model_bake_requests: HashMap<VoxelModelId, ModelBakeRequest>,

    chunk_event_reader: EventReader<ChunkEvent>,
}

pub struct ModelBakeRequest {
    pub offset: Vector3<u32>,
    pub size: Vector3<u32>,
}

impl VoxelBakerGpu {
    const GRAPH: VoxelBakerGraphConstants = VoxelBakerGraphConstants {
        bake_pass_name: "world_bake_pass",
        bake_chunk_compute_pipeline_name: "chunk_bake_compute_pipeline",
        bake_chunk_compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "bake_chunk",
            entry_point_fn: "main",
        },
        bake_model_compute_pipeline_name: "model_bake_compute_pipeline",
        bake_model_compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "bake_model",
            entry_point_fn: "main",
        },
    };

    pub fn new() -> Self {
        Self {
            bake_pass: None,
            model_bake_requests: HashMap::new(),
            chunk_bake_requests: HashMap::new(),
            chunk_event_reader: EventReader::new(),
        }
    }

    pub fn create_chunk_bake_request(&mut self, chunk_id: ChunkId, bake_request: ModelBakeRequest) {
        let old = self.chunk_bake_requests.insert(chunk_id, bake_request);
    }

    pub fn create_model_bake_request(
        &mut self,
        model_id: VoxelModelId,
        bake_request: ModelBakeRequest,
    ) {
        let old = self.model_bake_requests.insert(model_id, bake_request);
        assert!(
            old.is_none(),
            "Overwrote previous bake request for model {:?}",
            model_id
        );
    }

    pub fn set_graph_bake_pass(&mut self, fg: &mut FrameGraphBuilder) -> FrameGraphResource<Pass> {
        let chunk_compute_pipeline = fg.create_compute_pipeline(
            Self::GRAPH.bake_chunk_compute_pipeline_name,
            Self::GRAPH.bake_chunk_compute_pipeline_info,
        );
        let model_compute_pipeline = fg.create_compute_pipeline(
            Self::GRAPH.bake_model_compute_pipeline_name,
            Self::GRAPH.bake_model_compute_pipeline_info,
        );

        let pass = fg.create_input_pass(
            Self::GRAPH.bake_pass_name,
            &[&chunk_compute_pipeline, &model_compute_pipeline],
            &[],
        );
        self.bake_pass = Some(pass);

        pass
    }

    pub fn write_graph_passes(
        mut baker: ResMut<VoxelBakerGpu>,
        mut renderer: ResMut<Renderer>,
        voxel_registry_gpu: Res<VoxelModelRegistryGpu>,
    ) {
        let baker = &mut *baker;
        renderer.frame_graph_executor.supply_pass_ref(
            Self::GRAPH.bake_pass_name,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                // Bake chunks first.
                {
                    let compute_pipeline =
                        ctx.get_compute_pipeline(Self::GRAPH.bake_chunk_compute_pipeline_name);
                    let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);

                    for (chunk_id, bake_request) in baker.chunk_bake_requests.drain() {
                        let bake_volume =
                            bake_request.size.x * bake_request.size.y * bake_request.size.z;
                        compute_pass.bind_uniforms(&mut |writer| {
                            writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                            writer.write_uniform::<Vector3<i32>>(
                                "u_shader.chunk_pos",
                                *chunk_id.chunk_pos,
                            );
                            writer.write_uniform::<u32>(
                                "u_shader.chunk_height",
                                chunk_id.chunk_lod.as_tree_height(),
                            );
                            writer.write_uniform::<Vector3<u32>>(
                                "u_shader.voxel_offset",
                                bake_request.offset,
                            );
                            writer.write_uniform::<Vector3<u32>>(
                                "u_shader.voxel_size",
                                bake_request.size,
                            );
                            writer.write_uniform("u_shader.bake_volume", bake_volume);
                        });

                        let wg_size = compute_pass.workgroup_size();
                        compute_pass.dispatch(bake_volume.div_ceil(wg_size.x), 1, 1);
                    }
                }

                // Bake entity models.
                {
                    let compute_pipeline =
                        ctx.get_compute_pipeline(Self::GRAPH.bake_model_compute_pipeline_name);
                    let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);

                    for (model_id, bake_request) in baker.model_bake_requests.drain() {
                        let bake_volume =
                            bake_request.size.x * bake_request.size.y * bake_request.size.z;
                        let model_ptr_gpu = voxel_registry_gpu
                            .get_model_gpu_ptr(&model_id)
                            .expect("Voxel model gpu ptr should exist if baking.");
                        log::info!(
                            "Baking model {:?} ptr {} with volume {} ({}x{}x{})",
                            model_id,
                            model_ptr_gpu,
                            bake_volume,
                            bake_request.size.x,
                            bake_request.size.y,
                            bake_request.size.z
                        );
                        compute_pass.bind_uniforms(&mut |writer| {
                            writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                            writer.write_uniform::<u32>("u_shader.model_ptr", model_ptr_gpu);
                            writer.write_uniform::<Vector3<u32>>(
                                "u_shader.voxel_offset",
                                bake_request.offset,
                            );
                            writer.write_uniform::<Vector3<u32>>(
                                "u_shader.voxel_size",
                                bake_request.size,
                            );
                            writer.write_uniform("u_shader.bake_volume", bake_volume);
                        });

                        let wg_size = compute_pass.workgroup_size();
                        compute_pass.dispatch(bake_volume.div_ceil(wg_size.x), 1, 1);
                    }
                }
            },
        );
    }
}
