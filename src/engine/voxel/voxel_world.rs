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
use crate::{
    common::geometry::aabb::AABB,
    engine::voxel::{
        thc::{VoxelModelTHC, VoxelModelTHCCompressed},
        voxel::VoxelModelType,
    },
};
use crate::{common::geometry::ray::Ray, engine::voxel::sft_compressed::VoxelModelSFTCompressed};
use crate::{
    common::{
        archetype::{Archetype, ArchetypeIter, ArchetypeIterMut},
        bitset::Bitset,
        dyn_vec::TypeInfo,
        morton::{self, morton_encode, morton_traversal_octree},
    },
    consts::{self, voxel::VOXEL_METER_LENGTH},
    engine::{
        asset::asset::{AssetHandle, AssetPath, Assets},
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
            voxel_world,
            voxel_world_gpu::VoxelWorldGpu,
        },
        window::time::Stopwatch,
    },
    session::Session,
    settings::Settings,
};

pub struct QueuedVoxelEdit {
    chunk_pos: Vector3<i32>,
    data: VoxelModelEdit,
}

pub struct AsyncVoxelEdit {
    chunk_pos: Vector3<i32>,
    local_min: Vector3<u32>,
    local_max: Vector3<u32>,
    attachment_map: AttachmentInfoMap,
    edit_fn: Box<
        dyn Fn(
                VoxelEdit,
                /*world_voxel_pos*/ Vector3<i32>,
                /*local_voxel_pos=*/ Vector3<u32>,
            ) + Send,
    >,
}

pub struct VoxelModelFlatEdit {
    pub flat: VoxelModelFlat,
}

impl VoxelModelFlatEdit {
    // Leaves the whole voxel range untouched, meaning if the edit is applied nothing will happen.
    pub fn new_empty(side_length: Vector3<u32>, attachment_map: AttachmentInfoMap) -> Self {
        let mut flat = VoxelModelFlat::new_empty(side_length);
        for (_, attachment) in attachment_map.iter() {
            flat.initialize_attachment_buffers(attachment);
        }
        Self { flat }
    }

    pub fn side_length(&self) -> &Vector3<u32> {
        self.flat.side_length()
    }

    // Only enable when we do the override setting property.
    // pub fn set_untouched(&mut self, voxel_index: usize) {
    //     self.flat.presence_data.set_bit(voxel_index, false);
    //     for (_, data) in &mut self.flat.attachment_presence_data {
    //         data.set_bit(voxel_index, false);
    //     }
    // }

    pub fn set_removed(&mut self, voxel_index: usize) {
        self.flat.presence_data.set_bit(voxel_index, true);
        for (_, data) in self.flat.attachment_presence_data.iter_mut() {
            data.set_bit(voxel_index, false);
        }
    }

    pub fn set_attachment(
        &mut self,
        voxel_index: usize,
        attachment_id: AttachmentId,
        data: &[u32],
    ) {
        let attachment = self.flat.attachment_map.get_unchecked(attachment_id);
        self.flat.presence_data.set_bit(voxel_index, true);
        self.flat
            .attachment_presence_data
            .get_mut(attachment_id)
            .unwrap()
            .set_bit(voxel_index, true);
        let initial_offset = voxel_index * attachment.size() as usize;
        self.flat.attachment_data.get_mut(attachment_id).unwrap()
            [initial_offset..(initial_offset + attachment.size() as usize)]
            .copy_from_slice(data);
    }
}

pub struct VoxelEdit<'a> {
    flat: &'a mut VoxelModelFlatEdit,
    voxel_index: usize,
}

impl<'a> VoxelEdit<'a> {
    pub fn new(flat: &'a mut VoxelModelFlatEdit, local_pos: Vector3<u32>) -> Self {
        let voxel_index = flat.flat.get_voxel_index(local_pos);
        Self { flat, voxel_index }
    }

    //pub fn set_untouched(&mut self) {
    //    self.flat.set_untouched(self.voxel_index);
    //}

    pub fn set_removed(&mut self) {
        self.flat.set_removed(self.voxel_index);
    }

    pub fn set_attachment(&mut self, attachment_id: AttachmentId, data: &[u32]) {
        self.flat
            .set_attachment(self.voxel_index, attachment_id, data);
    }
}

#[derive(Resource)]
pub struct VoxelWorld {
    pub registry: VoxelModelRegistry,
    pub chunks: VoxelChunks,
    pub global_materials: VoxelMaterialSet,
    pub to_update_normals: HashSet<VoxelModelId>,

    // TODO: Queue up edit tasks but make the edit on the chunk itself atomic and async, then
    // switch over to the new chunk after the async handler is done so there is no longer frame
    // stutter when applying edits.
    pub edit_queue: VecDeque<QueuedVoxelEdit>,
    pub async_edit_queue: VecDeque<AsyncVoxelEdit>,
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
            global_materials: VoxelMaterialSet::new(4),
            to_update_normals: HashSet::new(),

            chunk_edit_handler_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(
                    std::thread::available_parallelism()
                        .map(|x| x.get())
                        .unwrap_or(0),
                )
                .build()
                .unwrap(),
            chunk_edit_handler_count: 0,
            finished_chunk_edit_recv,
            finished_chunk_edit_send,
            edit_queue: VecDeque::new(),
            async_edit_queue: VecDeque::new(),
        }
    }

    /// Number of async edits currently in progress.
    pub fn async_edit_count(&self) -> u32 {
        self.async_edit_queue.len() as u32 + self.chunk_edit_handler_count
    }

    pub fn register_renderable_voxel_model<T>(
        &mut self,
        name: impl ToString,
        voxel_model: VoxelModel<T>,
    ) -> VoxelModelId
    where
        T: VoxelModelImplConcrete,
    {
        let id = self
            .registry
            .register_renderable_voxel_model(name, voxel_model);
        self.to_update_normals.insert(id);
        return id;
    }

    pub fn process_chunk_edits(&mut self, assets: &mut Assets) {
        if let Some(next_edit) = self.edit_queue.front() {
            todo!("Check for chunk load then process edit.");
        }

        // Check if the thread pool for chunk voxel edits has a thread available.
        if self.chunk_edit_handler_count < self.chunk_edit_handler_pool.current_num_threads() as u32
        {
            // Get the next async edit we need to perform.
            if let Some(next_async_edit) = self.async_edit_queue.pop_front() {
                let finish_sender = self.finished_chunk_edit_send.clone();
                self.chunk_edit_handler_count += 1;
                self.chunk_edit_handler_pool.spawn(move || {
                    let stopwatch = Stopwatch::new("edit_handler scope");

                    let mi = next_async_edit.local_min;
                    let ma = next_async_edit.local_max;
                    let chunk_voxel_pos = next_async_edit.chunk_pos
                        * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32;

                    let mut flat_edit =
                        VoxelModelFlatEdit::new_empty(ma - mi, next_async_edit.attachment_map);
                    for z in mi.z..ma.z {
                        for y in mi.y..ma.y {
                            for x in mi.x..ma.x {
                                let local_pos = Vector3::new(x, y, z);
                                let world_pos = chunk_voxel_pos + local_pos.cast::<i32>();
                                (*next_async_edit.edit_fn)(
                                    VoxelEdit::new(&mut flat_edit, local_pos),
                                    world_pos,
                                    local_pos,
                                );
                            }
                        }
                    }

                    finish_sender.send(FinishedChunkEdit {
                        chunk_position: next_async_edit.chunk_pos,
                        edit_result: VoxelModelEdit {
                            offset: next_async_edit.local_min,
                            data: flat_edit,
                        },
                    });
                });
            }
        }

        match self.finished_chunk_edit_recv.try_recv() {
            Ok(finished_edit) => {
                self.chunk_edit_handler_count -= 1;
                Self::apply_edit_to_chunk(
                    &mut self.chunks,
                    &mut self.registry,
                    &mut self.edit_queue,
                    finished_edit.chunk_position,
                    finished_edit.edit_result,
                );
            }
            Err(err) => match err {
                std::sync::mpsc::TryRecvError::Empty => {}
                std::sync::mpsc::TryRecvError::Disconnected => {
                    log::error!("Error with async edit thread disconnection");
                }
            },
        }
    }

    pub fn clear_state(mut voxel_world: ResMut<VoxelWorld>) {
        voxel_world.chunks.renderable_chunks.is_dirty = false;
    }

    pub fn update_render_center(&mut self, center_pos: Vector3<f32>) {
        self.chunks.update_player_position(center_pos);
    }

    // Processes:
    // - Voxel terrain chunks
    // - Terrain chunk edits (async handlers, and sync)
    // - Model removal with gpu mem dealloc
    pub fn update_post_physics(
        events: Res<Events>,
        settings: Res<Settings>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        mut assets: ResMut<Assets>,
        session: Res<Session>,
    ) {
        let voxel_world: &mut VoxelWorld = &mut voxel_world;
        let chunks: &mut VoxelChunks = &mut voxel_world.chunks;
        // let mut player_query = ecs_world.player_query::<&Transform>();

        // if let Some((_, player_transform)) = player_query.try_player() {
        //     let player_pos = player_transform.position;
        // }

        chunks.try_update_chunk_render_distance(&settings);
        chunks.update_chunk_queue(&mut assets, &mut voxel_world.registry, &session);
        voxel_world.process_chunk_edits(&mut assets);

        for model in voxel_world
            .chunks
            .renderable_chunks
            .to_unload_models
            .drain(..)
        {
            assert_ne!(model, VoxelModelId::null());
            voxel_world
                .registry
                .unload_model(model, voxel_world_gpu.voxel_allocator_mut());
        }
    }

    pub fn trace_world(&self, mut ecs_world: &ECSWorld, mut ray: Ray) -> Option<VoxelTraceInfo> {
        let mut closest_entity: Option<(f32, hecs::Entity, VoxelModelId, Vector3<u32>)> = None;

        let mut renderable_model_query = ecs_world.query::<(&Transform, &RenderableVoxelEntity)>();
        for (entity_id, (local_transform, renderable)) in renderable_model_query.iter() {
            let Some(voxel_model_id) = renderable.voxel_model_id() else {
                continue;
            };

            let model = self.registry.get_dyn_model(voxel_model_id);

            let world_transform = ecs_world.get_world_transform(entity_id, &local_transform);
            let half_side_length = model
                .length()
                .zip_map(&world_transform.scale, |x, y| x as f32 * y)
                * consts::voxel::VOXEL_METER_LENGTH
                * 0.5;
            let min = world_transform.position - half_side_length;
            let max = world_transform.position + half_side_length;
            let aabb = AABB::new_two_point(min, max);
            let rotation_anchor = world_transform.position;
            let r = world_transform.rotation.to_rotation_matrix().inverse();

            let rotated_ray_origin =
                (r.matrix() * (ray.origin - rotation_anchor)) + rotation_anchor;
            let rotated_ray_dir = r.matrix() * ray.dir;
            let rotated_ray = Ray::new(rotated_ray_origin, rotated_ray_dir);
            let Some(model_trace) = model.trace(&rotated_ray, &aabb) else {
                continue;
            };

            if let Some(last_closest) = &closest_entity {
                if last_closest.0 < model_trace.depth_t {
                    continue;
                }
            };

            closest_entity = Some((
                model_trace.depth_t,
                entity_id,
                voxel_model_id,
                model_trace.local_position,
            ));
        }

        if let Some((world_voxel_hit, depth_t)) = self.trace_terrain(ray, 1000.0) {
            let mut is_closer = true;
            if let Some((entity_t, _, _, _)) = &closest_entity {
                is_closer = is_closer && depth_t < *entity_t;
            }
            if is_closer {
                return Some(VoxelTraceInfo::Terrain {
                    world_voxel_pos: world_voxel_hit,
                });
            }
        }

        if let Some((entity_t, entity_id, entity_model_id, entity_local_voxel)) = closest_entity {
            return Some(VoxelTraceInfo::Entity {
                entity_id,
                voxel_model_id: entity_model_id,
                local_voxel_pos: entity_local_voxel,
            });
        }

        return None;
    }

    /// Returns the hit voxel with the corresponding ray.
    pub fn trace_terrain(
        &self,
        mut ray: Ray,
        _max_t: f32,
    ) -> Option<(Vector3<i32>, /*depth_t=*/ f32)> {
        let chunks_aabb = self.chunks.renderable_chunks_aabb();
        let Some(terrain_t) = ray.intersect_aabb(&chunks_aabb) else {
            return None;
        };
        ray.advance(terrain_t);
        let mut chunk_dda = self.chunks.renderable_chunks_dda(&ray);
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
                    let world_voxel_pos = (self.chunks.renderable_chunks.chunk_anchor
                        + chunk_dda.curr_grid_pos())
                    .map(|x| x * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32)
                        + local_chunk_hit.local_position.cast::<i32>();
                    return Some((world_voxel_pos, local_chunk_hit.depth_t));
                }
            }
            chunk_dda.step();
        }

        return None;
    }

    fn apply_edit_to_chunk(
        mut chunks: &mut VoxelChunks,
        mut registry: &mut VoxelModelRegistry,
        mut edit_queue: &mut VecDeque<QueuedVoxelEdit>,
        world_chunk_pos: Vector3<i32>,
        chunk_edit: VoxelModelEdit,
    ) {
        let chunk = chunks.get_chunk_node(world_chunk_pos);
        // Check if the chunk is loaded already.
        let Some(chunk) = chunk else {
            edit_queue.push_back(QueuedVoxelEdit {
                chunk_pos: world_chunk_pos,
                data: chunk_edit,
            });
            return;
        };

        match chunk {
            VoxelRegionLeafNode::Empty => {
                let mut node = chunks
                    .get_or_create_chunk_node_mut(world_chunk_pos)
                    .expect("Region should be loaded by now");

                let mut new_flat = VoxelModelFlat::new_empty(Vector3::new(
                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                ));
                new_flat.set_voxel_range(&chunk_edit);
                if new_flat.is_empty() {
                    *node = VoxelRegionLeafNode::new_air();
                    chunks
                        .renderable_chunks
                        .try_load_chunk(&world_chunk_pos, VoxelModelId::air());
                } else {
                    let new_chunk_model = VoxelModelSFT::from(&new_flat);

                    let new_model_id = registry.register_renderable_voxel_model(
                        format!(
                            "chunk_{}_{}_{}",
                            world_chunk_pos.x, world_chunk_pos.y, world_chunk_pos.z
                        ),
                        VoxelModel::new(new_chunk_model),
                    );

                    *node = VoxelRegionLeafNode::new_with_model(new_model_id);
                    chunks
                        .renderable_chunks
                        .try_load_chunk(&world_chunk_pos, new_model_id);
                }
                chunks.mark_chunk_edited(world_chunk_pos);
                chunks.mark_region_edited(VoxelChunks::chunk_to_region_pos(&world_chunk_pos));
            }
            VoxelRegionLeafNode::Existing { uuid, model } => {
                let Some(model_id) = model else {
                    edit_queue.push_back(QueuedVoxelEdit {
                        chunk_pos: world_chunk_pos,
                        data: chunk_edit,
                    });
                    return;
                };

                let chunk_model = registry.get_dyn_model_mut(*model_id);
                chunk_model.set_voxel_range(&chunk_edit);
                chunks.mark_chunk_edited(world_chunk_pos);
            }
        }
    }

    pub fn apply_voxel_edit_entity(
        &mut self,
        edit: VoxelEditEntityInfo,
        f: impl Fn(
            VoxelEdit,
            /*world_voxel_pos*/ Vector3<i32>,
            /*local_voxel_pos=*/ Vector3<u32>,
        ),
    ) {
        let chunk_model = self.registry.get_dyn_model_mut(edit.model_id);
        let side_length = chunk_model.length();

        let min_offset =
            (edit.local_voxel_pos).zip_map(&side_length, |x, len| x.clamp(0, len as i32) as u32);
        let max_offset = (edit.local_voxel_pos + edit.voxel_length.cast::<i32>())
            .zip_map(&side_length, |x, len| x.clamp(0, len as i32) as u32);

        let edit_offset = edit
            .local_voxel_pos
            .zip_map(&min_offset, |x, y| (x.max(0) as u32 - y));
        let edit_length = (max_offset - min_offset).map(|x| x as u32);

        if edit_length.x == 0
            || edit_length.y == 0
            || edit_length.z == 0
            || min_offset.x >= side_length.x
            || min_offset.y >= side_length.y
            || min_offset.z >= side_length.z
        {
            return;
        }

        let mut voxel_data =
            VoxelModelFlatEdit::new_empty(edit_length, edit.attachment_map.clone());
        for voxel_z in min_offset.z..max_offset.z {
            for voxel_y in min_offset.y..max_offset.y {
                for voxel_x in min_offset.x..max_offset.x {
                    let world_voxel_pos = Vector3::new(voxel_x, voxel_y, voxel_z);
                    let local_voxel_pos = world_voxel_pos - min_offset;
                    f(
                        VoxelEdit::new(&mut voxel_data, local_voxel_pos),
                        world_voxel_pos.cast::<i32>(),
                        local_voxel_pos,
                    );
                }
            }
        }
        chunk_model.set_voxel_range(&VoxelModelEdit {
            offset: min_offset,
            data: voxel_data,
        });
        self.to_update_normals.insert(edit.model_id);
    }

    pub fn apply_voxel_edit(
        &mut self,
        edit: VoxelEditInfo,
        f: impl Fn(
            VoxelEdit,
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
                    let mut voxel_data = VoxelModelFlatEdit::new_empty(
                        (max_offset - min_offset).map(|x| x as u32),
                        edit.attachment_map.clone(),
                    );
                    debug!(
                        "min offset {:?}, min_chunk_offset: {:?}, max offset {:?}, max-min {:?}",
                        min_offset,
                        min_chunk_offset,
                        max_offset,
                        (max_offset - min_offset).map(|x| x as u32),
                    );
                    for voxel_z in min_offset.z..max_offset.z {
                        for voxel_y in min_offset.y..max_offset.y {
                            for voxel_x in min_offset.x..max_offset.x {
                                let world_voxel_pos = Vector3::new(voxel_x, voxel_y, voxel_z);
                                let local_voxel_pos =
                                    (world_voxel_pos - min_offset).map(|x| x as u32);
                                f(
                                    VoxelEdit::new(&mut voxel_data, local_voxel_pos),
                                    world_voxel_pos,
                                    local_voxel_pos,
                                );
                            }
                        }
                    }

                    let world_chunk_pos = Vector3::new(chunk_x, chunk_y, chunk_z);
                    let chunk_edit = VoxelModelEdit {
                        offset: (min_offset - min_chunk_offset).map(|x| x as u32),
                        data: voxel_data,
                    };
                    log::info!("APplying iwth offset {:?}", chunk_edit.offset);
                    Self::apply_edit_to_chunk(
                        &mut self.chunks,
                        &mut self.registry,
                        &mut self.edit_queue,
                        world_chunk_pos,
                        chunk_edit,
                    )
                }
            }
        }
    }

    pub fn apply_voxel_edit_async(
        &mut self,
        edit: VoxelEditInfo,
        f: impl Fn(
                VoxelEdit,
                /*world_voxel_pos*/ Vector3<i32>,
                /*local_voxel_pos=*/ Vector3<u32>,
            ) + Send
            + Clone
            + 'static,
    ) {
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

                    let world_chunk_pos = Vector3::new(chunk_x, chunk_y, chunk_z);
                    let chunk_local_min = (min_offset - min_chunk_offset).map(|x| x as u32);
                    let chunk_local_max = (max_offset - min_chunk_offset).map(|x| x as u32);
                    if ((chunk_local_max - chunk_local_min).iter().any(|x| *x == 0)) {
                        continue;
                    }

                    self.async_edit_queue.push_back(AsyncVoxelEdit {
                        chunk_pos: world_chunk_pos,
                        local_min: chunk_local_min,
                        local_max: chunk_local_max,
                        attachment_map: edit.attachment_map.clone(),
                        edit_fn: Box::new(f.clone()),
                    })
                }
            }
        }
    }

    // Issues the save request in the registry and updates the model info with the provided asset
    // path the model was saved to.
    pub fn save_model(
        &mut self,
        assets: &mut Assets,
        model_id: VoxelModelId,
        asset_path: AssetPath,
    ) {
        let mut model_info = self.registry.get_model_info_mut(model_id).unwrap();
        model_info.asset_path = Some(asset_path.clone());
        match &model_info.model_type {
            Some(VoxelModelType::Flat) => {
                let flat = self.get_model::<VoxelModelFlat>(model_id).clone();
                assets.save_asset(asset_path, flat);
            }
            Some(VoxelModelType::THC) => {
                let thc = self.get_model::<VoxelModelTHC>(model_id);
                assets.save_asset(asset_path, VoxelModelTHCCompressed::from(thc));
            }
            Some(VoxelModelType::THCCompressed) => {
                let thc_compressed = self.get_model::<VoxelModelTHCCompressed>(model_id).clone();
                assets.save_asset(asset_path, thc_compressed);
            }
            Some(VoxelModelType::SFT) => {
                let sft = self.get_model::<VoxelModelSFT>(model_id).clone();
                assets.save_asset(asset_path, VoxelModelSFTCompressed::from(&sft));
            }
            Some(VoxelModelType::SFTCompressed) => {
                let sft_compressed = self.get_model::<VoxelModelSFTCompressed>(model_id).clone();
                assets.save_asset(asset_path, sft_compressed);
            }
            None => {
                log::error!("Don't know how to save this asset format");
            }
            ty => todo!("Save model type {:?}", ty),
        }
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

pub enum VoxelTraceInfo {
    Terrain {
        world_voxel_pos: Vector3<i32>,
    },
    Entity {
        entity_id: hecs::Entity,
        voxel_model_id: VoxelModelId,
        local_voxel_pos: Vector3<u32>,
    },
}

pub struct FinishedChunkEdit {
    pub chunk_position: Vector3<i32>,
    pub edit_result: VoxelModelEdit,
}
