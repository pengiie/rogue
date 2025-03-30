use std::{
    any::{Any, TypeId},
    borrow::Borrow,
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
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
        asset::asset::Assets,
        ecs::ecs_world::ECSWorld,
        event::Events,
        graphics::{
            backend::{Buffer, GfxBufferCreateInfo, ResourceId},
            device::DeviceResource,
            gpu_allocator::{Allocation, GpuBufferAllocator},
        },
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{voxel::VoxelModelRange, voxel_terrain::ChunkData, voxel_world},
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
    voxel_terrain::{self, ChunkTreeGpu, VoxelChunks},
    voxel_transform::VoxelModelTransform,
};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VoxelModelId {
    id: u64,
}

impl VoxelModelId {
    pub fn null() -> Self {
        Self { id: u64::MAX }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct VoxelModelInfo {
    name: String,
    model_type: std::any::TypeId,
    gpu_type: Option<std::any::TypeId>,
    archetype_index: u64,
}

pub struct VoxelModelRegistry {
    /// Each archetype is (VoxelModel<T>, VoxelModelGpu<T::Gpu>).
    /// The key is the (TypeId::of::<T>()) and the value is (Archetype, TypeId::of::<T::Gpu>())
    renderable_voxel_model_archtypes: HashMap<TypeId, (Archetype, TypeId)>,
    /// Each archetype is (VoxelModel<T>)
    standalone_voxel_model_archtypes: HashMap<TypeId, Archetype>,
    // TODO: Create FreeList alloc generic impl and replace this with it so we can also unload
    // voxel models.
    voxel_model_info: Vec<VoxelModelInfo>,

    // Maps model_type to the vtable for dyn VoxelModelImpl and
    // maps gpu_type to the vtable for dyn VoxelModelGpuImpl.
    type_vtables: HashMap<TypeId, *mut ()>,
    id_counter: u64,
}

impl VoxelModelRegistry {
    pub fn new() -> Self {
        Self {
            renderable_voxel_model_archtypes: HashMap::new(),
            standalone_voxel_model_archtypes: HashMap::new(),
            voxel_model_info: Vec::new(),
            type_vtables: HashMap::new(),
            id_counter: 0,
        }
    }

    pub fn next_id(&mut self) -> VoxelModelId {
        let id = self.id_counter;
        self.id_counter += 1;
        VoxelModelId { id }
    }

    pub fn register_renderable_voxel_model<T>(
        &mut self,
        name: impl ToString,
        voxel_model: VoxelModel<T>,
    ) -> VoxelModelId
    where
        T: VoxelModelImplConcrete,
    {
        let voxel_model_gpu = VoxelModelGpu::new(T::Gpu::new());
        let model_type_info = TypeInfo::new::<T>();
        let gpu_type_info = TypeInfo::new::<T::Gpu>();
        let id = self.next_id();

        // Extract fat pointers for this voxel model T's implementation of VoxelModelImpl and
        // T::Gpu's VoxelModelGpuImpl.
        let voxel_model_vtable_ptr = {
            let dyn_ref/*: (*mut (), *mut ())*/ = voxel_model.deref() as &dyn VoxelModelImpl;
            let fat_ptr = std::ptr::from_ref(&dyn_ref) as *const _ as *const (*mut (), *mut ());
            // Safety: We know &dyn T aka. fat_ptr is a fat pointer containing two pointers.
            unsafe { fat_ptr.as_ref() }.unwrap().1
        };
        let voxel_model_gpu_vtable_ptr = {
            let dyn_ref/*: (*mut (), *mut ())*/ = voxel_model_gpu.deref() as &dyn VoxelModelGpuImpl;
            let fat_ptr = std::ptr::from_ref(&dyn_ref) as *const _ as *const (*mut (), *mut ());
            // Safety: We know &dyn T aka. fat_ptr is a fat pointer containing two pointers.
            unsafe { fat_ptr.as_ref() }.unwrap().1
        };

        self.type_vtables
            .insert(model_type_info.type_id(), voxel_model_vtable_ptr);
        self.type_vtables
            .insert(gpu_type_info.type_id(), voxel_model_gpu_vtable_ptr);

        let (ref mut archetype, _) = self
            .renderable_voxel_model_archtypes
            .entry(model_type_info.type_id())
            .or_insert_with(|| {
                (
                    Archetype::new(vec![model_type_info, gpu_type_info]),
                    gpu_type_info.type_id(),
                )
            });
        let archetype_index = archetype.insert(
            id.id,
            (voxel_model.into_model(), voxel_model_gpu.into_model_gpu()),
        );

        let info = VoxelModelInfo {
            name: name.to_string(),
            model_type: model_type_info.type_id(),
            gpu_type: Some(gpu_type_info.type_id()),
            archetype_index,
        };
        self.voxel_model_info.push(info);

        id
    }

    pub fn get_dyn_model<'a>(&'a self, id: VoxelModelId) -> &'a dyn VoxelModelImpl {
        let model_info = self
            .voxel_model_info
            .get(id.id as usize)
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &self
                .renderable_voxel_model_archtypes
                .get(&model_info.model_type)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type);

        // Safety: If model_info is still valid, then the archetype_index it contains must best
        // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
        // reference to self and explicity the lifetime.
        let model_ptr =
            unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *const u8 };

        let voxel_model_dyn_ref = {
            let model_vtable = *self.type_vtables.get(&model_info.model_type).unwrap();
            let fat_ptr = (model_ptr, model_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
        };

        voxel_model_dyn_ref
    }

    pub fn get_model<'a, T: VoxelModelImpl>(&'a self, id: VoxelModelId) -> &'a T {
        let model_info = self
            .voxel_model_info
            .get(id.id as usize)
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &self
                .renderable_voxel_model_archtypes
                .get(&model_info.model_type)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type);
        assert_eq!(model_type_info.type_id(), std::any::TypeId::of::<T>());

        // Safety: If model_info is still valid, then the archetype_index it contains must best
        // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
        // reference to self and explicity the lifetime.
        let model_ptr =
            unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *const u8 };

        // Safety: We asset above the type id matches.
        unsafe { (model_ptr as *const T).as_ref().unwrap() }
    }

    pub fn get_dyn_model_mut<'a>(&'a mut self, id: VoxelModelId) -> &'a mut dyn VoxelModelImpl {
        let model_info = self
            .voxel_model_info
            .get(id.id as usize)
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &mut self
                .renderable_voxel_model_archtypes
                .get_mut(&model_info.model_type)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type);

        // Safety: If model_info is still valid, then the archetype_index it contains must best
        // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
        // reference to self and explicity the lifetime.
        let model_ptr =
            unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *mut u8 };

        let voxel_model_dyn_ref = {
            let model_vtable = *self.type_vtables.get(&model_info.model_type).unwrap();
            let fat_ptr = (model_ptr, model_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
        };

        voxel_model_dyn_ref
    }

    pub fn get_dyn_renderable_model<'a>(
        &'a self,
        id: VoxelModelId,
    ) -> (&'a dyn VoxelModelImpl, &'a dyn VoxelModelGpuImpl) {
        let model_info = self
            .voxel_model_info
            .get(id.id as usize)
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &self
                .renderable_voxel_model_archtypes
                .get(&model_info.model_type)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type);
        let model_gpu_type_info = &archetype.type_infos()[1];
        assert_eq!(model_gpu_type_info.type_id(), model_info.gpu_type.unwrap());

        // Safety: If model_info is still valid, then the archetype_index it contains must best
        // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
        // reference to self and explicity the lifetime.
        let model_ptr =
            unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *mut u8 };

        let voxel_model_dyn_ref = {
            let model_vtable = *self.type_vtables.get(&model_info.model_type).unwrap();
            let fat_ptr = (model_ptr, model_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
        };

        let model_gpu_ptr = unsafe {
            archetype.get_raw(model_gpu_type_info, model_info.archetype_index) as *mut u8
        };

        let voxel_model_gpu_dyn_ref = {
            let model_gpu_vtable = *self
                .type_vtables
                .get(&model_gpu_type_info.type_id())
                .unwrap();
            let fat_ptr = (model_gpu_ptr, model_gpu_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelGpuImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
        };

        (voxel_model_dyn_ref, voxel_model_gpu_dyn_ref)
    }

    pub fn renderable_models_dyn_iter(&self) -> RenderableVoxelModelIter<'_> {
        let archetype_iters = self
            .renderable_voxel_model_archtypes
            .iter()
            .map(|(type_id, (archetype, gpu_type_id))| {
                (
                    *self.type_vtables.get(type_id).unwrap(),
                    *self.type_vtables.get(gpu_type_id).unwrap(),
                    archetype.iter(),
                )
            })
            .collect();
        RenderableVoxelModelIter {
            archetype_iters,
            current_archetype_index: 0,
        }
    }

    pub fn renderable_models_dyn_iter_mut(&mut self) -> RenderableVoxelModelIterMut<'_> {
        let archetype_iters_mut = self
            .renderable_voxel_model_archtypes
            .iter_mut()
            .map(|(type_id, (archetype, gpu_type_id))| {
                (
                    *self.type_vtables.get(type_id).unwrap(),
                    *self.type_vtables.get(gpu_type_id).unwrap(),
                    archetype.iter_mut(),
                )
            })
            .collect();
        RenderableVoxelModelIterMut {
            archetype_iters_mut,
            current_archetype_index: 0,
        }
    }
}

#[derive(Resource)]
pub struct VoxelWorld {
    pub registry: VoxelModelRegistry,
    pub chunks: VoxelChunks,

    pub last_player_position: Option<Vector3<f32>>,
}

impl VoxelWorld {
    pub fn new(settings: &Settings) -> Self {
        Self {
            registry: VoxelModelRegistry::new(),
            chunks: VoxelChunks::new(settings),
            last_player_position: None,
        }
    }

    pub fn init_chunk_loading(&mut self, assets: &mut Assets) {}

    pub fn update_post_physics(
        events: Res<Events>,
        settings: Res<Settings>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut assets: ResMut<Assets>,
    ) {
        let mut player_query = ecs_world.player_query::<&Transform>();
        if let Some((_, player_transform)) = player_query.try_player() {
            if voxel_world.last_player_position.is_none() {
                // Initialize chunk loading.
                //
            }
        }
        let terrain: &mut VoxelChunks = &mut voxel_world.chunks;

        terrain.try_queue_new_chunks();

        // Process next chunk.
        terrain.chunk_tree.is_dirty = false;
        terrain.chunk_queue.handle_enqueued_chunks();

        const RECIEVED_CHUNKS_PER_FRAME: usize = 1;
        // Loops until the reciever is empty.
        'lp: for _ in 0..RECIEVED_CHUNKS_PER_FRAME {
            match voxel_world
                .chunks
                .chunk_queue
                .finished_chunk_recv
                .try_recv()
            {
                Ok(finished_chunk) => {
                    voxel_world.chunks.chunk_queue.chunk_handler_count -= 1;
                    if finished_chunk.is_empty() {
                        voxel_world
                            .chunks
                            .chunk_tree
                            .set_world_chunk_empty(finished_chunk.chunk_position);
                    } else {
                        let chunk_name = format!(
                            "chunk_{}_{}_{}",
                            finished_chunk.chunk_position.x,
                            finished_chunk.chunk_position.y,
                            finished_chunk.chunk_position.z
                        );
                        let chunk_uuid = uuid::Uuid::new_v4();
                        let voxel_model_id = voxel_world.registry.register_renderable_voxel_model(
                            chunk_name,
                            VoxelModel::new(finished_chunk.esvo.unwrap()),
                        );
                        voxel_world.chunks.chunk_tree.set_world_chunk_data(
                            finished_chunk.chunk_position,
                            ChunkData {
                                chunk_uuid,
                                voxel_model_id,
                            },
                        );
                        debug!(
                            "Recieved finished chunk {:?}",
                            finished_chunk.chunk_position
                        );
                    }
                }
                Err(err) => match err {
                    std::sync::mpsc::TryRecvError::Disconnected => {
                        panic!("Shouldn't be disconnected")
                    }
                    _ => break 'lp,
                },
            }
        }
    }

    /// Returns the hit voxel with the corresponding ray.
    pub fn trace_terrain(&self, mut ray: Ray, _max_t: f32) -> Option<Vector3<i32>> {
        let chunks_aabb = self.chunks.chunks_aabb();
        let Some(terrain_t) = ray.intersect_aabb(&chunks_aabb) else {
            return None;
        };
        ray.advance(terrain_t);
        let mut chunk_dda = self.chunks.chunks_dda(&ray);
        debug!("PLAYER is in chunk {:?}", chunk_dda.curr_grid_pos());
        while (chunk_dda.in_bounds()) {
            let morton = morton::morton_traversal(
                morton::morton_encode(chunk_dda.curr_grid_pos().map(|x| x as u32)),
                self.chunks.chunk_tree.chunk_side_length.trailing_zeros(),
            );
            if let Some(chunk_data) = self.chunks.chunk_tree.get_chunk_data(morton) {
                let chunk_local = self.get_dyn_model(chunk_data.voxel_model_id);
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
                        (self.chunks.chunk_tree.chunk_origin + chunk_dda.curr_grid_pos())
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
                    let rel_chunk = Vector3::new(chunk_x, chunk_y, chunk_z)
                        - self.chunks.chunk_tree.chunk_origin;
                    if (rel_chunk.x < 0
                        || rel_chunk.y < 0
                        || rel_chunk.z < 0
                        || rel_chunk.x >= self.chunks.chunk_tree.chunk_side_length as i32
                        || rel_chunk.y >= self.chunks.chunk_tree.chunk_side_length as i32
                        || rel_chunk.z >= self.chunks.chunk_tree.chunk_side_length as i32)
                    {
                        continue;
                    }
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

                    let Some(chunk_data) = self
                        .chunks
                        .chunk_tree
                        .get_world_chunk_data(Vector3::new(chunk_x, chunk_y, chunk_z))
                    else {
                        debug!("Implement force loading and queing chunk edits");
                        continue;
                    };
                    let chunk_model = self.get_dyn_model_mut(chunk_data.voxel_model_id);
                    chunk_model.set_voxel_range(&VoxelModelRange {
                        offset: (min_offset - min_chunk_offset).map(|x| x as u32),
                        data: voxel_data,
                    });
                }
            }
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

struct RenderableVoxelModelIter<'a> {
    archetype_iters: Vec<(*mut (), *mut (), ArchetypeIter<'a>)>,
    current_archetype_index: usize,
}

impl<'a> std::iter::Iterator for RenderableVoxelModelIter<'a> {
    type Item = (
        VoxelModelId,
        (&'a dyn VoxelModelImpl, &'a dyn VoxelModelGpuImpl),
    );

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_archetype_index >= self.archetype_iters.len() {
            return None;
        }

        let (curr_model_vtable, curr_model_gpu_vtable, current_archetype) = self
            .archetype_iters
            .get_mut(self.current_archetype_index)
            .unwrap();

        let Some((global_id, ptrs)) = current_archetype.next().or_else(|| None) else {
            self.current_archetype_index += 1;
            return self.next();
        };

        let voxel_model_ref = {
            let fat_ptr = (ptrs[0].1, *curr_model_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
        };

        let voxel_model_gpu_ref = {
            let fat_ptr = (ptrs[1].1, *curr_model_gpu_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelGpuImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
        };

        Some((
            VoxelModelId { id: global_id },
            (voxel_model_ref, voxel_model_gpu_ref),
        ))
    }
}

struct RenderableVoxelModelIterMut<'a> {
    archetype_iters_mut: Vec<(*mut (), *mut (), ArchetypeIterMut<'a>)>,
    current_archetype_index: usize,
}

impl<'a> std::iter::Iterator for RenderableVoxelModelIterMut<'a> {
    type Item = (
        VoxelModelId,
        (&'a mut dyn VoxelModelImpl, &'a mut dyn VoxelModelGpuImpl),
    );

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_archetype_index >= self.archetype_iters_mut.len() {
            return None;
        }

        let (curr_model_vtable, curr_model_gpu_vtable, current_archetype) = self
            .archetype_iters_mut
            .get_mut(self.current_archetype_index)
            .unwrap();

        let Some((global_id, ptrs)) = current_archetype.next().or_else(|| None) else {
            self.current_archetype_index += 1;
            return self.next();
        };

        let voxel_model_ref = {
            let fat_ptr = (ptrs[0].1, *curr_model_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
        };

        let voxel_model_gpu_ref = {
            let fat_ptr = (ptrs[1].1, *curr_model_gpu_vtable);
            let mut dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelGpuImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
        };

        Some((
            VoxelModelId { id: global_id },
            (voxel_model_ref, voxel_model_gpu_ref),
        ))
    }
}

/// Type erased contiguous storage for objects (model_type, gpu_type).

#[derive(Resource)]
pub struct VoxelWorldGpu {
    /// The acceleration buffer for rendered entity voxel model bounds interaction, hold the
    /// pointed to voxel model index and the position and rotation matrix data of this entity.
    acceleration_buffer: Option<ResourceId<Buffer>>,
    /// The acceleration buffer for the voxel terrain.
    terrain_acceleration_buffer: Option<ResourceId<Buffer>>,
    /// The buffer for every unique voxel models info such as its data pointers and length.
    voxel_model_info_allocator: Option<GpuBufferAllocator>,
    voxel_model_info_map: HashMap<VoxelModelId, VoxelWorldModelGpuInfo>,
    // Rendered voxel models entities, count of entities pointing to models in the acceleration buffer.
    rendered_voxel_model_entity_count: u32,
    terrain_side_length: u32,

    /// The allocator that owns and manages the world data buffer holding all the voxel model
    /// information.
    voxel_data_allocator: Option<GpuBufferAllocator>,

    frame_state: VoxelWorldGpuFrameState,
}

struct VoxelWorldModelGpuInfo {
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
            acceleration_buffer: None,
            terrain_acceleration_buffer: None,
            voxel_model_info_allocator: None,
            voxel_model_info_map: HashMap::new(),
            rendered_voxel_model_entity_count: 0,
            terrain_side_length: 0,
            voxel_data_allocator: None,
            frame_state: VoxelWorldGpuFrameState::new(),
        }
    }

    pub fn did_buffers_update(&self) -> bool {
        self.frame_state.did_buffers_update
    }

    fn mark_dirty(&mut self) {
        self.frame_state.did_buffers_update = true;
    }

    pub fn voxel_allocator(&self) -> Option<&GpuBufferAllocator> {
        self.voxel_data_allocator.as_ref()
    }

    pub fn clear_frame_state(mut voxel_world_gpu: ResMut<VoxelWorldGpu>) {
        voxel_world_gpu.frame_state.clear();
    }

    pub fn update_gpu_objects(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        mut device: ResMut<DeviceResource>,
    ) {
        let voxel_world_gpu = &mut voxel_world_gpu;
        if voxel_world_gpu.acceleration_buffer.is_none() {
            voxel_world_gpu.acceleration_buffer = Some(device.create_buffer(GfxBufferCreateInfo {
                name: "world_entity_acceleration_buffer".to_owned(),
                size: 4 * 1000000,
            }));
            voxel_world_gpu.mark_dirty();
        }

        if voxel_world_gpu.terrain_acceleration_buffer.is_none() {
            voxel_world_gpu.terrain_acceleration_buffer =
                Some(device.create_buffer(GfxBufferCreateInfo {
                    name: "world_terrain_acceleration_buffer".to_owned(),
                    size: 4 * 1000000, // 1000 voxel models
                }));
            voxel_world_gpu.mark_dirty();
        }

        if voxel_world_gpu.voxel_model_info_allocator.is_none() {
            voxel_world_gpu.voxel_model_info_allocator = Some(GpuBufferAllocator::new(
                &mut device,
                "voxel_model_info_allocator",
                1 << 20,
            ));
            voxel_world_gpu.mark_dirty();
        }

        if voxel_world_gpu.voxel_data_allocator.is_none() {
            // 2 gig
            voxel_world_gpu.voxel_data_allocator = Some(GpuBufferAllocator::new(
                &mut device,
                "voxel_data_allocator",
                1 << 31,
            ));
            voxel_world_gpu.mark_dirty();
        }

        // TODO: Get entities with transform, then see which models are in view of the camera
        // frustum or near it, then update the voxel model those entities point to. This way we can
        // save on transfer bandwidth by only updating what we need. This will also be why
        // splitting up the terrain into discrete voxel models per chunk is important.
        for (voxel_model_id, (model, model_gpu)) in
            voxel_world.registry.renderable_models_dyn_iter_mut()
        {
            let allocator = voxel_world_gpu.voxel_data_allocator.as_mut().unwrap();
            if model_gpu.update_gpu_objects(allocator, model) {
                voxel_world_gpu
                    .frame_state
                    .updated_voxel_model_allocations
                    .push(voxel_model_id);
            }
        }
    }

    pub fn write_render_data(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        mut device: ResMut<DeviceResource>,
    ) {
        let Some(allocator) = &mut voxel_world_gpu.voxel_data_allocator else {
            return;
        };

        // Update gpu model buffer data (Do this first so the allocation data is ready to reference).
        for (entity, (mut voxel_model, mut voxel_model_gpu)) in
            voxel_world.registry.renderable_models_dyn_iter_mut()
        {
            voxel_model_gpu.deref_mut().write_gpu_updates(
                &mut device,
                allocator,
                voxel_model.deref_mut() as &mut dyn VoxelModelImpl,
            );
        }

        // Write any new renderable voxel model infos registered in the past frame.
        // TODO: Change from clone and figure out borrow checker stuff.
        for new_renderable_id in voxel_world_gpu
            .frame_state
            .updated_voxel_model_allocations
            .clone()
        {
            voxel_world_gpu.register_voxel_model_info(&voxel_world, new_renderable_id);
        }
        for voxel_model_info_copy in &voxel_world_gpu.frame_state.voxel_model_info_copies {
            // debug!(
            //     "MADE a COPY at {} with len {} {:?}",
            //     voxel_model_info_copy.dst_index * 4,
            //     voxel_model_info_copy.src_data.len() * 4,
            //     voxel_model_info_copy.src_data
            // );
            device.write_buffer_slice(
                voxel_world_gpu.world_voxel_model_info_buffer(),
                voxel_model_info_copy.dst_index as u64 * 4,
                bytemuck::cast_slice(voxel_model_info_copy.src_data.as_slice()),
            );
        }

        //let mut entity_voxel_models_query =
        //    ecs_world.query::<(&VoxelModelTransform, &RenderableVoxelModelRef)>();

        // Write acceleration buffer data.
        {
            //let mut entity_acceleration_data = Vec::new();
            //let mut rendered_entity_count = 0;
            //for (entity, (transform, RenderableVoxelModelRef(voxel_model_id))) in
            //    entity_voxel_models_query.iter()
            //{
            //    let Some((data_ptr, model)) = voxel_model_info_map.get(voxel_model_id) else {
            //        // The voxel model isn't ready to be rendered on the gpu so skip rendering it.
            //        continue;
            //    };

            //    let obb = transform.as_obb(*model);

            //    // Each acceleration data entity has the form.
            //    //   - Oriented bounding box data
            //    //   - Voxel model info data pointer (indexing in voxel_model_info_buffer)
            //    entity_acceleration_data.append(&mut obb.as_acceleration_data());
            //    entity_acceleration_data.push(*data_ptr);

            //    rendered_entity_count += 1;
            //}

            //// Used in the gpu world info to know the limits of the acceleration buffer.
            //voxel_world_gpu.rendered_voxel_model_entity_count = rendered_entity_count;

            //device.queue().write_buffer(
            //    voxel_world_gpu.world_acceleration_buffer(),
            //    0,
            //    bytemuck::cast_slice(&entity_acceleration_data),
            //);
        }

        // Write voxel terrain acceleration buffer data.
        if voxel_world.chunks.is_chunk_tree_dirty()
            || !voxel_world_gpu
                .frame_state
                .voxel_model_info_copies
                .is_empty()
        {
            let chunk_tree = voxel_world.chunks.chunk_tree();
            voxel_world_gpu.terrain_side_length = chunk_tree.chunk_side_length();
            let chunk_tree_gpu = ChunkTreeGpu::build(
                chunk_tree,
                voxel_world_gpu
                    .voxel_model_info_map
                    .iter()
                    .map(|(id, info)| (*id, info.info_allocation.start_index_stride_dword() as u32))
                    .collect::<HashMap<_, _>>(),
            );

            device.write_buffer_slice(
                voxel_world_gpu.world_terrain_acceleration_buffer(),
                0,
                bytemuck::cast_slice(&chunk_tree_gpu.data),
            );
        }
    }

    fn voxel_model_info_allocator_mut(&mut self) -> &mut GpuBufferAllocator {
        self.voxel_model_info_allocator.as_mut().unwrap()
    }

    fn register_voxel_model_info(
        &mut self,
        voxel_world: &VoxelWorld,
        voxel_model_id: VoxelModelId,
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
            let Some(last_copy) = self.frame_state.voxel_model_info_copies.last() else {
                break 'should_append false;
            };

            if (model_info_allocation.start_index_stride_dword() as u32) < last_copy.dst_index {
                break 'should_append false;
            }

            model_info_allocation.start_index_stride_dword() as u32
                == last_copy.dst_index + last_copy.src_data.len() as u32
        };

        if should_append {
            let Some(last_copy) = self.frame_state.voxel_model_info_copies.last_mut() else {
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
            self.frame_state.voxel_model_info_copies.push(new_copy);
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
        self.terrain_side_length
    }

    pub fn world_acceleration_buffer(&self) -> &ResourceId<Buffer> {
        self.acceleration_buffer
            .as_ref()
            .expect("world_acceleration_buffer not initialized when it should have been by now")
    }

    pub fn world_terrain_acceleration_buffer(&self) -> &ResourceId<Buffer> {
        self.terrain_acceleration_buffer.as_ref().expect(
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
