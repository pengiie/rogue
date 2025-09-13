use std::collections::HashMap;

use hecs::Without;
use nalgebra::Vector3;
use serde::ser::{SerializeMap, SerializeStruct};

use crate::{
    common::{
        dyn_vec::{DynVec, TypeInfo, TypeInfoCloneable},
        vtable,
    },
    engine::{
        asset::repr::collider::ColliderRegistryAsset,
        entity::{
            ecs_world::{ECSWorld, Entity},
            EntityParent,
        },
        physics::{
            box_collider::BoxCollider,
            capsule_collider::CapsuleCollider,
            collider::{Collider, ColliderConcrete, ColliderType, Colliders},
            transform::Transform,
        },
        voxel::terrain::chunks::VoxelChunks,
    },
};

// Spatial hashmap binning colliders per region.
pub struct ColliderRegistry {
    pub bins: HashMap</*region_pos*/ Vector3<i32>, Vec<(Entity, ColliderId)>>,
    pub colliders: HashMap<ColliderType, DynVec>,
    collider_vtables: HashMap<ColliderType, *const ()>,
}

impl ColliderRegistry {
    pub fn new() -> Self {
        Self {
            bins: HashMap::new(),
            colliders: HashMap::new(),
            collider_vtables: HashMap::new(),
        }
    }

    pub fn register_collider<C: Collider + Clone + 'static>(&mut self, collider: C) -> ColliderId {
        let collider_type = collider.collider_type();

        let collider_vtable_ptr = {
            // dyn pointer to the collider data and vtable.
            let collider_dyn = &collider as &dyn Collider;
            unsafe { vtable::get_vtable_ptr(collider_dyn) }
        };

        let vec = self
            .colliders
            .entry(collider_type)
            .or_insert(DynVec::new(TypeInfoCloneable::new::<C>()));
        let index = vec.size();
        vec.push(collider);
        return ColliderId {
            collider_type: collider_type,
            index,
        };
    }

    pub fn contains_id(&self, collider_id: &ColliderId) -> bool {
        let Some(colliders) = self.colliders.get(&collider_id.collider_type) else {
            return false;
        };

        return collider_id.index < colliders.size();
    }

    pub fn update_entity_collider_positions(&mut self, ecs_world: &mut ECSWorld) {
        // Clear all the bins and then populate each one with the colliders.
        // This is trading off so we do O(2n) here so we don't do O(n^2) during collision
        // detection. TODO: Benchmark and also don't clear entire bin each time and figure out how
        // to selectively modify colliders.
        self.bins.clear();
        for (entity, (transform, colliders)) in ecs_world
            .query_mut::<Without<(&Transform, &Colliders), &EntityParent>>()
            .into_iter()
        {
            for collider_id in &colliders.colliders {
                let aabb = self.get_collider_dyn(collider_id).aabb(transform);
                let region_min = VoxelChunks::position_to_region_pos(&aabb.min);
                let region_max = VoxelChunks::position_to_region_pos(&aabb.max);
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

    pub fn get_collider_dyn(&self, collider_id: &ColliderId) -> &dyn Collider {
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
                std::mem::transmute::<(*const (), *const ()), *const dyn Collider>((
                    collider, vtable_ptr,
                ))
            };
            unsafe { dyn_fat_ptr.as_ref() }.unwrap()
        };
        dyn_ref
    }

    pub fn get_collider<T: Collider + ColliderConcrete + 'static>(
        &self,
        collider_id: &ColliderId,
    ) -> &T {
        let collider = self
            .colliders
            .get(&collider_id.collider_type)
            .unwrap()
            .get_unchecked(collider_id.index)
            .as_ptr() as *const T;
        assert_eq!(collider_id.collider_type, T::concrete_collider_type());
        return unsafe { &*collider };
    }

    pub fn get_collider_mut<T: Collider + ColliderConcrete + 'static>(
        &mut self,
        collider_id: &ColliderId,
    ) -> &mut T {
        let collider = self
            .colliders
            .get(&collider_id.collider_type)
            .unwrap()
            .get_unchecked(collider_id.index)
            .as_ptr() as *mut T;
        assert_eq!(collider_id.collider_type, T::concrete_collider_type());
        return unsafe { &mut *collider };
    }
}

#[derive(Copy, Clone, serde::Serialize, serde::Deserialize, Hash, PartialEq, Eq)]
pub struct ColliderId {
    pub collider_type: ColliderType,
    pub index: usize,
}

impl ColliderId {
    const fn null() -> ColliderId {
        ColliderId {
            collider_type: ColliderType::Null,
            index: 0,
        }
    }

    fn is_null(&self) -> bool {
        self.collider_type == ColliderType::Null
    }
}

impl From<&ColliderRegistryAsset> for ColliderRegistry {
    fn from(asset: &ColliderRegistryAsset) -> Self {
        let mut colliders = asset.colliders.clone();
        let mut collider_vtables = HashMap::new();
        for (key, val) in asset.colliders.iter() {
            if val.is_empty() {
                colliders.remove(key);
                continue;
            }

            match key {
                ColliderType::Null => {}
                ColliderType::Capsule => {
                    let capsule = val.get::<CapsuleCollider>(0);
                    let vtable_ptr = unsafe { vtable::get_vtable_ptr(capsule as &dyn Collider) };
                    collider_vtables.insert(*key, vtable_ptr);
                }
                ColliderType::Plane => todo!(),
                ColliderType::Box => {
                    let b = val.get::<BoxCollider>(0);
                    let vtable_ptr = unsafe { vtable::get_vtable_ptr(b as &dyn Collider) };
                    collider_vtables.insert(*key, vtable_ptr);
                }
            }
        }

        Self {
            bins: HashMap::new(),
            colliders,
            collider_vtables,
        }
    }
}
