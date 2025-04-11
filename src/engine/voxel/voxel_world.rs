use std::{
    any::{Any, TypeId},
    borrow::Borrow,
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
    sync::mpsc::{Receiver, Sender},
    u64,
};

use hecs::Entity;
use log::debug;
use nalgebra::{allocator, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{
        aabb::AABB,
        archetype::{Archetype, ArchetypeIter, ArchetypeIterMut},
        dyn_vec::TypeInfo,
        morton::{self, morton_encode, morton_traversal},
        ray::Ray,
    },
    consts::{self, voxel::VOXEL_METER_LENGTH},
    engine::{
        asset::asset::{AssetHandle, Assets},
        ecs::ecs_world::ECSWorld,
        event::Events,
        graphics::{
            backend::{Buffer, GfxBufferCreateInfo, ResourceId},
            device::DeviceResource,
            gpu_allocator::{Allocation, GpuBufferAllocator},
        },
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{
            voxel::VoxelModelRange,
            voxel_terrain::{ChunkModelType, VoxelRegionLeafNode},
            voxel_world,
        },
    },
    settings::Settings,
};

use super::{
    cursor::VoxelEdit,
    esvo::{VoxelModelESVO, VoxelModelESVOGpu},
    flat::VoxelModelFlat,
    voxel::{
        RenderableVoxelModelRef, VoxelModel, VoxelModelGpu, VoxelModelGpuImpl,
        VoxelModelGpuImplConcrete, VoxelModelImpl, VoxelModelImplConcrete, VoxelModelSchema,
    },
    voxel_registry::{VoxelModelId, VoxelModelRegistry},
    voxel_terrain::{self, RenderableChunksGpu, VoxelChunks},
    voxel_transform::VoxelModelTransform,
};

#[derive(Resource)]
pub struct VoxelWorld {
    pub registry: VoxelModelRegistry,
    pub chunks: VoxelChunks,

    pub chunk_edit_handler_pool: rayon::ThreadPool,
    pub chunk_edit_handler_count: u32,
    pub finished_chunk_edit_recv: Receiver<FinishedChunkEdit>,
    pub finished_chunk_edit_send: Sender<FinishedChunkEdit>,
}

impl VoxelWorld {
    pub fn new(settings: &Settings) -> Self {
        let (finished_chunk_edit_send, finished_chunk_edit_recv) = std::sync::mpsc::channel();

        Self {
            registry: VoxelModelRegistry::new(),
            chunks: VoxelChunks::new(settings),

            chunk_edit_handler_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap(),
            chunk_edit_handler_count: 0,
            finished_chunk_edit_recv,
            finished_chunk_edit_send,
        }
    }

    pub fn process_chunk_edits(&mut self, assets: &mut Assets) {}

    pub fn clear_state(mut voxel_world: ResMut<VoxelWorld>) {
        voxel_world.chunks.renderable_chunks.is_dirty = false;
    }

    pub fn update_post_physics(
        events: Res<Events>,
        settings: Res<Settings>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut assets: ResMut<Assets>,
    ) {
        let voxel_world: &mut VoxelWorld = &mut voxel_world;
        let chunks: &mut VoxelChunks = &mut voxel_world.chunks;
        let mut player_query = ecs_world.player_query::<&Transform>();
        if let Some((_, player_transform)) = player_query.try_player() {
            let player_pos = player_transform.isometry.translation.vector;
            chunks.update_player_position(player_pos);
        }

        chunks.update_chunk_queue(&mut assets, &mut voxel_world.registry);
    }

    /// Returns the hit voxel with the corresponding ray.
    pub fn trace_terrain(&self, mut ray: Ray, _max_t: f32) -> Option<Vector3<i32>> {
        let chunks_aabb = self.chunks.renderable_chunks_aabb();
        let Some(terrain_t) = ray.intersect_aabb(&chunks_aabb) else {
            return None;
        };
        ray.advance(terrain_t);
        let mut chunk_dda = self.chunks.renderable_chunks_dda(&ray);
        debug!("PLAYER is in chunk {:?}", chunk_dda.curr_grid_pos());
        while (chunk_dda.in_bounds()) {
            if let Some(chunk_model_id) = self
                .chunks
                .renderable_chunks
                .get_chunk_model(chunk_dda.curr_grid_pos().map(|x| x as u32))
            {
                let chunk_local = self.get_dyn_model(chunk_model_id);
                let chunks_aabb_min = chunks_aabb.min
                    + chunk_dda
                        .curr_grid_pos()
                        .map(|x| x as f32 * consts::voxel::TERRAIN_CHUNK_METER_LENGTH);
                let chunk_aabb = AABB::new_two_point(
                    chunks_aabb_min,
                    chunks_aabb_min
                        + Vector3::new(
                            consts::voxel::TERRAIN_CHUNK_METER_LENGTH,
                            consts::voxel::TERRAIN_CHUNK_METER_LENGTH,
                            consts::voxel::TERRAIN_CHUNK_METER_LENGTH,
                        ),
                );
                if let Some(local_chunk_hit) = chunk_local.trace(&ray, &chunk_aabb) {
                    return Some(
                        (self.chunks.renderable_chunks.chunk_anchor + chunk_dda.curr_grid_pos())
                            .map(|x| x * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32)
                            + local_chunk_hit.cast::<i32>(),
                    );
                }
            }
            chunk_dda.step();
        }

        return None;
    }

    pub fn apply_voxel_edit(
        &mut self,
        edit: VoxelEdit,
        f: impl Fn(
            &mut VoxelModelFlat,
            /*world_voxel_pos*/ Vector3<i32>,
            /*local_voxel_pos=*/ Vector3<u32>,
        ),
    ) {
        log::info!(
            "Applying edit at {:?} with length {:?}.",
            edit.world_voxel_position,
            edit.world_voxel_length
        );
        let edit_voxel_max = edit.world_voxel_position + edit.world_voxel_length.cast::<i32>();
        let chunk_min = edit
            .world_voxel_position
            .map(|x| x.div_euclid(consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32));
        let chunk_max = (edit_voxel_max + Vector3::new(-1, -1, -1))
            .map(|x| x.div_euclid(consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32));
        for chunk_x in chunk_min.x..=chunk_max.x {
            for chunk_y in chunk_min.y..=chunk_max.y {
                for chunk_z in chunk_min.z..=chunk_max.z {
                    // The bottom-left-back voxel of the current chunk we are in.
                    let min_chunk_offset = Vector3::new(
                        chunk_x * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                        chunk_y * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                        chunk_z * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                    );
                    let min_offset = edit
                        .world_voxel_position
                        .zip_map(&min_chunk_offset, |x, y| x.max(y));
                    let max_offset = (min_chunk_offset
                        + Vector3::new(
                            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                        ))
                    .zip_map(&edit_voxel_max, |x, y| x.min(y));
                    debug!("min offset {:?}, max offset {:?}", min_offset, max_offset);
                    let mut voxel_data =
                        VoxelModelFlat::new_empty((max_offset - min_offset).map(|x| x as u32));
                    for voxel_x in min_offset.x..max_offset.x {
                        for voxel_y in min_offset.y..max_offset.y {
                            for voxel_z in min_offset.z..max_offset.z {
                                let world_voxel_pos = Vector3::new(voxel_x, voxel_y, voxel_z);
                                let local_voxel_pos =
                                    (world_voxel_pos - min_offset).map(|x| x as u32);
                                f(&mut voxel_data, world_voxel_pos, local_voxel_pos);
                            }
                        }
                    }

                    let world_chunk_pos = Vector3::new(chunk_x, chunk_y, chunk_z);
                    let chunk = self.chunks.get_chunk_node(world_chunk_pos);
                    let Some(chunk) = chunk else {
                        todo!("Queue to ensure chunk load and queue the edit.")
                    };

                    match chunk {
                        VoxelRegionLeafNode::Empty => {
                            let mut node = self
                                .chunks
                                .get_or_create_chunk_node_mut(world_chunk_pos)
                                .expect("Region should be loaded by now");
                            if voxel_data
                                .side_length()
                                .iter()
                                .all(|x| *x == consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH)
                            {
                                // Make edit override the chunk.
                                let new_model_id = self.registry.register_renderable_voxel_model(
                                    format!(
                                        "chunk_{}_{}_{}",
                                        world_chunk_pos.x, world_chunk_pos.y, world_chunk_pos.z
                                    ),
                                    VoxelModel::new(ChunkModelType::from(voxel_data)),
                                );

                                *node = VoxelRegionLeafNode::new_with_model(new_model_id);
                                self.chunks
                                    .renderable_chunks
                                    .try_load_chunk(&world_chunk_pos, new_model_id);
                            } else {
                                let mut new_chunk_model = ChunkModelType::new_empty(Vector3::new(
                                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                ));
                                new_chunk_model.set_voxel_range(&VoxelModelRange {
                                    offset: (min_offset - min_chunk_offset).map(|x| x as u32),
                                    data: voxel_data,
                                });

                                let new_model_id = self.registry.register_renderable_voxel_model(
                                    format!(
                                        "chunk_{}_{}_{}",
                                        world_chunk_pos.x, world_chunk_pos.y, world_chunk_pos.z
                                    ),
                                    VoxelModel::new(new_chunk_model),
                                );

                                *node = VoxelRegionLeafNode::new_with_model(new_model_id);
                                self.chunks
                                    .renderable_chunks
                                    .try_load_chunk(&world_chunk_pos, new_model_id);
                            }
                            self.chunks.mark_chunk_edited(world_chunk_pos);
                            self.chunks
                                .mark_region_edited(VoxelChunks::chunk_to_region_pos(
                                    &world_chunk_pos,
                                ));
                        }
                        VoxelRegionLeafNode::Existing { uuid, model } => {
                            let Some(model_id) = model else {
                                todo!("Qeueu to ensure chunk load and queue the edit.");
                            };

                            let chunk_model = self.get_dyn_model_mut(*model_id);
                            chunk_model.set_voxel_range(&VoxelModelRange {
                                offset: (min_offset - min_chunk_offset).map(|x| x as u32),
                                data: voxel_data,
                            });
                            self.chunks.mark_chunk_edited(world_chunk_pos);
                        }
                    }
                }
            }
        }
    }

    pub fn apply_voxel_edit_async(
        &mut self,
        edit: VoxelEdit,
        f: impl Fn(
                &mut VoxelModelFlat,
                /*world_voxel_pos*/ Vector3<i32>,
                /*local_voxel_pos=*/ Vector3<u32>,
            ) + 'static,
    ) {
    }

    pub fn get_model<T: VoxelModelImpl>(&self, id: VoxelModelId) -> &T {
        self.registry.get_model(id)
    }

    pub fn get_dyn_model(&self, id: VoxelModelId) -> &dyn VoxelModelImpl {
        self.registry.get_dyn_model(id)
    }

    pub fn get_dyn_model_mut(&mut self, id: VoxelModelId) -> &mut dyn VoxelModelImpl {
        self.registry.get_dyn_model_mut(id)
    }
}

pub struct FinishedChunkEdit {
    pub chunk_position: Vector3<i32>,
    pub edit_result: VoxelModelRange,
}

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
    voxel_data_allocator: Option<GpuBufferAllocator>,

    to_register_models: Vec<VoxelModelId>,
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

            voxel_data_allocator: None,
            to_register_models: Vec::new(),
            //        frame_state: VoxelWorldGpuFrameState::new(),
        }
    }

    pub fn voxel_allocator(&self) -> Option<&GpuBufferAllocator> {
        self.voxel_data_allocator.as_ref()
    }

    pub fn update_gpu_objects(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        mut device: ResMut<DeviceResource>,
    ) {
        let voxel_world_gpu = &mut voxel_world_gpu;
        if voxel_world_gpu.entity_acceleration_buffer.is_none() {
            voxel_world_gpu.entity_acceleration_buffer =
                Some(device.create_buffer(GfxBufferCreateInfo {
                    name: "world_entity_acceleration_buffer".to_owned(),
                    size: 4 * 1000,
                }));
        }

        if voxel_world_gpu.voxel_model_info_allocator.is_none() {
            voxel_world_gpu.voxel_model_info_allocator = Some(GpuBufferAllocator::new(
                &mut device,
                "voxel_model_info_allocator",
                1 << 20,
            ));
        }

        if voxel_world_gpu.voxel_data_allocator.is_none() {
            // 2 gig
            voxel_world_gpu.voxel_data_allocator = Some(GpuBufferAllocator::new(
                &mut device,
                "voxel_data_allocator",
                1 << 31,
            ));
        }

        // Update terrain acceleration buffer.
        voxel_world_gpu
            .renderable_chunks
            .update_gpu_objects(&mut device, &voxel_world.chunks.renderable_chunks);

        // TODO: Get entities with transform, then see which models are in view of the camera
        // frustum or near it, then update the voxel model those entities point to. This way we can
        // save on transfer bandwidth by only updating what we need. This will also be why
        // splitting up the terrain into discrete voxel models per chunk is important.
        for (voxel_model_id, (model, model_gpu)) in
            voxel_world.registry.renderable_models_dyn_iter_mut()
        {
            let allocator = voxel_world_gpu.voxel_data_allocator.as_mut().unwrap();
            if model_gpu.update_gpu_objects(allocator, model) {
                voxel_world_gpu.to_register_models.push(voxel_model_id);
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
        let Some(allocator) = &mut voxel_world_gpu.voxel_data_allocator else {
            return;
        };

        // Update gpu model buffer data, do this first so the allocation data is ready to reference
        // when registering updated_voxel_model_allocations.
        for (entity, (mut voxel_model, mut voxel_model_gpu)) in
            voxel_world.registry.renderable_models_dyn_iter_mut()
        {
            voxel_model_gpu.deref_mut().write_gpu_updates(
                &mut device,
                allocator,
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
        for model_info_copy in registered_model_infos {
            device.write_buffer_slice(
                voxel_world_gpu.world_voxel_model_info_buffer(),
                model_info_copy.dst_index as u64 * 4,
                bytemuck::cast_slice(model_info_copy.src_data.as_slice()),
            );
        }

        voxel_world_gpu.renderable_chunks.write_render_data(
            &mut device,
            &voxel_world.chunks.renderable_chunks,
            &voxel_world_gpu.voxel_model_info_map,
        );
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

    pub fn world_acceleration_buffer(&self) -> &ResourceId<Buffer> {
        self.entity_acceleration_buffer
            .as_ref()
            .expect("world_acceleration_buffer not initialized when it should have been by now")
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

    pub fn world_data_buffer(&self) -> Option<&ResourceId<Buffer>> {
        self.voxel_data_allocator
            .as_ref()
            .map(|allocator| allocator.buffer())
    }
}

struct VoxelModelInfoCopy {
    src_data: Vec<u32>,
    /// Destination index into the gpu-side u32 array.
    dst_index: u32,
}
