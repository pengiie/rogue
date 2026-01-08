use nalgebra::{UnitQuaternion, Vector3};
use rogue_macros::game_component;

use crate::common::serde_util::impl_unit_type_serde;
use crate::entity::ecs_world::ECSWorld;
use crate::physics::{
    physics_world::PhysicsWorld,
    rigid_body::{RigidBody, RigidBodyType},
    transform::Transform,
};
use crate::resource::{Res, ResMut};

#[derive(Clone, Default)]
#[game_component(name = "SpinningPlatform")]
pub struct SpinningPlatform;
impl_unit_type_serde!(SpinningPlatform);

impl SpinningPlatform {
    pub fn on_physics_update(ecs_world: ResMut<ECSWorld>, physics_world: Res<PhysicsWorld>) {
        for (entity, (rigid_body, transform)) in ecs_world
            .query::<(&mut RigidBody, &mut Transform)>()
            .with::<(SpinningPlatform,)>()
            .into_iter()
        {
            let revolutions_per_sec = 0.05;
            rigid_body.rigid_body_type = RigidBodyType::KinematicPositionBased;
            transform.rotation *= UnitQuaternion::from_axis_angle(
                &Vector3::y_axis(),
                revolutions_per_sec
                    * 2.0
                    * std::f32::consts::PI
                    * physics_world.time_step().as_secs_f32(),
            );
        }
    }
}
