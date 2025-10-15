use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use nalgebra::Vector3;
use rogue_macros::Resource;

use super::{
    capsule_collider::CapsuleCollider,
    plane_collider::PlaneCollider,
    rigid_body::{ForceType, RigidBody},
    transform::Transform,
};
use crate::{common::geometry::aabb::AABB, engine::physics::rigid_body::RigidBodyType};
use crate::{
    common::{
        dyn_vec::{DynVecCloneable, TypeInfo},
        freelist::FreeList,
    },
    engine::{
        debug::DebugRenderer,
        editor::editor::Editor,
        entity::{
            ecs_world::{ECSWorld, Entity},
            EntityChildren, EntityParent,
        },
        physics::{collider::Colliders, collider_registry::ColliderRegistry},
        resource::{Res, ResMut},
        voxel::terrain::chunks::VoxelChunks,
        window::time::Instant,
    },
    session::Session,
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

    pub fn render_debug_colliders(
        mut physics_world: ResMut<PhysicsWorld>,
        mut debug_renderer: ResMut<DebugRenderer>,
        editor: Res<Editor>,
        ecs_world: Res<ECSWorld>,
    ) {
        for (entity, (transform, colliders)) in ecs_world
            .query::<(&Transform, &Colliders)>()
            .without::<(EntityParent,)>()
            .into_iter()
        {
            for collider_id in &colliders.colliders {
                let collider = physics_world.colliders.get_collider_dyn(collider_id);
                collider.render_debug(&transform, &mut debug_renderer);
            }
        }
    }

    pub fn validate_colliders_exist(&self, ecs_world: &mut ECSWorld) {
        for (entity, colliders) in ecs_world.query_mut::<&Colliders>().into_iter() {
            for collider_id in &colliders.colliders {
                assert!(self.colliders.contains_id(collider_id));
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

        for (entity, (transform, rigid_body)) in ecs_world
            .query_mut::<(&mut Transform, &mut RigidBody)>()
            .without::<(EntityParent,)>()
            .into_iter()
        {
            if rigid_body.rigid_body_type == RigidBodyType::Dynamic {
                // Apply gravity.
                rigid_body.apply_force(
                    ForceType::Force,
                    rigid_body.mass() * physics_world.settings.gravity,
                );

                rigid_body.update(timestep, transform);
            }
        }

        physics_world
            .colliders
            .update_entity_collider_positions(&mut ecs_world);

        let mut tested_collision = HashSet::new();
        for (_, bin) in physics_world.colliders.bins.iter() {
            for (entity_a, collider_id_a) in bin {
                for (entity_b, collider_id_b) in bin {
                    if *entity_a == *entity_b {
                        continue;
                    }

                    if tested_collision.contains(&(*entity_a, *entity_b)) {
                        continue;
                    }
                    tested_collision.insert((*entity_a, *entity_b));
                    tested_collision.insert((*entity_b, *entity_a));

                    // TODO: Do collider triggers that don't require a rigid body.
                    let mut query = ecs_world
                        .query_many_mut::<(&mut Transform, &mut RigidBody), 2>([
                            *entity_a, *entity_b,
                        ]);
                    let [Some((transform_a, rigid_body_a)), Some((transform_b, rigid_body_b))] =
                        query.get()
                    else {
                        continue;
                    };

                    let collider_a = physics_world.colliders.get_collider_dyn(collider_id_a);
                    let collider_b = physics_world.colliders.get_collider_dyn(collider_id_b);
                    let Some(collision_info) =
                        collider_a.test_collision(collider_b, &transform_a, &transform_b)
                    else {
                        continue;
                    };
                    if collision_info.penetration_depth.norm_squared() == 0.0 {
                        continue;
                    }

                    let normal = collision_info.penetration_depth.normalize();
                    let relative_velocity = rigid_body_b.velocity() - rigid_body_a.velocity();
                    let relative_velocity_along_normal = relative_velocity.dot(&normal);

                    let restitution = rigid_body_a.restitution.min(rigid_body_b.restitution);
                    let inv_mass_a = rigid_body_a.inv_mass();
                    let inv_mass_b = rigid_body_b.inv_mass();

                    let impulse = -((1.0 + restitution) * relative_velocity_along_normal)
                        / (inv_mass_a + inv_mass_b);
                    let impulse_vec = impulse * normal;

                    // Apply the normal velocity.
                    rigid_body_a.velocity -= inv_mass_a * impulse_vec;
                    rigid_body_b.velocity += inv_mass_b * impulse_vec;

                    const POSITION_CORRECTION_FACTOR: f32 = 0.8;
                    // Correct position so objects are no longer penetrating.
                    let weighting_a =
                        POSITION_CORRECTION_FACTOR * (inv_mass_a / (inv_mass_a + inv_mass_b));
                    let weighting_b =
                        POSITION_CORRECTION_FACTOR * (inv_mass_b / (inv_mass_a + inv_mass_b));
                    transform_a.position -= weighting_a * collision_info.penetration_depth;
                    transform_b.position += weighting_b * collision_info.penetration_depth;
                }
            }
        }

        physics_world.last_timestep = physics_world.last_timestep + timestep;
    }
}
