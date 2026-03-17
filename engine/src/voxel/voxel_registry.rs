use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
    path::Path,
    ptr::NonNull,
};

use rogue_macros::Resource;

use super::{
    flat::{VoxelModelFlat, VoxelModelFlatGpu},
    sft::VoxelModelSFT,
    sft_compressed::VoxelModelSFTCompressed,
    sft_compressed_gpu::VoxelModelSFTCompressedGpu,
    sft_gpu::VoxelModelSFTGpu,
    thc::{VoxelModelTHC, VoxelModelTHCCompressed, VoxelModelTHCCompressedGpu, VoxelModelTHCGpu},
    voxel::{VoxelModelGpuImpl, VoxelModelGpuImplMethods, VoxelModelImpl, VoxelModelImplMethods},
};
use crate::entity::{
    RenderableVoxelEntity,
    ecs_world::{ECSWorld, Entity},
};
use crate::event::{EventReader, Events};
use crate::resource::{Res, ResMut};
use crate::{
    asset::{
        asset::{AssetHandle, AssetPath, AssetStatus, Assets, GameAssetPath},
        repr::voxel::any::VoxelModelAsset,
    },
    common::vtable,
};
use crate::{
    common::{
        archetype::{Archetype, ArchetypeIter, ArchetypeIterMut},
        dyn_vec::{DynVec, TypeInfo, TypeInfoCloneable},
        freelist::{FreeList, FreeListHandle},
    },
    voxel::rvox_asset::RVOXAsset,
};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VoxelModelId {
    pub handle: FreeListHandle<VoxelModelInfo>,
}

impl VoxelModelId {
    pub fn new(handle: FreeListHandle<VoxelModelInfo>) -> Self {
        Self { handle }
    }

    pub fn air() -> Self {
        Self {
            handle: FreeListHandle::new(0x0000_FFFE, 0),
        }
    }

    pub fn is_air(&self) -> bool {
        self.handle.index() == 0x0000_FFFE
    }

    pub fn null() -> Self {
        Self::new(FreeListHandle::DANGLING)
    }

    pub fn is_null(&self) -> bool {
        self.handle.is_null()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VoxelModelEvent {
    UpdatedModel(VoxelModelId),
}

#[derive(Clone, Eq, PartialEq)]
pub struct VoxelModelInfo {
    pub model_type_id: std::any::TypeId,
    /// The index within the DynVec the model is assigned to.
    index: u64,
    /// For asset based models.
    pub asset_path: Option<GameAssetPath>,
}

pub struct VoxelModelTypeInfo {
    // Vtable to VoxelModelImplMethods.
    model_impl_vtable: *const (),
    model_type_info: TypeInfoCloneable,
}

// TODO: Follow pattern of collider registry and ecs where we get concrete type info and remove
// VoxelModelType.
#[derive(Resource)]
pub struct VoxelModelRegistry {
    /// Each archetype is (VoxelModel<T>, VoxelModelGpu<T::Gpu>).
    /// The key is the (TypeId::of::<T>()) and the value is (Archetype, TypeId::of::<T::Gpu>())
    //renderable_voxel_model_archtypes: HashMap<TypeId, (Archetype, TypeId)>,
    ///// Each archetype is (VoxelModel<T>)
    //standalone_voxel_model_archtypes: HashMap<TypeId, Archetype>,
    pub voxel_model_types: HashMap<TypeId, VoxelModelTypeInfo>,
    pub voxel_model_type_names: HashMap<String, TypeId>,
    pub voxel_model_data: HashMap<TypeId, DynVec>,
    pub voxel_model_info: FreeList<VoxelModelInfo>,

    /// Models which are tied to a specific project asset and are not destructible.
    /// Essentially allows for caching and reuse of model data between entities.
    pub static_asset_models: HashMap<GameAssetPath, VoxelModelId>,
    loading_static_model_handles: HashMap<GameAssetPath, AssetHandle>,
    to_load_static_asset_models: HashSet<GameAssetPath>,
    ///// Non-terrain voxel models that need their normals updated.
    //pub to_update_model_normals: Vec<VoxelModelId>,
    //
}

impl VoxelModelRegistry {
    pub fn new() -> Self {
        let mut s = Self {
            voxel_model_types: HashMap::new(),
            voxel_model_type_names: HashMap::new(),
            voxel_model_data: HashMap::new(),
            voxel_model_info: FreeList::new(),

            static_asset_models: HashMap::new(),
            loading_static_model_handles: HashMap::new(),
            to_load_static_asset_models: HashSet::new(),
        };

        //s.register_voxel_model_type::<VoxelModelFlat>();
        //s.register_voxel_model_type::<VoxelModelSFT>();
        s.register_voxel_model_type::<VoxelModelSFTCompressed>();

        s
    }

    pub fn get_asset_model_id(&self, asset_path: &GameAssetPath) -> Option<VoxelModelId> {
        self.static_asset_models.get(asset_path).cloned()
    }

    pub fn get_model_asset_path(&self, voxel_model_id: VoxelModelId) -> Option<GameAssetPath> {
        self.voxel_model_info
            .get(voxel_model_id.handle)
            .expect("Given id doesn't exist.")
            .asset_path
            .clone()
    }

    pub fn get_voxel_model_type_id(&self, voxel_model_id: VoxelModelId) -> TypeId {
        self.voxel_model_info
            .get(voxel_model_id.handle)
            .expect("Given id doesn't exist.")
            .model_type_id
    }

    pub fn register_voxel_model_type<T: VoxelModelImpl>(&mut self) {
        let type_id = std::any::TypeId::of::<T>();
        if self.voxel_model_types.contains_key(&type_id) {
            return;
        }

        // Basically copied from `ECSWorld::register_game_component`.
        // Safety: We never access the contents of the pointer, only extracting the vtable, so
        // should be okay right? Use `without_provenance_mut` since this ptr isn't actually
        // associated with a memory allocation.
        let null = unsafe { NonNull::new_unchecked(std::ptr::without_provenance_mut::<T>(0x1234)) };
        let dyn_ref = unsafe { null.as_ref() } as &dyn VoxelModelImplMethods;
        // Safety: This reference is in fact a dyn ref.
        let vtable_ptr = unsafe { vtable::get_vtable_ptr(dyn_ref as &dyn VoxelModelImplMethods) };
        self.voxel_model_types.insert(
            type_id,
            VoxelModelTypeInfo {
                model_impl_vtable: vtable_ptr,
                model_type_info: TypeInfoCloneable::new::<T>(),
            },
        );

        let old = self
            .voxel_model_type_names
            .insert(T::NAME.to_owned(), type_id);
        assert!(
            old.is_none(),
            "{} voxel model type has a duplicate VoxelModelImpl::NAME with another already registered voxel model type with a different TypeId.",
            std::any::type_name::<T>()
        );
    }

    //pub fn set_voxel_model_asset_path(
    //    &mut self,
    //    voxel_model_id: VoxelModelId,
    //    asset_path: Option<AssetPath>,
    //) {
    //    self.voxel_model_info
    //        .get_mut(voxel_model_id.handle)
    //        .unwrap()
    //        .asset_path = asset_path;
    //}

    //pub fn peek_next_id(&self) -> VoxelModelId {
    //    VoxelModelId::new(self.voxel_model_info.next_free_handle())
    //}

    pub fn flush_out_events(
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut events: ResMut<Events>,
    ) {
    }

    pub fn set_model_asset_path(
        &mut self,
        voxel_model_id: VoxelModelId,
        asset_path: Option<GameAssetPath>,
    ) {
        self.voxel_model_info
            .get_mut(voxel_model_id.handle)
            .expect("Given id doesn't exist.")
            .asset_path = asset_path;
    }

    pub fn load_asset_model(&mut self, asset_path: &GameAssetPath) {
        assert!(
            !self.static_asset_models.contains_key(asset_path),
            "Should check if the asset model is loaded already."
        );
        if self.loading_static_model_handles.contains_key(asset_path) {
            return;
        }
        self.to_load_static_asset_models.insert(asset_path.clone());
    }

    pub fn clone_model(&mut self, voxel_model_id: VoxelModelId) -> VoxelModelId {
        let info = self
            .voxel_model_info
            .get(voxel_model_id.handle)
            .expect("Given id doesn't exist.");
        let type_id = info.model_type_id;
        let type_info = self
            .voxel_model_types
            .get(&type_id)
            .expect("Given id doesn't exist since its type id doesnt exist in the registry.");
        let dyn_vec = self
            .voxel_model_data
            .get_mut(&type_id)
            .expect("Given id doesn't exist since its type id doesnt exist in the data vec.");
        let data = dyn_vec.get_bytes(info.index as usize).as_ptr();

        // Safety: We use the same type_id to index into the type info and the dyn vec so data
        // should be the expected type.
        let new_model_ptr = unsafe { type_info.model_type_info.clone_data(data) };
        let index = dyn_vec.len() as u64;
        unsafe { dyn_vec.push_unchecked(new_model_ptr) };

        let voxel_id = self.voxel_model_info.push(VoxelModelInfo {
            model_type_id: type_id,
            index,
            asset_path: info.asset_path.clone(),
        });
        VoxelModelId::new(voxel_id)
    }

    pub fn handle_model_load_events(
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        events: Res<Events>,
        mut assets: ResMut<Assets>,
        mut ecs_world: ResMut<ECSWorld>,
    ) {
        let Some(project_dir) = assets.project_dir().clone() else {
            log::error!("Tried loading voxel model assets before project dir is set.");
            return;
        };
        let voxel_registry = &mut voxel_registry as &mut VoxelModelRegistry;

        for to_load_asset in voxel_registry.to_load_static_asset_models.drain() {
            assert!(
                !voxel_registry
                    .loading_static_model_handles
                    .contains_key(&to_load_asset),
                "Should only request load of an asset once."
            );
            let asset_path = to_load_asset.as_file_asset_path(&project_dir);
            let asset_handle = assets.load_asset::<RVOXAsset>(asset_path);
            voxel_registry
                .loading_static_model_handles
                .insert(to_load_asset.clone(), asset_handle);
        }

        // Clone because of the use of voxel_registry later.
        // TODO: Clean up and make rust like me.
        for (asset_path, asset_handle) in &voxel_registry.loading_static_model_handles.clone() {
            match assets.get_asset_status(asset_handle) {
                AssetStatus::InProgress => {
                    continue;
                }
                AssetStatus::Saved => unreachable!(),
                AssetStatus::Loaded => {
                    voxel_registry
                        .loading_static_model_handles
                        .remove(asset_path);
                    // Register the loaded static voxel model asset.
                    let asset = assets
                        .take_asset::<RVOXAsset>(asset_handle)
                        .expect("Asset should exist if loaded.");
                    let voxel_model_id = voxel_registry
                        .register_voxel_model(asset.sft_compressed, Some(asset_path.clone()));
                    voxel_registry
                        .static_asset_models
                        .insert(asset_path.clone(), voxel_model_id);
                    log::debug!("Loaded static model {}.", asset_path.asset_path);
                }
                AssetStatus::NotFound => {
                    log::error!(
                        "Tried loading renderable asset at {} but it is not found.",
                        asset_path.asset_path
                    );
                }
                AssetStatus::Error(error) => {
                    log::error!(
                        "Error while loading renderable asset at {}: {}",
                        asset_path.asset_path,
                        error
                    );
                }
            }
        }
    }

    pub fn register_voxel_model<T: VoxelModelImpl>(
        &mut self,
        voxel_model: T,
        asset_path: Option<GameAssetPath>,
    ) -> VoxelModelId {
        let type_id = std::any::TypeId::of::<T>();
        let data = self
            .voxel_model_data
            .entry(type_id)
            .or_insert_with(|| DynVec::new(TypeInfo::new::<T>()));
        let index = data.len() as u64;
        data.push(voxel_model);
        let voxel_id = self.voxel_model_info.push(VoxelModelInfo {
            model_type_id: type_id,
            index,
            asset_path: asset_path.clone(),
        });
        if let Some(asset_path) = asset_path {
            self.static_asset_models
                .insert(asset_path.clone(), VoxelModelId::new(voxel_id));
        }
        VoxelModelId::new(voxel_id)
    }

    pub fn get_model<'a, T: VoxelModelImpl>(&'a self, id: VoxelModelId) -> &'a T {
        let info = self
            .voxel_model_info
            .get(id.handle)
            .expect("Given id doesn't exist.");
        let data = self
            .voxel_model_data
            .get(&info.model_type_id)
            .expect("Given id doesn't exist since its type id doesnt exist in the data vec.");
        assert!(
            data.type_info().type_id() == std::any::TypeId::of::<T>(),
            "Given id is of type {:?} but requested type is {:?}.",
            data.type_info().name(),
            std::any::type_name::<T>()
        );
        let data_ptr = data.get_bytes(info.index as usize).as_ptr();
        // Safety: We assert the type id is the same as T.
        unsafe { &*(data_ptr as *const T) }
    }

    pub fn get_model_mut<'a, T: VoxelModelImpl>(&mut self, id: VoxelModelId) -> &'a mut T {
        let info = self
            .voxel_model_info
            .get(id.handle)
            .expect("Given id doesn't exist.");
        let data = self
            .voxel_model_data
            .get_mut(&info.model_type_id)
            .expect("Given id doesn't exist since its type id doesnt exist in the data vec.");
        assert!(
            data.type_info().type_id() == std::any::TypeId::of::<T>(),
            "Given id is of type {:?} but requested type is {:?}.",
            data.type_info().name(),
            std::any::type_name::<T>()
        );
        let data_ptr = data.get_mut_bytes(info.index as usize).as_ptr();
        // Safety: We assert the type id is the same as T.
        unsafe { &mut *(data_ptr as *mut T) }
    }

    pub fn get_dyn_model<'a>(&'a self, id: VoxelModelId) -> &'a dyn VoxelModelImplMethods {
        let info = self
            .voxel_model_info
            .get(id.handle)
            .expect("Given id doesn't exist.");
        let data = self
            .voxel_model_data
            .get(&info.model_type_id)
            .expect("Given id doesn't exist since its type id doesnt exist in the data vec.");
        let data_ptr = data.get_bytes(info.index as usize).as_ptr();
        let vtable_ptr = self
            .voxel_model_types
            .get(&info.model_type_id)
            .expect("Type should exist")
            .model_impl_vtable;
        // Safety: Dyn ref is just a fat pointer with ptr to data and ptr to the vtable.
        return unsafe { std::mem::transmute((data_ptr, vtable_ptr)) };
    }

    pub fn get_dyn_model_mut<'a>(
        &'a mut self,
        id: VoxelModelId,
    ) -> &'a mut dyn VoxelModelImplMethods {
        let info = self
            .voxel_model_info
            .get(id.handle)
            .expect("Given id doesn't exist.");
        let data = self
            .voxel_model_data
            .get_mut(&info.model_type_id)
            .expect("Given id doesn't exist since its type id doesnt exist in the data vec.");
        let data_ptr = data.get_mut_bytes(info.index as usize).as_mut_ptr();
        let vtable_ptr = self
            .voxel_model_types
            .get(&info.model_type_id)
            .expect("Type should exist")
            .model_impl_vtable;
        // Safety: Dyn ref is just a fat pointer with ptr to data and ptr to the vtable.
        return unsafe { std::mem::transmute((data_ptr, vtable_ptr)) };
    }

    //pub fn renderable_models_dyn_iter(&self) -> RenderableVoxelModelIter<'_> {
    //    let archetype_iters = self
    //        .renderable_voxel_model_archtypes
    //        .iter()
    //        .map(|(type_id, (archetype, gpu_type_id))| {
    //            (
    //                *self.type_vtables.get(type_id).unwrap(),
    //                *self.type_vtables.get(gpu_type_id).unwrap(),
    //                archetype.iter(),
    //            )
    //        })
    //        .collect();
    //    RenderableVoxelModelIter {
    //        archetype_iters,
    //        current_archetype_index: 0,
    //    }
    //}

    //pub fn renderable_models_dyn_iter_mut(&mut self) -> RenderableVoxelModelIterMut<'_> {
    //    let archetype_iters_mut = self
    //        .renderable_voxel_model_archtypes
    //        .iter_mut()
    //        .map(|(type_id, (archetype, gpu_type_id))| {
    //            (
    //                *self.type_vtables.get(type_id).unwrap(),
    //                *self.type_vtables.get(gpu_type_id).unwrap(),
    //                archetype.iter_mut(),
    //            )
    //        })
    //        .collect();
    //    RenderableVoxelModelIterMut {
    //        archetype_iters_mut,
    //        current_archetype_index: 0,
    //    }
    //}
}

pub struct RenderableVoxelModelIter<'a> {
    archetype_iters: Vec<(*mut (), *mut (), ArchetypeIter<'a>)>,
    current_archetype_index: usize,
}

impl<'a> std::iter::Iterator for RenderableVoxelModelIter<'a> {
    type Item = (
        VoxelModelId,
        (
            &'a dyn VoxelModelImplMethods,
            &'a dyn VoxelModelGpuImplMethods,
        ),
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
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelImplMethods;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
        };

        let voxel_model_gpu_ref = {
            let fat_ptr = (ptrs[1].1, *curr_model_gpu_vtable);
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelGpuImplMethods;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
        };

        Some((
            VoxelModelId {
                handle: global_id.as_typed(),
            },
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
        (
            &'a mut dyn VoxelModelImplMethods,
            &'a mut dyn VoxelModelGpuImplMethods,
        ),
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
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelImplMethods;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
        };

        let voxel_model_gpu_ref = {
            let fat_ptr = (ptrs[1].1, *curr_model_gpu_vtable);
            let mut dyn_ptr_ptr =
                std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelGpuImplMethods;

            // TODO: Write why this is safe.
            unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
        };

        Some((
            VoxelModelId {
                handle: global_id.as_typed(),
            },
            (voxel_model_ref, voxel_model_gpu_ref),
        ))
    }
}
