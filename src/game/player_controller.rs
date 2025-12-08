use nalgebra::{UnitQuaternion, Vector2, Vector3};
use rogue_macros::game_component;

use crate::{
    common::serde_util::impl_unit_type_serde,
    consts,
    engine::{
        entity::ecs_world::ECSWorld,
        input::{keyboard::Key, Input},
        physics::{
            physics_world::PhysicsWorld,
            rigid_body::{ForceType, RigidBody},
            transform::Transform,
        },
        resource::{Res, ResMut},
    },
};

#[derive(Clone)]
#[game_component(name = "PlayerController")]
pub struct PlayerController {
    input_state: PlayerControllerInputState,
}

// Don't serialize data for this component.
impl_unit_type_serde!(PlayerController);

#[derive(Clone)]
pub struct PlayerControllerInputState {
    movement_axes: Vector2<f32>,
    did_jump: bool,
}

impl PlayerControllerInputState {
    pub fn new() -> Self {
        PlayerControllerInputState {
            movement_axes: Vector2::new(0.0, 0.0),
            did_jump: false,
        }
    }

    pub fn reset(&mut self) {
        *self = PlayerControllerInputState::new();
    }
}

impl Default for PlayerController {
    fn default() -> Self {
        PlayerController::new()
    }
}

impl PlayerController {
    pub fn new() -> Self {
        PlayerController {
            input_state: PlayerControllerInputState::new(),
        }
    }

    pub fn on_update(mut ecs_world: ResMut<ECSWorld>, input: Res<Input>) {
        let Some((entity, (transform, rigid_body, controller))) = ecs_world
            .query_mut::<(&Transform, &RigidBody, &mut PlayerController)>()
            .into_iter()
            .next()
        else {
            return;
        };

        let movement_axes = input.movement_axes();
        controller.input_state.movement_axes = movement_axes;

        controller.input_state.did_jump |= input.is_key_pressed(Key::Space);
    }

    pub fn on_physics_update(mut ecs_world: ResMut<ECSWorld>, physics_world: Res<PhysicsWorld>) {
        let Some((entity, (transform, mut rigid_body, controller))) = ecs_world
            .query_mut::<(&Transform, &mut RigidBody, &mut PlayerController)>()
            .into_iter()
            .next()
        else {
            return;
        };

        if controller.input_state.movement_axes.norm_squared() != 0.0 {
            log::debug!(
                "Applying movement axes: {:?}",
                controller.input_state.movement_axes
            );
            let y_rotation = UnitQuaternion::from_axis_angle(
                &Vector3::y_axis(),
                transform.rotation.euler_angles().1,
            );
            let translation = y_rotation
                * Vector3::new(
                    controller.input_state.movement_axes.x,
                    0.0,
                    controller.input_state.movement_axes.y,
                )
                .normalize();
            rigid_body.apply_force(ForceType::VelocityChange, translation);
        }

        controller.input_state.reset();
    }
}
