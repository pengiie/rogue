use std::{
    any::TypeId,
    collections::HashMap,
    ops::{Deref, DerefMut},
    path::Path,
};

use rogue_macros::Resource;

use crate::common::{
    archetype::{Archetype, ArchetypeIter, ArchetypeIterMut},
    dyn_vec::{DynVec, TypeInfo, TypeInfoCloneable},
    freelist::{FreeList, FreeListHandle},
};
use crate::asset::{
    asset::{AssetHandle, AssetPath, AssetStatus, Assets, GameAssetPath},
    repr::voxel::any::VoxelModelAsset,
};
use crate::entity::{
    ecs_world::{ECSWorld, Entity},
    RenderableVoxelEntity,
};
use crate::event::{EventReader, Events};
use crate::resource::{Res, ResMut};
use crate::voxel::{
    voxel_allocator::VoxelDataAllocator, voxel_events::EventVoxelRenderableEntityLoad,
};
use super::{
    flat::{VoxelModelFlat, VoxelModelFlatGpu},
    sft::VoxelModelSFT,
    sft_compressed::VoxelModelSFTCompressed,
    sft_compressed_gpu::VoxelModelSFTCompressedGpu,
    sft_gpu::VoxelModelSFTGpu,
    thc::{VoxelModelTHC, VoxelModelTHCCompressed, VoxelModelTHCCompressedGpu, VoxelModelTHCGpu},
    voxel::{VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl, VoxelModelImplMethods},
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

#[derive(Clone, Eq, PartialEq)]
pub struct VoxelModelInfo {
    pub name: String,
    pub model_type_id: std::any::TypeId,
    /// The index within the DynVec the model is assigned to.
    index: u64,
    /// For asset based models.
    pub asset_path: Option<GameAssetPath>,
}

pub struct VoxelModelTypeInfo {
    // Vtable to VoxelModelImplMethods.
    model_impl_vtable: *mut (),
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
    pub voxel_model_data: HashMap<TypeId, DynVec>,
    pub voxel_model_info: FreeList<VoxelModelInfo>,

    /// Models which are tied to a specific project asset and are not destructible.
    /// Essentially allows for caching and reuse of model data between entities.
    pub static_asset_models: HashMap<GameAssetPath, VoxelModelId>,
    model_load_event_reader: EventReader<EventVoxelRenderableEntityLoad>,
    loading_static_model_handles: HashMap<GameAssetPath, AssetHandle>,
    loading_renderable_entities: HashMap<GameAssetPath, Vec<Entity>>,
    ///// Non-terrain voxel models that need their normals updated.
    //pub to_update_model_normals: Vec<VoxelModelId>,
}

impl VoxelModelRegistry {
    pub fn new() -> Self {
        let mut s = Self {
            voxel_model_types: HashMap::new(),
            voxel_model_data: HashMap::new(),
            voxel_model_info: FreeList::new(),

            static_asset_models: HashMap::new(),
            model_load_event_reader: EventReader::new(),
            loading_static_model_handles: HashMap::new(),
            loading_renderable_entities: HashMap::new(),
        };

        s.register_voxel_model_type::<VoxelModelFlat>();
        s.register_voxel_model_type::<VoxelModelSFT>();
        s.register_voxel_model_type::<VoxelModelSFTCompressed>();

        s
    }

    pub fn register_voxel_model_type<T: VoxelModelImpl>(&mut self) {
        todo!();
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

    pub fn handle_model_load_events(
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        events: Res<Events>,
        mut assets: ResMut<Assets>,
        mut ecs_world: ResMut<ECSWorld>,
    ) {
        let voxel_registry = &mut voxel_registry as &mut VoxelModelRegistry;

        // Look for any renderable load requests.
        for event in voxel_registry.model_load_event_reader.read(&events) {
            let Ok(mut renderable) = ecs_world.get::<&mut RenderableVoxelEntity>(event.entity)
            else {
                // Ignore events where the renderable doesn't exist.
                continue;
            };
            let Some(model_asset_path) = renderable.model_asset_path() else {
                log::error!("Should not be sending EventVoxelRenderableEntityLoad for a renderable entity without an asset path.");
                continue;
            };
            // If we don't worry about force reloading then use a cached model.
            if !event.reload {
                if let Some(model_id) = voxel_registry.static_asset_models.get(model_asset_path) {
                    if renderable.is_dynamic() {
                        todo!("clone then create new model");
                    } else {
                        renderable.set_model_id(*model_id);
                    }
                    continue;
                }
            }

            // Enqueue this entity to be loaded when the model is loaded.
            voxel_registry
                .loading_renderable_entities
                .entry(model_asset_path.clone())
                .or_default()
                .push(event.entity);

            // Start loading this static model if it isn't already loading.
            if !voxel_registry
                .loading_static_model_handles
                .contains_key(model_asset_path)
            {
                let model_asset_handle = assets.load_asset::<VoxelModelAsset>(todo!());
                voxel_registry
                    .loading_static_model_handles
                    .insert(model_asset_path.clone(), model_asset_handle);
            }
        }

        // Check on the state of any loading static voxel model assets.
        let mut finished_asset_paths = Vec::new();
        // Clone because of the use of voxel_registry later.
        // TODO: Clean up and make rust like me.
        for (asset_path, asset_handle) in &voxel_registry.loading_static_model_handles.clone() {
            match assets.get_asset_status(asset_handle) {
                AssetStatus::InProgress => {
                    continue;
                }
                AssetStatus::Saved => unreachable!(),
                AssetStatus::Loaded => {
                    // Register the loaded static voxel model asset.
                    let asset = assets.take_asset::<VoxelModelAsset>(asset_handle).unwrap();
                    let voxel_model_id = voxel_registry.register_voxel_model_asset(*asset);
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
            finished_asset_paths.push(asset_path.clone());
        }

        // Update any waiting RenderableEntity components with the loaded static model asset.
        for asset_path in finished_asset_paths {
            voxel_registry
                .loading_static_model_handles
                .remove(&asset_path);
            let Some(entities) = voxel_registry
                .loading_renderable_entities
                .get_mut(&asset_path)
            else {
                continue;
            };
            let Some(static_model_id) = voxel_registry.static_asset_models.get(&asset_path) else {
                // Model must have failed to load for some reason.
                entities.clear();
                continue;
            };
            for entity in entities.drain(..) {
                let Ok(mut renderable) = ecs_world.get::<&mut RenderableVoxelEntity>(entity) else {
                    // Ignore any entities which no longer have a renderable component.
                    continue;
                };
                if renderable.is_dynamic() {
                    todo!("Figure out dynamic loading and stuff.");
                } else {
                    renderable.set_model(Some(asset_path.clone()), *static_model_id);
                }
            }
        }
    }

    ///// Noop if model is already unloaded or doesn't exist.
    //pub fn unload_model(&mut self, id: VoxelModelId, voxel_allocator: &mut VoxelDataAllocator) {
    //    let info_handle = id.handle;
    //    let Some(info) = self.voxel_model_info.get(info_handle) else {
    //        return;
    //    };
    //    if info.gpu_type.is_some() {
    //        let mut dynegpu = self.get_dyn_gpu_model_mut(id);
    //        dyn_gpu.deallocate(voxel_allocator);

    //        let info = self.voxel_model_info.get(info_handle).unwrap();
    //        self.renderable_voxel_model_archtypes
    //            .get_mut(&info.model_type_id)
    //            .unwrap()
    //            .0
    //            .remove(info.archetype_index);
    //    } else {
    //        self.standalone_voxel_model_archtypes
    //            .get_mut(&info.model_type_id)
    //            .unwrap()
    //            .remove(info.archetype_index);
    //    }
    //    self.voxel_model_info.remove(info_handle);
    //}

    //fn convert_model<T: VoxelModelImplConcrete, C: VoxelModelImplConcrete + for<'a> From<&'a T>>(
    //    &mut self,
    //    renderable_voxel_model: &mut RenderableVoxelEntity,
    //    info: &VoxelModelInfo,
    //    original_id: VoxelModelId,
    //) {
    //    //let converted_model = C::from(voxel_world.registry.get_model::<T>(original_id));
    //    //let converted_model_id = voxel_world
    //    //    .registry
    //    //    .register_renderable_voxel_model(&info.name, VoxelModel::new(converted_model));
    //    //voxel_world
    //    //    .registry
    //    //    .set_voxel_model_asset_path(converted_model_id, info.asset_path.clone());
    //    //renderable_voxel_model.set_model(converted_model_id);

    //    ////voxel_world.to_update_normals.insert(converted_model_id);
    //}
    //

    pub fn register_voxel_model<T: VoxelModelImpl>(&mut self, voxel_model: T) -> VoxelModelId {
        todo!()
    }

    pub fn register_voxel_model_asset(&mut self, asset: VoxelModelAsset) -> VoxelModelId {
        todo!()
    }

    //// TODO: Add more methods to the Impl so we don't have like 50 match statements.
    //pub fn register_renderable_voxel_model_any(
    //    &mut self,
    //    name: impl ToString,
    //    voxel_model_any: VoxelModelAnyAsset,
    //) -> VoxelModelId {
    //    let voxel_model_gpu: Box<dyn VoxelModelGpuImpl> = match voxel_model_any.model_type {
    //        VoxelModelType::Flat => Box::new(VoxelModelFlatGpu::new()),
    //        VoxelModelType::THC => Box::new(VoxelModelTHCGpu::new()),
    //        VoxelModelType::THCCompressed => Box::new(VoxelModelTHCCompressedGpu::new()),
    //        VoxelModelType::SFT => Box::new(VoxelModelSFTGpu::new()),
    //        VoxelModelType::SFTCompressed => Box::new(VoxelModelSFTCompressedGpu::new()),
    //    };
    //    let model_type_info = match voxel_model_any.model_type {
    //        VoxelModelType::Flat => TypeInfoCloneable::new::<VoxelModelFlat>(),
    //        VoxelModelType::THC => TypeInfoCloneable::new::<VoxelModelTHC>(),
    //        VoxelModelType::THCCompressed => TypeInfoCloneable::new::<VoxelModelTHCCompressed>(),
    //        VoxelModelType::SFT => TypeInfoCloneable::new::<VoxelModelSFT>(),
    //        VoxelModelType::SFTCompressed => TypeInfoCloneable::new::<VoxelModelSFTCompressed>(),
    //    };
    //    let gpu_type_info = match voxel_model_any.model_type {
    //        VoxelModelType::Flat => TypeInfoCloneable::new::<VoxelModelFlatGpu>(),
    //        VoxelModelType::THC => TypeInfoCloneable::new::<VoxelModelTHCGpu>(),
    //        VoxelModelType::THCCompressed => TypeInfoCloneable::new::<VoxelModelTHCCompressedGpu>(),
    //        VoxelModelType::SFT => TypeInfoCloneable::new::<VoxelModelSFTGpu>(),
    //        VoxelModelType::SFTCompressed => TypeInfoCloneable::new::<VoxelModelSFTCompressedGpu>(),
    //    };
    //    let id = self.peek_next_id();

    //    // Extract fat pointers for this voxel model T's implementation of VoxelModelImpl and
    //    // T::Gpu's VoxelModelGpuImpl.
    //    let voxel_model_vtable_ptr = {
    //        let dyn_ref/*: (*mut (), *mut ())*/ = voxel_model_any.model.deref() as &dyn VoxelModelImpl;
    //        let fat_ptr = std::ptr::from_ref(&dyn_ref) as *const _ as *const (*mut (), *mut ());
    //        // Safety: We know &dyn T aka. fat_ptr is a fat pointer containing two pointers.
    //        unsafe { fat_ptr.as_ref() }.unwrap().1
    //    };
    //    let voxel_model_gpu_vtable_ptr = {
    //        let dyn_ref/*: (*mut (), *mut ())*/ = voxel_model_gpu.deref() as &dyn VoxelModelGpuImpl;
    //        let fat_ptr = std::ptr::from_ref(&dyn_ref) as *const _ as *const (*mut (), *mut ());
    //        // Safety: We know &dyn T aka. fat_ptr is a fat pointer containing two pointers.
    //        unsafe { fat_ptr.as_ref() }.unwrap().1
    //    };

    //    self.type_vtables
    //        .insert(model_type_info.type_id(), voxel_model_vtable_ptr);
    //    self.type_vtables
    //        .insert(gpu_type_info.type_id(), voxel_model_gpu_vtable_ptr);

    //    let (ref mut archetype, _) = self
    //        .renderable_voxel_model_archtypes
    //        .entry(model_type_info.type_id())
    //        .or_insert_with(|| {
    //            (
    //                Archetype::new(vec![model_type_info, gpu_type_info]),
    //                gpu_type_info.type_id(),
    //            )
    //        });

    //    let archetype_index = match voxel_model_any.model_type {
    //        VoxelModelType::Flat => archetype.insert(
    //            id.handle.as_untyped(),
    //            (
    //                *voxel_model_any.model.downcast::<VoxelModelFlat>().unwrap(),
    //                *voxel_model_gpu.downcast::<VoxelModelFlatGpu>().unwrap(),
    //            ),
    //        ),
    //        VoxelModelType::THC => archetype.insert(
    //            id.handle.as_untyped(),
    //            (
    //                *voxel_model_any.model.downcast::<VoxelModelTHC>().unwrap(),
    //                *voxel_model_gpu.downcast::<VoxelModelTHCGpu>().unwrap(),
    //            ),
    //        ),
    //        VoxelModelType::THCCompressed => archetype.insert(
    //            id.handle.as_untyped(),
    //            (
    //                *voxel_model_any
    //                    .model
    //                    .downcast::<VoxelModelTHCCompressed>()
    //                    .unwrap(),
    //                *voxel_model_gpu
    //                    .downcast::<VoxelModelTHCCompressedGpu>()
    //                    .unwrap(),
    //            ),
    //        ),
    //        VoxelModelType::SFT => archetype.insert(
    //            id.handle.as_untyped(),
    //            (
    //                *voxel_model_any.model.downcast::<VoxelModelSFT>().unwrap(),
    //                *voxel_model_gpu.downcast::<VoxelModelSFTGpu>().unwrap(),
    //            ),
    //        ),
    //        VoxelModelType::SFTCompressed => archetype.insert(
    //            id.handle.as_untyped(),
    //            (
    //                *voxel_model_any
    //                    .model
    //                    .downcast::<VoxelModelSFTCompressed>()
    //                    .unwrap(),
    //                *voxel_model_gpu
    //                    .downcast::<VoxelModelSFTCompressedGpu>()
    //                    .unwrap(),
    //            ),
    //        ),
    //    };

    //    let info = VoxelModelInfo {
    //        name: name.to_string(),
    //        model_type: Some(voxel_model_any.model_type),
    //        model_type_id: model_type_info.type_id(),
    //        gpu_type: Some(gpu_type_info.type_id()),
    //        archetype_index,
    //        asset_path: None,
    //    };
    //    self.voxel_model_info.push(info);

    //    id
    //}

    //pub fn register_renderable_voxel_model<T>(
    //    &mut self,
    //    name: impl ToString,
    //    voxel_model: VoxelModel<T>,
    //) -> VoxelModelId
    //where
    //    T: VoxelModelImplConcrete,
    //{
    //    let voxel_model_gpu = VoxelModelGpu::new(T::Gpu::new());
    //    let model_type_info = TypeInfoCloneable::new::<T>();
    //    let gpu_type_info = TypeInfoCloneable::new::<T::Gpu>();
    //    let id = self.peek_next_id();

    //    // Extract fat pointers for this voxel model T's implementation of VoxelModelImpl and
    //    // T::Gpu's VoxelModelGpuImpl.
    //    let voxel_model_vtable_ptr = {
    //        let dyn_ref/*: (*mut (), *mut ())*/ = voxel_model.deref() as &dyn VoxelModelImpl;
    //        let fat_ptr = std::ptr::from_ref(&dyn_ref) as *const _ as *const (*mut (), *mut ());
    //        // Safety: We know &dyn T aka. fat_ptr is a fat pointer containing two pointers.
    //        unsafe { fat_ptr.as_ref() }.unwrap().1
    //    };
    //    let voxel_model_gpu_vtable_ptr = {
    //        let dyn_ref/*: (*mut (), *mut ())*/ = voxel_model_gpu.deref() as &dyn VoxelModelGpuImpl;
    //        let fat_ptr = std::ptr::from_ref(&dyn_ref) as *const _ as *const (*mut (), *mut ());
    //        // Safety: We know &dyn T aka. fat_ptr is a fat pointer containing two pointers.
    //        unsafe { fat_ptr.as_ref() }.unwrap().1
    //    };

    //    self.type_vtables
    //        .insert(model_type_info.type_id(), voxel_model_vtable_ptr);
    //    self.type_vtables
    //        .insert(gpu_type_info.type_id(), voxel_model_gpu_vtable_ptr);

    //    let (ref mut archetype, _) = self
    //        .renderable_voxel_model_archtypes
    //        .entry(model_type_info.type_id())
    //        .or_insert_with(|| {
    //            (
    //                Archetype::new(vec![model_type_info, gpu_type_info]),
    //                gpu_type_info.type_id(),
    //            )
    //        });
    //    let archetype_index = archetype.insert(
    //        id.handle.as_untyped(),
    //        (voxel_model.into_model(), voxel_model_gpu.into_model_gpu()),
    //    );

    //    let info = VoxelModelInfo {
    //        name: name.to_string(),
    //        model_type: T::model_type(),
    //        model_type_id: model_type_info.type_id(),
    //        gpu_type: Some(gpu_type_info.type_id()),
    //        archetype_index,
    //        asset_path: None,
    //    };
    //    self.voxel_model_info.push(info);

    //    id
    //}

    pub fn get_dyn_model<'a>(&'a self, id: VoxelModelId) -> &'a dyn VoxelModelImplMethods {
        todo!()
    }

    pub fn get_dyn_model_mut<'a>(
        &'a mut self,
        id: VoxelModelId,
    ) -> &'a mut dyn VoxelModelImplMethods {
        todo!()
    }
    //    let model_info = self
    //        .voxel_model_info
    //        .get(id.handle)
    //        .expect("Voxel model id is invalid");

    //    let archetype = if model_info.gpu_type.is_none() {
    //        todo!("Fetch from standalone archetype");
    //    } else {
    //        &self
    //            .renderable_voxel_model_archtypes
    //            .get(&model_info.model_type_id)
    //            .unwrap()
    //            .0
    //    };

    //    let model_type_info = &archetype.type_infos()[0];
    //    assert_eq!(model_type_info.type_id(), model_info.model_type_id);

    //    // Safety: If model_info is still valid, then the archetype_index it contains must best
    //    // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
    //    // reference to self and explicity the lifetime.
    //    let model_ptr =
    //        unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *const u8 };

    //    let voxel_model_dyn_ref = {
    //        let model_vtable = *self.type_vtables.get(&model_info.model_type_id).unwrap();
    //        let fat_ptr = (model_ptr, model_vtable);
    //        let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelImpl;

    //        // TODO: Write why this is safe.
    //        unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
    //    };

    //    voxel_model_dyn_ref
    //}

    //pub fn get_model<'a, T: VoxelModelImpl>(&'a self, id: VoxelModelId) -> &'a T {
    //    let model_info = self
    //        .voxel_model_info
    //        .get(id.handle)
    //        .expect("Voxel model id is invalid");

    //    let archetype = if model_info.gpu_type.is_none() {
    //        todo!("Fetch from standalone archetype");
    //    } else {
    //        &self
    //            .renderable_voxel_model_archtypes
    //            .get(&model_info.model_type_id)
    //            .unwrap()
    //            .0
    //    };

    //    let model_type_info = &archetype.type_infos()[0];
    //    assert_eq!(model_type_info.type_id(), model_info.model_type_id);
    //    assert_eq!(model_type_info.type_id(), std::any::TypeId::of::<T>());

    //    // Safety: If model_info is still valid, then the archetype_index it contains must best
    //    // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
    //    // reference to self and explicity the lifetime.
    //    let model_ptr =
    //        unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *const u8 };

    //    // Safety: We asset above the type id matches.
    //    unsafe { (model_ptr as *const T).as_ref().unwrap() }
    //}

    //pub fn get_dyn_model_mut<'a>(&'a mut self, id: VoxelModelId) -> &'a mut dyn VoxelModelImpl {
    //    let model_info = self
    //        .voxel_model_info
    //        .get(id.handle)
    //        .expect("Voxel model id is invalid");

    //    let archetype = if model_info.gpu_type.is_none() {
    //        todo!("Fetch from standalone archetype");
    //    } else {
    //        &mut self
    //            .renderable_voxel_model_archtypes
    //            .get_mut(&model_info.model_type_id)
    //            .unwrap()
    //            .0
    //    };

    //    let model_type_info = &archetype.type_infos()[0];
    //    assert_eq!(model_type_info.type_id(), model_info.model_type_id);

    //    // Safety: If model_info is still valid, then the archetype_index it contains must best
    //    // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
    //    // reference to self and explicity the lifetime.
    //    let model_ptr =
    //        unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *mut u8 };

    //    let voxel_model_dyn_ref = {
    //        let model_vtable = *self.type_vtables.get(&model_info.model_type_id).unwrap();
    //        let fat_ptr = (model_ptr, model_vtable);
    //        let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelImpl;

    //        // TODO: Write why this is safe.
    //        unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
    //    };

    //    voxel_model_dyn_ref
    //}

    //pub fn get_dyn_renderable_model<'a>(
    //    &'a self,
    //    id: VoxelModelId,
    //) -> (&'a dyn VoxelModelImpl, &'a dyn VoxelModelGpuImpl) {
    //    let model_info = self
    //        .voxel_model_info
    //        .get(id.handle)
    //        .expect("Voxel model id is invalid");

    //    let archetype = if model_info.gpu_type.is_none() {
    //        todo!("Fetch from standalone archetype");
    //    } else {
    //        &self
    //            .renderable_voxel_model_archtypes
    //            .get(&model_info.model_type_id)
    //            .unwrap()
    //            .0
    //    };

    //    let model_type_info = &archetype.type_infos()[0];
    //    assert_eq!(model_type_info.type_id(), model_info.model_type_id);
    //    let model_gpu_type_info = &archetype.type_infos()[1];
    //    assert_eq!(model_gpu_type_info.type_id(), model_info.gpu_type.unwrap());

    //    // Safety: If model_info is still valid, then the archetype_index it contains must best
    //    // valid to the archetype. We can also cast to to a *mut ptr since we take a mutable
    //    // reference to self and explicity the lifetime.
    //    let model_ptr =
    //        unsafe { archetype.get_raw(model_type_info, model_info.archetype_index) as *mut u8 };

    //    let voxel_model_dyn_ref = {
    //        let model_vtable = *self.type_vtables.get(&model_info.model_type_id).unwrap();
    //        let fat_ptr = (model_ptr, model_vtable);
    //        let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelImpl;

    //        // TODO: Write why this is safe.
    //        unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
    //    };

    //    let model_gpu_ptr = unsafe {
    //        archetype.get_raw(model_gpu_type_info, model_info.archetype_index) as *mut u8
    //    };

    //    let voxel_model_gpu_dyn_ref = {
    //        let model_gpu_vtable = *self
    //            .type_vtables
    //            .get(&model_gpu_type_info.type_id())
    //            .unwrap();
    //        let fat_ptr = (model_gpu_ptr, model_gpu_vtable);
    //        let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelGpuImpl;

    //        // TODO: Write why this is safe.
    //        unsafe { dyn_ptr_ptr.as_ref().unwrap().deref() }
    //    };

    //    (voxel_model_dyn_ref, voxel_model_gpu_dyn_ref)
    //}

    //pub fn get_dyn_gpu_model_mut<'a>(
    //    &'a mut self,
    //    id: VoxelModelId,
    //) -> &'a mut dyn VoxelModelGpuImpl {
    //    let model_info = self
    //        .voxel_model_info
    //        .get(id.handle)
    //        .expect("Voxel model id is invalid");

    //    let archetype = if model_info.gpu_type.is_none() {
    //        panic!("nope");
    //    } else {
    //        &self
    //            .renderable_voxel_model_archtypes
    //            .get(&model_info.model_type_id)
    //            .unwrap()
    //            .0
    //    };

    //    let model_type_info = &archetype.type_infos()[0];
    //    assert_eq!(model_type_info.type_id(), model_info.model_type_id);
    //    let model_gpu_type_info = &archetype.type_infos()[1];
    //    assert_eq!(model_gpu_type_info.type_id(), model_info.gpu_type.unwrap());

    //    let model_gpu_ptr = unsafe {
    //        archetype.get_raw(model_gpu_type_info, model_info.archetype_index) as *mut u8
    //    };

    //    let voxel_model_gpu_dyn_ref = {
    //        let model_gpu_vtable = *self
    //            .type_vtables
    //            .get(&model_gpu_type_info.type_id())
    //            .unwrap();
    //        let fat_ptr = (model_gpu_ptr, model_gpu_vtable);
    //        let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelGpuImpl;

    //        // TODO: Write why this is safe.
    //        unsafe { dyn_ptr_ptr.as_mut().unwrap().deref_mut() }
    //    };

    //    voxel_model_gpu_dyn_ref
    //}

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
        (&'a dyn VoxelModelImplMethods, &'a dyn VoxelModelGpuImpl),
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
            let dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *const &dyn VoxelModelGpuImpl;

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
            &'a mut dyn VoxelModelGpuImpl,
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
            let mut dyn_ptr_ptr = std::ptr::from_ref(&fat_ptr) as *mut &mut dyn VoxelModelGpuImpl;

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
