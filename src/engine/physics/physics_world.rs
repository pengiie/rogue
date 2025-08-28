use std::{collections::HashMap, time::Duration};

use hecs::Without;
use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    common::{
        aabb::AABB,
        dyn_vec::{DynVec, TypeInfo},
        freelist::FreeList,
    },
    engine::{
        entity::{
            ecs_world::{ECSWorld, Entity},
            EntityChildren, EntityParent,
        },
        resource::ResMut,
        voxel::voxel_terrain::VoxelChunks,
        window::time::Instant,
    },
};

use super::{
    capsule_collider::CapsuleCollider,
    plane_collider::PlaneCollider,
    rigid_body::{ForceType, RigidBody},
    transform::Transform,
};

pub enum PhysicsTimestep {
    Max(Duration),
    Fixed(Duration),
}

pub struct PhysicsSettings {
    timestep: PhysicsTimestep,
    /// acceleration, meters / seconds^2
    gravity: Vector3<f32>,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            timestep: PhysicsTimestep::Max(Duration::from_millis(10)),
            gravity: Vector3::new(0.0, -9.8, 0.0),
        }
    }
}

pub struct CollisionInfo {
    pub penetration_depth: Vector3<f32>,
}

pub trait ColliderConcrete {
    fn concrete_collider_type() -> ColliderType;
}

pub trait Collider: downcast::Any {
    fn test_collision(&self, other: &dyn Collider) -> Option<CollisionInfo>;
    fn aabb(&self, world_transform: &Transform) -> AABB;
    fn collider_type(&self) -> ColliderType;
}

downcast::downcast!(dyn Collider);

// Spatial hashmap binning colliders per region.
pub struct ColliderRegistry {
    bins: HashMap</*region_pos*/ Vector3<i32>, Vec<(Entity, ColliderId)>>,
    colliders: HashMap<ColliderType, DynVec>,
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

    pub fn register_collider<C: Collider + 'static>(&mut self, collider: C) -> ColliderId {
        let collider_type = collider.collider_type();

        let collider_vtable_ptr = {
            // dyn pointer to the collider data and vtable.
            let collider_dyn = &collider as &dyn Collider;
            // pointer to the dyn pointer, reinterpreting as a tuple of two pointers.
            let collider_dyn_fat_ptr =
                std::ptr::from_ref(&collider_dyn) as *const _ as *const (*const (), *const ());
            let collider_vtable_ptr = unsafe { collider_dyn_fat_ptr.as_ref() }.unwrap().1;
            collider_vtable_ptr
        };

        let vec = self
            .colliders
            .entry(collider_type)
            .or_insert(DynVec::new(TypeInfo::new::<C>()));
        let index = vec.size();
        vec.push(collider);
        assert!(
            self.collider_vtables
                .insert(collider_type, collider_vtable_ptr)
                .map_or(true, |vtable| vtable == collider_vtable_ptr),
            "Two different implementations for same collider type."
        );
        return ColliderId {
            collider_type: collider_type,
            index,
        };
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

#[derive(Resource)]
pub struct PhysicsWorld {
    last_timestep: Instant,
    update_time: Instant,
    settings: PhysicsSettings,
    pub colliders: ColliderRegistry,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            last_timestep: Instant::now(),
            update_time: Instant::now(),
            settings: PhysicsSettings::default(),
            colliders: ColliderRegistry::new(),
        }
    }

    pub fn reset_last_timestep(&mut self) {
        self.last_timestep = Instant::now();
    }

    pub fn physics_update_count(&mut self) -> u32 {
        self.update_time = Instant::now();
        let dur = self.update_time - self.last_timestep;
        match self.settings.timestep {
            PhysicsTimestep::Max(max_duration) => {
                (dur.as_secs_f32() / max_duration.as_secs_f32()).ceil() as u32
            }
            PhysicsTimestep::Fixed(fixed_duration) => {
                let updates = (dur.as_secs_f32() / fixed_duration.as_secs_f32()).floor() as u32;
                updates
            }
        }
    }

    pub fn do_physics_update(
        mut physics_world: ResMut<PhysicsWorld>,
        mut ecs_world: ResMut<ECSWorld>,
    ) {
        let timestep = match physics_world.settings.timestep {
            PhysicsTimestep::Max(duration) => {
                (physics_world.update_time - physics_world.last_timestep).min(duration)
            }
            PhysicsTimestep::Fixed(duration) => duration,
        };

        for (entity, (transform, rigid_body)) in
            ecs_world.query_mut::<Without<(&mut Transform, &mut RigidBody), &EntityParent>>()
        {
            // Apply gravity.
            rigid_body.apply_force(
                ForceType::Force,
                rigid_body.mass() * physics_world.settings.gravity,
            );

            rigid_body.update(timestep, transform);
        }

        physics_world
            .colliders
            .update_entity_collider_positions(&mut ecs_world);

        for (_, bin) in physics_world.colliders.bins.iter() {
            for (entity_a, collider_id_a) in bin {
                for (entity_b, collider_id_b) in bin {
                    if *entity_a == *entity_b {
                        continue;
                    }

                    // TODO: Do collider triggers that don't require a rigid body.
                    let Ok(mut rigid_body_a) = ecs_world.get::<&mut RigidBody>(*entity_a) else {
                        continue;
                    };
                    let Ok(mut rigid_body_b) = ecs_world.get::<&mut RigidBody>(*entity_b) else {
                        continue;
                    };

                    let collider_a = physics_world.colliders.get_collider_dyn(collider_id_a);
                    let collider_b = physics_world.colliders.get_collider_dyn(collider_id_b);
                    let Some(collision_info) = collider_a.test_collision(collider_b) else {
                        continue;
                    };

                    // Momentum (p) = mass * velocity
                    // COM applies here so:
                    // m1*vi1 + m2*vi2 = m1*vf1 + m2*vf2
                    // vf1 = m1*vi1 + m2*vi2 -
                    let new_v1 =
                        rigid_body_a.inv_mass() * rigid_body_b.mass() * rigid_body_b.velocity;
                    let new_v2 =
                        rigid_body_b.inv_mass() * rigid_body_a.mass() * rigid_body_a.velocity;
                    rigid_body_a.velocity = new_v1;
                    rigid_body_b.velocity = new_v2;

                    // Depending on restitution, COE also applies.
                    // KE = 1/2 * mass * velocity^2
                    // 1/2*m1*vi1^2 + 1/2*m2*vi2^2 =  q
                }
            }
        }

        physics_world.last_timestep = physics_world.last_timestep + timestep;
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Colliders {
    pub colliders: Vec<ColliderId>,
}

impl Default for Colliders {
    fn default() -> Self {
        Self::new()
    }
}

impl Colliders {
    pub fn new() -> Self {
        Self {
            colliders: Vec::new(),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, serde::Serialize, serde::Deserialize, Debug)]
pub enum ColliderType {
    Null,
    Capsule,
    Plane,
}
