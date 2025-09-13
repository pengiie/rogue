use std::{
    any::{Any, TypeId},
    borrow::Borrow,
    collections::{HashMap, HashSet, VecDeque},
    ops::{Deref, DerefMut},
    sync::mpsc::{Receiver, Sender},
    u32, u64,
};

use hecs::Entity;
use log::debug;
use nalgebra::{allocator, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{
        archetype::{Archetype, ArchetypeIter, ArchetypeIterMut},
        bitset::Bitset,
        dyn_vec::TypeInfo,
        morton::{self, morton_encode, morton_traversal_octree},
    },
    consts::{self, voxel::VOXEL_METER_LENGTH},
    engine::{
        asset::asset::{AssetHandle, Assets},
        entity::{ecs_world::ECSWorld, RenderableVoxelEntity},
        event::Events,
        graphics::{
            backend::{
                Buffer, GfxBufferCreateInfo, GraphicsBackendDevice, GraphicsBackendRecorder,
                ResourceId,
            },
            device::{DeviceResource, GfxDevice},
            frame_graph::FrameGraphContext,
            gpu_allocator::{Allocation, GpuBufferAllocator},
            renderer::Renderer,
        },
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{
            terrain::{
                chunks::{VoxelChunks, VoxelRegionLeafNode},
                RenderableChunksGpu,
            },
            voxel::VoxelModelEdit,
            voxel_world::{self, VoxelWorld},
        },
        window::time::Stopwatch,
    },
    session::Session,
    settings::Settings,
};
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::ray::Ray;
use super::{
    attachment::{AttachmentId, AttachmentInfoMap, AttachmentMap},
    cursor::{VoxelEditEntityInfo, VoxelEditInfo},
    flat::VoxelModelFlat,
    sft::VoxelModelSFT,
    voxel::{
        VoxelMaterialSet, VoxelModel, VoxelModelGpu, VoxelModelGpuImpl, VoxelModelGpuImplConcrete,
        VoxelModelImpl, VoxelModelImplConcrete, VoxelModelSchema,
    },
    voxel_allocator::VoxelDataAllocator,
    voxel_registry::{VoxelModelId, VoxelModelRegistry},
    voxel_transform::VoxelModelTransform,
};

#[derive(Resource)]
pub struct VoxelWorldGpu {
    pub renderable_chunks: RenderableChunksGpu,

    /// The acceleration buffer for rendered entity voxel model bounds interaction, hold the
    /// pointed to voxel model index and the position and rotation matrix data of this entity.
    entity_acceleration_buffer: Option<ResourceId<Buffer>>,

    /// The buffer for every unique voxel models info such as its data pointers and length.
    voxel_model_info_allocator: Option<GpuBufferAllocator>,
    voxel_model_info_map: HashMap<VoxelModelId, VoxelWorldModelGpuInfo>,
    // Rendered voxel models entities, count of entities pointing to models in the acceleration buffer.
    rendered_voxel_model_entity_count: u32,

    /// The allocator that owns and manages the world data buffer holding all the voxel model
    /// information.
    voxel_data_allocator: VoxelDataAllocator,

    to_register_models: Vec<VoxelModelId>,
    initialized_normals: HashSet<VoxelModelId>,
}

pub struct VoxelWorldModelGpuInfo {
    pub info_allocation: Allocation,
    // The dimensions of this voxel model.
    pub voxel_model_dimensions: Vector3<u32>,
}

struct VoxelWorldGpuFrameState {
    /// Some gpu object was changed (handle-wise), signals bind group recreation.
    did_buffers_update: bool,

    /// A list of voxel models that allocated a new buffer, so their info entry must be
    /// created/updated.
    updated_voxel_model_allocations: Vec<VoxelModelId>,

    /// Aggregates the copies to be made to the model info buffer.
    voxel_model_info_copies: Vec<VoxelModelInfoCopy>,
}

impl VoxelWorldGpuFrameState {
    pub fn new() -> Self {
        Self {
            did_buffers_update: false,
            updated_voxel_model_allocations: Vec::new(),
            voxel_model_info_copies: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.did_buffers_update = false;
        self.updated_voxel_model_allocations.clear();
        self.voxel_model_info_copies.clear();
    }
}

impl VoxelWorldGpu {
    pub fn new() -> Self {
        Self {
            renderable_chunks: RenderableChunksGpu::new(),

            entity_acceleration_buffer: None,

            voxel_model_info_allocator: None,
            voxel_model_info_map: HashMap::new(),

            rendered_voxel_model_entity_count: 0,

            voxel_data_allocator: VoxelDataAllocator::new(),
            to_register_models: Vec::new(),
            initialized_normals: HashSet::new(),
            //        frame_state: VoxelWorldGpuFrameState::new(),
        }
    }

    pub fn voxel_allocator(&self) -> &VoxelDataAllocator {
        &self.voxel_data_allocator
    }

    pub fn voxel_allocator_mut(&mut self) -> &mut VoxelDataAllocator {
        &mut self.voxel_data_allocator
    }

    pub fn entity_acceleration_struct_size() -> usize {
        /*aabb_min*/
        (4 * 4) + // float3
        /*aabb_max*/
        (4 * 4) + // float3
        /*rotation*/ (4 * 11) +// matrix3x3
        /*model_info_ptr*/ 4 // uint
    }

    pub fn update_gpu_objects(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        mut device: ResMut<DeviceResource>,
        ecs_world: Res<ECSWorld>,
    ) {
        let voxel_world_gpu = &mut voxel_world_gpu;

        // Create or resize entity acceleration buffer.
        const DEFAULT_INITIAL_COUNT: usize = 10;
        let entity_count = Self::query_voxel_entities(&ecs_world).iter().count();
        let req_entity_data_size = (entity_count.max(DEFAULT_INITIAL_COUNT)
            * Self::entity_acceleration_struct_size()) as u64;
        if let Some(entity_acceleration_buffer) = &mut voxel_world_gpu.entity_acceleration_buffer {
            let buffer_info = device.get_buffer_info(entity_acceleration_buffer);
            if buffer_info.size < req_entity_data_size {
                // TODO: Remove old buffer.
                let new_buffer = device.create_buffer(GfxBufferCreateInfo {
                    name: "world_entity_acceleration_buffer".to_owned(),
                    size: req_entity_data_size,
                });
                *entity_acceleration_buffer = new_buffer;
            }
        } else {
            voxel_world_gpu.entity_acceleration_buffer =
                Some(device.create_buffer(GfxBufferCreateInfo {
                    name: "world_entity_acceleration_buffer".to_owned(),
                    size: req_entity_data_size,
                }));
        }

        if voxel_world_gpu.voxel_model_info_allocator.is_none() {
            voxel_world_gpu.voxel_model_info_allocator = Some(GpuBufferAllocator::new(
                &mut device,
                "voxel_model_info_allocator",
                1 << 20,
            ));
        }

        // if voxel_world_gpu.voxel_data_allocator.is_none() {
        //     // 2 gig
        //     voxel_world_gpu.voxel_data_allocator = Some(GpuBufferAllocator::new(
        //         &mut device,
        //         "voxel_data_allocator",
        //         1 << 31,
        //     ));
        // }

        // Update terrain acceleration buffer.
        voxel_world_gpu
            .renderable_chunks
            .update_gpu_objects(&mut device, &voxel_world.chunks.renderable_chunks);

        // Update each renderable model, though just because it's in the registry doesn't mean it's
        // being used.
        //
        // TODO: Get entities with transform, then see which models are in view of the camera
        // frustum or near it, then update the voxel model those entities point to. This way we can
        // save on transfer bandwidth by only updating what we need. This will also be why
        // splitting up the terrain into discrete voxel models per chunk is important.
        for (voxel_model_id, (model, model_gpu)) in
            voxel_world.registry.renderable_models_dyn_iter_mut()
        {
            if model_gpu.update_gpu_objects(
                &mut device,
                &mut voxel_world_gpu.voxel_data_allocator,
                model,
            ) {
                voxel_world_gpu.to_register_models.push(voxel_model_id);
                log::debug!("Registering model {:?}", voxel_model_id);
            }
        }
    }

    pub fn write_render_data(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        mut device: ResMut<DeviceResource>,
    ) {
        let voxel_world_gpu: &mut VoxelWorldGpu = &mut voxel_world_gpu;

        // Update gpu model buffer data, do this first so the allocation data is ready to reference
        // when registering updated_voxel_model_allocations.
        for (model_id, (mut voxel_model, mut voxel_model_gpu)) in
            voxel_world.registry.renderable_models_dyn_iter_mut()
        {
            voxel_model_gpu.deref_mut().write_gpu_updates(
                &mut device,
                &mut voxel_world_gpu.voxel_data_allocator,
                voxel_model.deref_mut() as &mut dyn VoxelModelImpl,
            );
        }

        let mut registered_model_infos = Vec::new();
        for voxel_model_id in voxel_world_gpu
            .to_register_models
            .drain(..)
            .collect::<Vec<_>>()
        {
            voxel_world_gpu.register_voxel_model_info(
                &voxel_world,
                voxel_model_id,
                &mut registered_model_infos,
            );
        }
        let did_register_a_model = !registered_model_infos.is_empty();
        for model_info_copy in registered_model_infos {
            device.write_buffer_slice(
                voxel_world_gpu.world_voxel_model_info_buffer(),
                model_info_copy.dst_index as u64 * 4,
                bytemuck::cast_slice(model_info_copy.src_data.as_slice()),
            );
        }

        // Write terrain acceleration data.
        voxel_world_gpu.renderable_chunks.write_render_data(
            &mut device,
            &voxel_world.chunks.renderable_chunks,
            &voxel_world_gpu.voxel_model_info_map,
            did_register_a_model,
        );

        // Write entity acceleration data.
        let mut voxel_entity_data = Vec::new();
        let mut voxel_entities_query = Self::query_voxel_entities(&ecs_world);
        voxel_world_gpu.rendered_voxel_model_entity_count = 0;
        for (entity, (local_transform, voxel_entity)) in voxel_entities_query.iter() {
            let Some(voxel_model_id) = voxel_entity.voxel_model_id() else {
                continue;
            };
            voxel_world_gpu.rendered_voxel_model_entity_count += 1;

            let Some(model_gpu_info) = voxel_world_gpu.voxel_model_info_map.get(&voxel_model_id)
            else {
                panic!("Model should be loaded by now");
            };

            let world_transform = ecs_world.get_world_transform(entity, local_transform);
            let half_side_length = model_gpu_info
                .voxel_model_dimensions
                .zip_map(&world_transform.scale, |x, y| x as f32 * y)
                * consts::voxel::VOXEL_METER_LENGTH
                * 0.5;
            let min = world_transform.position - half_side_length;
            let max = world_transform.position + half_side_length;
            let r = world_transform.rotation.to_rotation_matrix();
            // Transpose cause its inverse and nalgebra is clockwise? i dunno for sure.
            let r = r.matrix().transpose();
            let model_ptr = model_gpu_info.info_allocation.start_index_stride_dword() as u32;
            voxel_entity_data.extend_from_slice(&min.x.to_le_bytes());
            voxel_entity_data.extend_from_slice(&min.y.to_le_bytes());
            voxel_entity_data.extend_from_slice(&min.z.to_le_bytes());
            voxel_entity_data.extend_from_slice(&[0u8; 4]);

            voxel_entity_data.extend_from_slice(&max.x.to_le_bytes());
            voxel_entity_data.extend_from_slice(&max.y.to_le_bytes());
            voxel_entity_data.extend_from_slice(&max.z.to_le_bytes());
            voxel_entity_data.extend_from_slice(&[0u8; 4]);

            voxel_entity_data.extend_from_slice(&r.m11.to_le_bytes());
            voxel_entity_data.extend_from_slice(&r.m12.to_le_bytes());
            voxel_entity_data.extend_from_slice(&r.m13.to_le_bytes());
            voxel_entity_data.extend_from_slice(&[0u8; 4]);
            voxel_entity_data.extend_from_slice(&r.m21.to_le_bytes());
            voxel_entity_data.extend_from_slice(&r.m22.to_le_bytes());
            voxel_entity_data.extend_from_slice(&r.m23.to_le_bytes());
            voxel_entity_data.extend_from_slice(&[0u8; 4]);
            voxel_entity_data.extend_from_slice(&r.m31.to_le_bytes());
            voxel_entity_data.extend_from_slice(&r.m32.to_le_bytes());
            voxel_entity_data.extend_from_slice(&r.m33.to_le_bytes());
            voxel_entity_data.extend_from_slice(&[0u8; 4]);
            voxel_entity_data.extend_from_slice(&model_ptr.to_le_bytes());
            voxel_entity_data.extend_from_slice(&[0u8; 12]);
        }
        if !voxel_entity_data.is_empty() {
            device.write_buffer_slice(
                voxel_world_gpu.world_entity_acceleration_buffer(),
                0,
                bytemuck::cast_slice(voxel_entity_data.as_slice()),
            );
        }
    }

    fn query_voxel_entities<'a>(
        ecs_world: &'a ECSWorld,
    ) -> hecs::QueryBorrow<'a, (&Transform, &RenderableVoxelEntity)> {
        ecs_world.query()
    }

    fn voxel_model_info_allocator_mut(&mut self) -> &mut GpuBufferAllocator {
        self.voxel_model_info_allocator.as_mut().unwrap()
    }

    fn register_voxel_model_info(
        &mut self,
        voxel_world: &VoxelWorld,
        voxel_model_id: VoxelModelId,
        copies: &mut Vec<VoxelModelInfoCopy>,
    ) {
        let (voxel_model, voxel_model_gpu) = voxel_world
            .registry
            .get_dyn_renderable_model(voxel_model_id);
        let Some(mut model_gpu_info_ptrs) = voxel_model_gpu.aggregate_model_info() else {
            log::info!("Pointers are not ready.");
            return;
        };
        assert!(!model_gpu_info_ptrs.is_empty());

        let info_size = model_gpu_info_ptrs.len() + 1; // 1 for the schema
        let model_info_allocation = self
            .voxel_model_info_allocator_mut()
            .allocate(info_size.next_power_of_two() as u64 * 4)
            .expect("Couldn't allocate voxel mode info, out of room?");

        let should_append = 'should_append: {
            let Some(last_copy) = copies.last() else {
                break 'should_append false;
            };

            if (model_info_allocation.start_index_stride_dword() as u32) < last_copy.dst_index {
                break 'should_append false;
            }

            model_info_allocation.start_index_stride_dword() as u32
                == last_copy.dst_index + last_copy.src_data.len() as u32
        };

        if should_append {
            let Some(last_copy) = copies.last_mut() else {
                unreachable!();
            };
            let mut src_data = &mut last_copy.src_data;
            let original_length = src_data.len();
            src_data.reserve_exact(model_info_allocation.length_dword() as usize);
            src_data.push(voxel_model.schema());
            src_data.append(&mut model_gpu_info_ptrs);
            src_data.resize(src_data.capacity(), 0);
        } else {
            let mut src_data = Vec::with_capacity(model_info_allocation.length_dword() as usize);
            src_data.push(voxel_model.schema());
            src_data.append(&mut model_gpu_info_ptrs);
            src_data.resize(src_data.capacity(), 0);

            let new_copy = VoxelModelInfoCopy {
                src_data,
                dst_index: model_info_allocation.start_index_stride_dword() as u32,
            };
            copies.push(new_copy);
        }

        self.voxel_model_info_map.insert(
            voxel_model_id,
            VoxelWorldModelGpuInfo {
                info_allocation: model_info_allocation,
                voxel_model_dimensions: voxel_model.length(),
            },
        );
    }

    pub fn rendered_voxel_model_entity_count(&self) -> u32 {
        self.rendered_voxel_model_entity_count
    }

    pub fn terrain_side_length(&self) -> u32 {
        self.renderable_chunks.terrain_side_length
    }

    pub fn world_entity_acceleration_buffer(&self) -> &ResourceId<Buffer> {
        self.entity_acceleration_buffer.as_ref().expect(
            "world_entity_acceleration_buffer not initialized when it should have been by now",
        )
    }

    pub fn world_terrain_acceleration_buffer(&self) -> &ResourceId<Buffer> {
        self.renderable_chunks
            .terrain_acceleration_buffer
            .as_ref()
            .expect(
                "world_terrain_acceleration_buffer not initialized when it should have been by now",
            )
    }

    pub fn world_voxel_model_info_buffer(&self) -> &ResourceId<Buffer> {
        self.voxel_model_info_allocator
            .as_ref()
            .expect("world_voxel_model_info_buffer not initialized when it should have been now")
            .buffer()
    }

    pub fn world_data_buffers(&self) -> Vec<ResourceId<Buffer>> {
        self.voxel_data_allocator.buffers()
    }

    pub fn write_normal_calc_pass(
        mut renderer: ResMut<Renderer>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
    ) {
        let voxel_world: &mut VoxelWorld = &mut voxel_world;
        let voxel_world_gpu: &mut VoxelWorldGpu = &mut voxel_world_gpu;

        renderer.frame_graph_executor.supply_pass_ref(
            Renderer::GRAPH.normal_calc.pass_normal_calc,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext| {
                let terrain_pipeline =
                    ctx.get_compute_pipeline(Renderer::GRAPH.normal_calc.pipeline_compute_terrain);
                let mut compute_pass = recorder.begin_compute_pass(terrain_pipeline);

                for (i, chunk_pos) in voxel_world
                    .chunks
                    .renderable_chunks
                    .to_update_chunk_normals
                    .drain()
                    .enumerate()
                    .collect::<Vec<_>>()
                {
                    if voxel_world.chunks.get_chunk_node(chunk_pos).is_none() {
                        continue;
                    }

                    // Only update the normals for one chunk per frame.
                    // if i > 0 {
                    //     voxel_world
                    //         .chunks
                    //         .renderable_chunks
                    //         .to_update_chunk_normals
                    //         .insert(chunk_pos);
                    //     continue;
                    // }
                    log::debug!("Updating the normals for chunk {:?}", chunk_pos);
                    compute_pass.bind_uniforms(&mut |writer| {
                        writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                        writer.write_uniform("u_shader.world_chunk_pos", chunk_pos);
                    });
                    let vl = consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as f32;
                    let wg = compute_pass.workgroup_size().cast::<f32>();
                    compute_pass.dispatch(
                        (vl / wg.x).ceil() as u32,
                        (vl / wg.y).ceil() as u32,
                        (vl / wg.z).ceil() as u32,
                    );
                }
                drop(compute_pass);

                let standalone_pipeline = ctx
                    .get_compute_pipeline(Renderer::GRAPH.normal_calc.pipeline_compute_standalone);
                let mut compute_pass = recorder.begin_compute_pass(standalone_pipeline);

                let to_update_ids = voxel_world
                    .to_update_normals
                    .drain()
                    .map(|id| {
                        (
                            id,
                            voxel_world_gpu
                                .voxel_model_info_map
                                .get(&id)
                                .unwrap()
                                .info_allocation
                                .start_index_stride_dword(),
                        )
                    })
                    .collect::<Vec<_>>();
                for (model_id, model_info_ptr) in to_update_ids {
                    log::debug!(
                        "Updating the normals for model_id {:?} with ptr {:?}",
                        model_id,
                        model_info_ptr
                    );
                    compute_pass.bind_uniforms(&mut |writer| {
                        writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                        writer.write_uniform("u_shader.voxel_model_ptr", model_info_ptr as u32);
                    });
                    let vl = voxel_world.get_dyn_model(model_id).length().cast::<f32>();
                    let wg = compute_pass.workgroup_size().cast::<f32>();
                    compute_pass.dispatch(
                        (vl.x / wg.x).ceil() as u32,
                        (vl.y / wg.y).ceil() as u32,
                        (vl.z / wg.z).ceil() as u32,
                    );
                }
            },
        );
    }
}

struct VoxelModelInfoCopy {
    src_data: Vec<u32>,
    /// Destination index into the gpu-side u32 array.
    dst_index: u32,
}
