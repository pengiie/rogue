use std::{any::TypeId, collections::HashMap, ptr::NonNull};

use crate::entity::ecs_world::{ECSWorld, Entity};
use crate::physics::capsule_collider::CapsuleCollider;
use crate::physics::collider_voxel_registry::{self, VoxelColliderRegistry};
use crate::physics::voxel_collider::VoxelModelCollider;
use crate::physics::{
    box_collider::{self, BoxCollider},
    collider::{
        Collider, ColliderDeserializeFnPtr, ColliderIntersectionTest,
        ColliderIntersectionTestCaller, ColliderMethods, ContactManifold,
    },
    collider_component::EntityColliders,
    transform::Transform,
};
use crate::world::terrain::region_map::RegionPos;
use crate::common::{
    dyn_vec::{DynVecCloneable, TypeInfoCloneable},
    vtable,
};
use nalgebra::Vector3;

// The Collider::NAME lexographically sorted so a < b.
#[derive(Hash, PartialEq, Eq)]
struct ColliderIntersectionPair {
    a: TypeId,
    b: TypeId,
}

impl ColliderIntersectionPair {
    pub fn new(mut collider_a: TypeId, mut collider_b: TypeId) -> Self {
        // Swap to ensure name A is less than name B lexographically.
        if collider_a.cmp(&collider_b) != std::cmp::Ordering::Less {
            std::mem::swap(&mut collider_a, &mut collider_b);
        }
        Self {
            a: collider_a,
            b: collider_b,
        }
    }
}

type ColliderMethodsVtablePtr = *const ();

// Spatial hashmap binning colliders per region.
pub struct ColliderRegistry {
    pub bins: HashMap</*region_pos*/ Vector3<i32>, Vec<(Entity, ColliderId)>>,

    pub colliders: HashMap<TypeId, DynVecCloneable>,

    collider_vtables: HashMap<TypeId, ColliderMethodsVtablePtr>,
    pub collider_deserialize_fns: HashMap<TypeId, ColliderDeserializeFnPtr>,
    pub collider_type_info: HashMap</*Collider::NAME*/ String, TypeInfoCloneable>,
    pub collider_names: HashMap<TypeId, /*Collider::NAME*/ String>,

    intersection_functions: HashMap<ColliderIntersectionPair, ColliderIntersectionTestCaller>,

    pub voxel_collider_registry: VoxelColliderRegistry,
}

impl ColliderRegistry {
    pub fn new() -> Self {
        let mut reg = Self {
            bins: HashMap::new(),
            colliders: HashMap::new(),
            collider_vtables: HashMap::new(),
            collider_deserialize_fns: HashMap::new(),
            collider_type_info: HashMap::new(),
            collider_names: HashMap::new(),
            intersection_functions: HashMap::new(),

            voxel_collider_registry: VoxelColliderRegistry::new(),
        };

        reg.register_collider_type::<BoxCollider>();
        reg.register_collider_type::<CapsuleCollider>();
        reg.register_collider_type::<VoxelModelCollider>();
        reg.register_collider_intersection_fn::<BoxCollider, BoxCollider, _, _>(
            box_collider::test_intersection_box_box,
        );
        reg.register_collider_intersection_fn::<VoxelModelCollider, VoxelModelCollider, _, _>(
            collider_voxel_registry::test_intersection_voxel_voxel,
        );

        reg
    }

    fn register_collider_intersection_fn<
        A: Collider,
        B: Collider,
        F: ColliderIntersectionTest<Marker> + 'static,
        Marker,
    >(
        &mut self,
        func: F,
    ) {
        let pair =
            ColliderIntersectionPair::new(std::any::TypeId::of::<A>(), std::any::TypeId::of::<B>());
        let old = self
            .intersection_functions
            .insert(pair, ColliderIntersectionTestCaller::new(func));
        assert!(
            old.is_none(),
            "Already register intersection function for colliders {} and {}",
            std::any::type_name::<A>(),
            std::any::type_name::<B>()
        );
    }

    fn register_collider_type<C: Collider + 'static>(&mut self) {
        let type_id = std::any::TypeId::of::<C>();
        // Technically there can be two different vtable ptrs for the same type due to something
        // about codegen units, but that doesn't matter here since semantically there is no
        // difference so ignore duplicates.
        if self.collider_vtables.contains_key(&type_id) {
            return;
        }

        // Safety: We never access the contents of the pointer, only extracting the vtable, so
        // should be okay right? Use `without_provenance_mut` since this ptr isn't actually
        // associated with a memory allocation.
        let null = unsafe { NonNull::new_unchecked(std::ptr::without_provenance_mut::<C>(0x1234)) };
        let dyn_ref = unsafe { null.as_ref() } as &dyn ColliderMethods;
        // Safety: This reference is in fact a dyn ref.
        let vtable_ptr = unsafe { vtable::get_vtable_ptr(dyn_ref as &dyn ColliderMethods) };
        self.collider_vtables.insert(type_id, vtable_ptr);
        let de_f = C::deserialize_collider;
        self.collider_deserialize_fns.insert(type_id, de_f);

        let old = self
            .collider_type_info
            .insert(C::NAME.to_owned(), TypeInfoCloneable::new::<C>());
        assert!(
            old.is_none(),
            "{} collider has a duplicate Collider::NAME with another already registered component.",
            std::any::type_name::<C>()
        );
        self.collider_names.insert(type_id, C::NAME.to_owned());
    }

    pub fn register_collider<C: Collider>(&mut self, collider: C) -> ColliderId {
        let type_id = std::any::TypeId::of::<C>();
        if !self.collider_vtables.contains_key(&type_id) {
            panic!(
                "Tried to register collider of type `{}` which has not been registered within the ColliderRegisty.",
                std::any::type_name::<C>()
            );
        }

        let vec = self
            .colliders
            .entry(type_id)
            .or_insert(DynVecCloneable::new(TypeInfoCloneable::new::<C>()));
        let index = vec.len();
        vec.push(collider);
        return ColliderId {
            collider_type: type_id,
            index,
        };
    }

    /// Takes ownership of src_data. Type info must be a valid registered collider.
    /// Safety: src_data must be allocated with the same alignment and size as the provided type
    /// info. src_data is also taken ownership of.
    pub unsafe fn register_collider_raw(
        &mut self,
        type_info_cloneable: &TypeInfoCloneable,
        src_data: *mut u8,
    ) -> ColliderId {
        let type_id = type_info_cloneable.type_id();
        if !self.collider_vtables.contains_key(&type_id) {
            panic!(
                "Tried to register collider of type `{:?}` which has not been registered within the ColliderRegisty.",
                type_info_cloneable.type_id()
            );
        }

        let vec = self
            .colliders
            .entry(type_id)
            .or_insert(DynVecCloneable::new(type_info_cloneable.clone()));
        let index = vec.len();
        vec.push_unchecked(std::slice::from_raw_parts(
            src_data as *const u8,
            type_info_cloneable.size(),
        ));
        return ColliderId {
            collider_type: type_id,
            index,
        };
    }

    pub fn contains_id(&self, collider_id: &ColliderId) -> bool {
        let Some(colliders) = self.colliders.get(&collider_id.collider_type) else {
            return false;
        };

        return collider_id.index < colliders.len();
    }

    pub fn update_entity_collider_positions(&mut self, ecs_world: &mut ECSWorld) {
        // Clear all the bins and then populate each one with the colliders.
        // This is trading off so we do O(2n) here so we don't do O(n^2) during collision
        // detection. TODO: Benchmark and also don't clear entire bin each time and figure out how
        // to selectively modify colliders.
        self.bins.clear();
        for (entity, (transform, colliders)) in ecs_world
            .query::<(&Transform, &EntityColliders)>()
            .into_iter()
        {
            let world_transform = ecs_world.get_world_transform(entity, transform);
            for collider_id in &colliders.colliders {
                let Some(aabb) = self
                    .get_collider_dyn(collider_id)
                    .aabb(&world_transform, &self.voxel_collider_registry)
                else {
                    // Collider isn't ready to be used yet.
                    return;
                };
                let region_min = RegionPos::from_world_pos(&aabb.min);
                let region_max = RegionPos::from_world_pos(&aabb.max);
                for region_x in region_min.x..=region_max.x {
                    for region_y in region_min.y..=region_max.y {
                        for region_z in region_min.z..=region_max.z {
                            self.bins
                                .entry(Vector3::new(region_x, region_y, region_z))
                                .or_default()
                                .push((entity, collider_id.clone()));
                        }
                    }
                }
            }
        }
    }

    pub fn clone_collider(&mut self, collider_id: &ColliderId) -> ColliderId {
        let vec = self
            .colliders
            .get_mut(&collider_id.collider_type)
            .expect("Collider id is invalid");
        let new_index = vec.clone_element(collider_id.index);

        return ColliderId {
            collider_type: collider_id.collider_type,
            index: new_index,
        };
    }

    pub fn test_narrow_phase(
        &self,
        collider_id_a: &ColliderId,
        collider_id_b: &ColliderId,
        entity_transform_a: &Transform,
        entity_transform_b: &Transform,
    ) -> Option<ContactManifold> {
        let pair =
            ColliderIntersectionPair::new(collider_id_a.collider_type, collider_id_b.collider_type);
        let Some(func) = self.intersection_functions.get(&pair) else {
            return None;
        };

        func.run_erased(
            collider_id_a,
            collider_id_b,
            entity_transform_a,
            entity_transform_b,
            self,
        )
    }

    pub fn get_collider_dyn(&self, collider_id: &ColliderId) -> &dyn ColliderMethods {
        let collider = self
            .colliders
            .get(&collider_id.collider_type)
            .unwrap()
            .get_unchecked(collider_id.index)
            .as_ptr() as *const ();
        let dyn_ref = {
            let vtable_ptr = *self
                .collider_vtables
                .get(&collider_id.collider_type)
                .unwrap();
            let dyn_fat_ptr = unsafe {
                std::mem::transmute::<(*const (), *const ()), *const dyn ColliderMethods>((
                    collider, vtable_ptr,
                ))
            };
            unsafe { dyn_fat_ptr.as_ref() }.unwrap()
        };
        dyn_ref
    }

    pub fn get_collider_dyn_mut(&mut self, collider_id: &ColliderId) -> &mut dyn ColliderMethods {
        let collider = self
            .colliders
            .get_mut(&collider_id.collider_type)
            .unwrap()
            .get_mut_unchecked(collider_id.index)
            .as_ptr() as *mut ();
        let dyn_ref = {
            let vtable_ptr = *self
                .collider_vtables
                .get(&collider_id.collider_type)
                .unwrap();
            let dyn_fat_ptr = unsafe {
                std::mem::transmute::<(*mut (), *const ()), *mut dyn ColliderMethods>((
                    collider, vtable_ptr,
                ))
            };
            unsafe { dyn_fat_ptr.as_mut() }.unwrap()
        };
        dyn_ref
    }

    pub fn get_collider<T: Collider>(&self, collider_id: &ColliderId) -> &T {
        let collider = self
            .colliders
            .get(&collider_id.collider_type)
            .unwrap()
            .get_unchecked(collider_id.index)
            .as_ptr() as *const T;
        assert_eq!(collider_id.collider_type, std::any::TypeId::of::<T>());
        return unsafe { &*collider };
    }

    pub fn get_collider_mut<T: ColliderMethods + Collider + 'static>(
        &mut self,
        collider_id: &ColliderId,
    ) -> &mut T {
        let collider = self
            .colliders
            .get(&collider_id.collider_type)
            .unwrap()
            .get_unchecked(collider_id.index)
            .as_ptr() as *mut T;
        assert_eq!(collider_id.collider_type, std::any::TypeId::of::<T>());
        return unsafe { &mut *collider };
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct ColliderId {
    pub collider_type: TypeId,
    pub index: usize,
}

impl ColliderId {
    pub const fn null() -> ColliderId {
        ColliderId {
            collider_type: TypeId::of::<()>(),
            index: 0,
        }
    }

    fn is_null(&self) -> bool {
        self.collider_type == TypeId::of::<()>()
    }
}
