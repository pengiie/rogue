use std::{
    any::TypeId,
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use crate::common::{
    archetype::{Archetype, ArchetypeIter, ArchetypeIterMut},
    dyn_vec::TypeInfo,
};

use super::voxel::{
    VoxelModel, VoxelModelGpu, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
    VoxelModelImplConcrete,
};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VoxelModelId {
    id: u64,
}

impl VoxelModelId {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn null() -> Self {
        Self { id: u64::MAX }
    }

    pub fn is_null(&self) -> bool {
        self.id == u64::MAX
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

pub struct RenderableVoxelModelIter<'a> {
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

pub struct RenderableVoxelModelIterMut<'a> {
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
