use std::{
    any::TypeId,
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use crate::{
    common::{
        archetype::{Archetype, ArchetypeIter, ArchetypeIterMut},
        dyn_vec::TypeInfo,
        freelist::{FreeList, FreeListHandle},
    },
    engine::{
        asset::{asset::AssetPath, repr::voxel::any::VoxelModelAnyAsset},
        voxel::voxel_allocator::VoxelDataAllocator,
    },
};

use super::{
    flat::{VoxelModelFlat, VoxelModelFlatGpu},
    sft::VoxelModelSFT,
    sft_compressed::VoxelModelSFTCompressed,
    sft_compressed_gpu::VoxelModelSFTCompressedGpu,
    sft_gpu::VoxelModelSFTGpu,
    thc::{VoxelModelTHC, VoxelModelTHCCompressed, VoxelModelTHCCompressedGpu, VoxelModelTHCGpu},
    voxel::{
        VoxelModel, VoxelModelGpu, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
        VoxelModelImplConcrete, VoxelModelType,
    },
};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VoxelModelId {
    pub id: u64,
}

impl VoxelModelId {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn air() -> Self {
        Self { id: 0x0000_FFFEu64 }
    }

    pub fn is_air(&self) -> bool {
        self.id == 0x0000_FFFEu64
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
    pub name: String,
    pub model_type_id: std::any::TypeId,
    pub model_type: Option<VoxelModelType>,
    gpu_type: Option<std::any::TypeId>,
    // The index within the archetype the model is assigned to.
    archetype_index: u64,
    pub asset_path: Option<AssetPath>,
}

#[derive(Clone)]
pub struct VoxelModelRegistry {
    /// Each archetype is (VoxelModel<T>, VoxelModelGpu<T::Gpu>).
    /// The key is the (TypeId::of::<T>()) and the value is (Archetype, TypeId::of::<T::Gpu>())
    renderable_voxel_model_archtypes: HashMap<TypeId, (Archetype, TypeId)>,
    /// Each archetype is (VoxelModel<T>)
    standalone_voxel_model_archtypes: HashMap<TypeId, Archetype>,
    // TODO: Create FreeList alloc generic impl and replace this with it so we can also unload
    // voxel models.
    pub voxel_model_info: FreeList<VoxelModelInfo>,

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
            voxel_model_info: FreeList::new(),
            type_vtables: HashMap::new(),
            id_counter: 0,
        }
    }

    pub fn get_model_info(&self, id: VoxelModelId) -> Option<&VoxelModelInfo> {
        self.voxel_model_info
            .get(FreeListHandle::new(id.id as usize))
    }

    pub fn get_model_info_mut(&mut self, id: VoxelModelId) -> Option<&mut VoxelModelInfo> {
        self.voxel_model_info
            .get_mut(FreeListHandle::new(id.id as usize))
    }

    pub fn next_id(&mut self) -> VoxelModelId {
        let id = self.id_counter;
        self.id_counter += 1;
        VoxelModelId { id }
    }

    pub fn set_voxel_model_asset_path(
        &mut self,
        voxel_model_id: VoxelModelId,
        asset_path: Option<AssetPath>,
    ) {
        self.voxel_model_info
            .get_mut(FreeListHandle::new(voxel_model_id.id as usize))
            .unwrap()
            .asset_path = asset_path;
    }

    /// Noop if model is already unloaded or doesn't exist.
    pub fn unload_model(&mut self, id: VoxelModelId, voxel_allocator: &mut VoxelDataAllocator) {
        let info_handle = FreeListHandle::new(id.id as usize);
        let Some(info) = self.voxel_model_info.get(info_handle) else {
            return;
        };
        if info.gpu_type.is_some() {
            let mut dyn_gpu = self.get_dyn_gpu_model_mut(id);
            dyn_gpu.deallocate(voxel_allocator);
            log::info!("deallocated {:?}", id);

            let info = self.voxel_model_info.get(info_handle).unwrap();
            self.renderable_voxel_model_archtypes
                .get_mut(&info.model_type_id)
                .unwrap()
                .0
                .remove(info.archetype_index);
            log::info!("removed");
        } else {
            log::info!("deallocated standalone {:?}", id);
            self.standalone_voxel_model_archtypes
                .get_mut(&info.model_type_id)
                .unwrap()
                .remove(info.archetype_index);
        }
        self.voxel_model_info.remove(info_handle);
    }

    // TODO: Add more methods to the Impl so we don't have like 50 match statements.
    pub fn register_renderable_voxel_model_any(
        &mut self,
        name: impl ToString,
        voxel_model_any: VoxelModelAnyAsset,
    ) -> VoxelModelId {
        let voxel_model_gpu: Box<dyn VoxelModelGpuImpl> = match voxel_model_any.model_type {
            VoxelModelType::Flat => Box::new(VoxelModelFlatGpu::new()),
            VoxelModelType::THC => Box::new(VoxelModelTHCGpu::new()),
            VoxelModelType::THCCompressed => Box::new(VoxelModelTHCCompressedGpu::new()),
            VoxelModelType::SFT => Box::new(VoxelModelSFTGpu::new()),
            VoxelModelType::SFTCompressed => Box::new(VoxelModelSFTCompressedGpu::new()),
        };
        let model_type_info = match voxel_model_any.model_type {
            VoxelModelType::Flat => TypeInfo::new::<VoxelModelFlat>(),
            VoxelModelType::THC => TypeInfo::new::<VoxelModelTHC>(),
            VoxelModelType::THCCompressed => TypeInfo::new::<VoxelModelTHCCompressed>(),
            VoxelModelType::SFT => TypeInfo::new::<VoxelModelSFT>(),
            VoxelModelType::SFTCompressed => TypeInfo::new::<VoxelModelSFTCompressed>(),
        };
        let gpu_type_info = match voxel_model_any.model_type {
            VoxelModelType::Flat => TypeInfo::new::<VoxelModelFlatGpu>(),
            VoxelModelType::THC => TypeInfo::new::<VoxelModelTHCGpu>(),
            VoxelModelType::THCCompressed => TypeInfo::new::<VoxelModelTHCCompressedGpu>(),
            VoxelModelType::SFT => TypeInfo::new::<VoxelModelSFTGpu>(),
            VoxelModelType::SFTCompressed => TypeInfo::new::<VoxelModelSFTCompressedGpu>(),
        };
        let id = self.next_id();

        // Extract fat pointers for this voxel model T's implementation of VoxelModelImpl and
        // T::Gpu's VoxelModelGpuImpl.
        let voxel_model_vtable_ptr = {
            let dyn_ref/*: (*mut (), *mut ())*/ = voxel_model_any.model.deref() as &dyn VoxelModelImpl;
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

        let archetype_index = match voxel_model_any.model_type {
            VoxelModelType::Flat => archetype.insert(
                id.id,
                (
                    *voxel_model_any.model.downcast::<VoxelModelFlat>().unwrap(),
                    *voxel_model_gpu.downcast::<VoxelModelFlatGpu>().unwrap(),
                ),
            ),
            VoxelModelType::THC => archetype.insert(
                id.id,
                (
                    *voxel_model_any.model.downcast::<VoxelModelTHC>().unwrap(),
                    *voxel_model_gpu.downcast::<VoxelModelTHCGpu>().unwrap(),
                ),
            ),
            VoxelModelType::THCCompressed => archetype.insert(
                id.id,
                (
                    *voxel_model_any
                        .model
                        .downcast::<VoxelModelTHCCompressed>()
                        .unwrap(),
                    *voxel_model_gpu
                        .downcast::<VoxelModelTHCCompressedGpu>()
                        .unwrap(),
                ),
            ),
            VoxelModelType::SFT => archetype.insert(
                id.id,
                (
                    *voxel_model_any.model.downcast::<VoxelModelSFT>().unwrap(),
                    *voxel_model_gpu.downcast::<VoxelModelSFTGpu>().unwrap(),
                ),
            ),
            VoxelModelType::SFTCompressed => archetype.insert(
                id.id,
                (
                    *voxel_model_any
                        .model
                        .downcast::<VoxelModelSFTCompressed>()
                        .unwrap(),
                    *voxel_model_gpu
                        .downcast::<VoxelModelSFTCompressedGpu>()
                        .unwrap(),
                ),
            ),
        };

        let info = VoxelModelInfo {
            name: name.to_string(),
            model_type: Some(voxel_model_any.model_type),
            model_type_id: model_type_info.type_id(),
            gpu_type: Some(gpu_type_info.type_id()),
            archetype_index,
            asset_path: None,
        };
        self.voxel_model_info.push(info);

        id
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
            model_type: T::model_type(),
            model_type_id: model_type_info.type_id(),
            gpu_type: Some(gpu_type_info.type_id()),
            archetype_index,
            asset_path: None,
        };
        self.voxel_model_info.push(info);

        id
    }

    pub fn get_dyn_model<'a>(&'a self, id: VoxelModelId) -> &'a dyn VoxelModelImpl {
        let model_info = self
            .voxel_model_info
            .get(FreeListHandle::new(id.id as usize))
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &self
                .renderable_voxel_model_archtypes
                .get(&model_info.model_type_id)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type_id);

        // Safety: If model_info is still valid, then the archetype_index it contains must best
        // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
        // reference to self and explicity the lifetime.
        let model_ptr =
            unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *const u8 };

        let voxel_model_dyn_ref = {
            let model_vtable = *self.type_vtables.get(&model_info.model_type_id).unwrap();
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
            .get(FreeListHandle::new(id.id as usize))
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &self
                .renderable_voxel_model_archtypes
                .get(&model_info.model_type_id)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type_id);
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
            .get(FreeListHandle::new(id.id as usize))
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &mut self
                .renderable_voxel_model_archtypes
                .get_mut(&model_info.model_type_id)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type_id);

        // Safety: If model_info is still valid, then the archetype_index it contains must best
        // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
        // reference to self and explicity the lifetime.
        let model_ptr =
            unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *mut u8 };

        let voxel_model_dyn_ref = {
            let model_vtable = *self.type_vtables.get(&model_info.model_type_id).unwrap();
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
            .get(FreeListHandle::new(id.id as usize))
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            todo!("Fetch from standalone archetype");
        } else {
            &self
                .renderable_voxel_model_archtypes
                .get(&model_info.model_type_id)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type_id);
        let model_gpu_type_info = &archetype.type_infos()[1];
        assert_eq!(model_gpu_type_info.type_id(), model_info.gpu_type.unwrap());

        // Safety: If model_info is still valid, then the archetype_index it contains must best
        // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
        // reference to self and explicity the lifetime.
        let model_ptr =
            unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *mut u8 };

        let voxel_model_dyn_ref = {
            let model_vtable = *self.type_vtables.get(&model_info.model_type_id).unwrap();
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

    pub fn get_dyn_gpu_model_mut<'a>(
        &'a mut self,
        id: VoxelModelId,
    ) -> &'a mut dyn VoxelModelGpuImpl {
        let model_info = self
            .voxel_model_info
            .get(FreeListHandle::new(id.id as usize))
            .expect("Voxel model id is invalid");

        let archetype = if model_info.gpu_type.is_none() {
            panic!("nope");
        } else {
            &self
                .renderable_voxel_model_archtypes
                .get(&model_info.model_type_id)
                .unwrap()
                .0
        };

        let model_type_info = &archetype.type_infos()[0];
        assert_eq!(model_type_info.type_id(), model_info.model_type_id);
        let model_gpu_type_info = &archetype.type_infos()[1];
        assert_eq!(model_gpu_type_info.type_id(), model_info.gpu_type.unwrap());

        let model_gpu_ptr = unsafe {
            archetype.get_raw(model_gpu_type_info, model_info.archetype_index) as *mut u8
        };

        let voxel_model_gpu_dyn_ref = {
            let model_gpu_vtable = *self
                .type_vtables
                .get(&model_gpu_type_info.type_id())
                .unwrap();
            let fat_ptr = (model_gpu_ptr, model_gpu_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelGpuImpl;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
        };

        voxel_model_gpu_dyn_ref
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
