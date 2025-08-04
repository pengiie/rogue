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

pub trait Collider: downcast::Any {
    fn test_collision(&self, other: &dyn Collider) -> Option<CollisionInfo>;
    fn aabb(&self, world_transform: &Transform) -> AABB;
    fn collider_type(&self) -> ColliderType;
}

downcast::downcast!(dyn Collider);

/// An octree where the leaves are bins of colliders.
/// TODO: Dynamically resize leaves to optimize collision checks.
pub struct ColliderRegistry {
    bins: HashMap</*region_pos*/ Vector3<i32>, Vec<ColliderId>>,
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

    pub fn register_collider<C: Collider + 'static>(
        &mut self,
        entity: Entity,
        collider: C,
    ) -> ColliderId {
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
                .is_none(),
            "Two different implementations for same collider type."
        );
        return ColliderId {
            collider_type: collider_type,
            index,
        };
    }

    pub fn update_entity_collider_positions(&mut self, ecs_world: &mut ECSWorld) {
        self.bins.clear();
        for (entity, (transform, colliders)) in ecs_world
            .query_mut::<Without<(&Transform, &Colliders), &EntityParent>>()
            .into_iter()
        {
            for collider_id in &colliders.colliders {
                let aabb = self.get_collider_dyn(collider_id).aabb(transform);

                //self.bins.entry().or_insert(Vec::new())
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
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
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
    colliders: ColliderRegistry,
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

        for (entity, (transform, rigid_body, colliders)) in ecs_world
            .query::<Without<(&mut Transform, &mut RigidBody, &Colliders), &EntityParent>>()
            .into_iter()
        {
            // Apply gravity.
            rigid_body.apply_force(
                ForceType::Force,
                rigid_body.mass() * physics_world.settings.gravity,
            );

            rigid_body.update(timestep, transform);
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

#[derive(PartialEq, Eq, Clone, Copy, Hash, serde::Serialize, serde::Deserialize)]
pub enum ColliderType {
    Null,
    Capsule,
    Plane,
}
