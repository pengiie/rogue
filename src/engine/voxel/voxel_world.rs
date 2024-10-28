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
        archetype::{Archetype, ArchetypeIter, ArchetypeIterMut, TypeInfo},
    },
    engine::{
        ecs::ecs_world::ECSWorld,
        graphics::device::DeviceResource,
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::vox_consts,
    },
};

use super::{
    esvo::{VoxelModelESVO, VoxelModelESVOGpu},
    voxel::{
        RenderableVoxelModelRef, VoxelModel, VoxelModelGpu, VoxelModelGpuImpl,
        VoxelModelGpuImplConcrete, VoxelModelImpl, VoxelModelImplConcrete, VoxelModelSchema,
        VoxelRange,
    },
    voxel_allocator::VoxelAllocator,
    voxel_transform::VoxelModelTransform,
};

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
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

#[derive(Resource)]
pub struct VoxelWorld {
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

impl VoxelWorld {
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
            return None;
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
            return None;
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
    acceleration_buffer: Option<wgpu::Buffer>,
    /// The buffer for every unique voxel models info such as its data pointers and length.
    voxel_model_info_buffer: Option<wgpu::Buffer>,
    // Rendered voxel models entities, count of entities pointing to models in the acceleration buffer.
    rendered_voxel_model_entity_count: u32,

    /// The allocator that owns and manages the world data buffer holding all the voxel model
    /// information.
    allocator: Option<VoxelAllocator>,

    // Some gpu object was changed (handle-wise), signals bind group recreation.
    is_dirty: bool,
}

impl VoxelWorldGpu {
    pub fn new() -> Self {
        Self {
            acceleration_buffer: None,
            voxel_model_info_buffer: None,
            rendered_voxel_model_entity_count: 0,
            allocator: None,
            is_dirty: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    pub fn update_gpu_objects(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        device: Res<DeviceResource>,
    ) {
        // Refresh any dirty flag from the last frame.
        voxel_world_gpu.is_dirty = false;

        if voxel_world_gpu.acceleration_buffer.is_none() {
            voxel_world_gpu.acceleration_buffer =
                Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("world_acceleration_buffer"),
                    size: 4 * 1000, // 1000 voxel models
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            voxel_world_gpu.is_dirty = true;
        }

        if voxel_world_gpu.voxel_model_info_buffer.is_none() {
            voxel_world_gpu.voxel_model_info_buffer =
                Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("world_voxel_model_info_buffer"),
                    size: 4 * 1000, // 1000 u32s
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            voxel_world_gpu.is_dirty = true;
        }

        if voxel_world_gpu.allocator.is_none() {
            voxel_world_gpu.allocator = Some(VoxelAllocator::new(&device, 1 << 24));
            voxel_world_gpu.is_dirty = true;
        }
        let allocator = voxel_world_gpu.allocator.as_mut().unwrap();

        // TODO: Get entities with transform, then see which models are in view of the camera
        // frustum or near it, then update the voxel model those entities point to. This way we can
        // save on transfer bandwidth by only updating what we need. This will also be why
        // splitting up the terrain into discrete voxel models per chunk is important.
        for (voxel_model_id, (model, model_gpu)) in voxel_world.renderable_models_dyn_iter_mut() {
            model_gpu.update_gpu_objects(allocator, model);
        }
    }

    pub fn write_render_data(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        device: Res<DeviceResource>,
    ) {
        let Some(allocator) = &mut voxel_world_gpu.allocator else {
            return;
        };
        // Update gpu model buffer data (Do this first so the allocation data is ready )
        {
            for (entity, (mut voxel_model, mut voxel_model_gpu)) in
                voxel_world.renderable_models_dyn_iter_mut()
            {
                voxel_model_gpu.deref_mut().write_gpu_updates(
                    &device,
                    allocator,
                    voxel_model.deref_mut() as &mut dyn VoxelModelImpl,
                );
            }
        }

        let mut entity_voxel_models_query =
            ecs_world.query::<(&VoxelModelTransform, &RenderableVoxelModelRef)>();

        // TODO: Implement frustum culling.
        let entity_voxel_models = entity_voxel_models_query.iter().collect::<Vec<_>>();
        let used_voxel_model_ids = entity_voxel_models
            .iter()
            .map(|(_, (transform, id_ref))| *id_ref.deref().deref())
            .collect::<HashSet<_>>();
        let mut voxel_model_info_map = HashMap::new();

        // Update renderable voxel model info buffer.
        let mut voxel_model_info_data: Vec<u32> = Vec::new();
        for (voxel_model_id, (voxel_model, voxel_model_gpu)) in
            voxel_world.renderable_models_dyn_iter()
        {
            if !used_voxel_model_ids.contains(&voxel_model_id) {
                continue;
            }

            let model_info_ptr = voxel_model_info_data.len() as u32;
            let Some(mut model_gpu_info_ptrs) = voxel_model_gpu.aggregate_model_info() else {
                continue;
            };
            assert!(!model_gpu_info_ptrs.is_empty());
            // Each voxel model info has the form.
            //   - Schema
            //   - Voxel model specific gpu data

            voxel_model_info_data.push(voxel_model.schema());
            voxel_model_info_data.append(&mut model_gpu_info_ptrs);

            voxel_model_info_map.insert(voxel_model_id, (model_info_ptr, voxel_model));
        }
        device.queue().write_buffer(
            voxel_world_gpu.world_voxel_model_info_buffer(),
            0,
            bytemuck::cast_slice(&voxel_model_info_data),
        );

        // Write acceleration buffer data.
        {
            let mut entity_acceleration_data = Vec::new();
            for (entity, (transform, RenderableVoxelModelRef(voxel_model_id))) in
                &entity_voxel_models
            {
                let Some((data_ptr, model)) = voxel_model_info_map.get(voxel_model_id) else {
                    // The voxel model isn't ready to be rendered on the gpu so skip rendering it.
                    continue;
                };

                let obb = transform.as_obb(*model);

                // Each acceleration data entity has the form.
                //   - Oriented bounding box data
                //   - Voxel model info data pointer (indexing in voxel_model_info_buffer)
                entity_acceleration_data.append(&mut obb.as_acceleration_data());
                entity_acceleration_data.push(*data_ptr);
            }

            // Used in the gpu world info to know the limits of the acceleration buffer.
            voxel_world_gpu.rendered_voxel_model_entity_count = entity_voxel_models.len() as u32;

            device.queue().write_buffer(
                voxel_world_gpu.world_acceleration_buffer(),
                0,
                bytemuck::cast_slice(&entity_acceleration_data),
            );
        }
    }

    pub fn rendered_voxel_model_entity_count(&self) -> u32 {
        self.rendered_voxel_model_entity_count
    }

    pub fn world_acceleration_buffer(&self) -> &wgpu::Buffer {
        self.acceleration_buffer
            .as_ref()
            .expect("world_acceleration_buffer not initialized when it should have been by now")
    }

    pub fn world_voxel_model_info_buffer(&self) -> &wgpu::Buffer {
        self.voxel_model_info_buffer
            .as_ref()
            .expect("world_voxel_model_info_buffer not initialized when it should have been now")
    }

    pub fn world_data_buffer(&self) -> Option<&wgpu::Buffer> {
        self.allocator
            .as_ref()
            .map(|allocator| allocator.world_data_buffer())
    }
}
