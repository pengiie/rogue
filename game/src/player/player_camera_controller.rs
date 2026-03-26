use nalgebra::{UnitQuaternion, Vector2, Vector3};
use rogue_engine::{
    entity::ecs_world::ECSWorld,
    input::{Input, keyboard::Key},
    physics::{rigid_body::RigidBody, transform::Transform},
    resource::{Res, ResMut},
    window::window::Window,
};
use rogue_macros::game_component;

use crate::player::player_controller::PlayerController;

#[derive(Clone)]
#[game_component(name = "PlayerCameraController")]
pub struct PlayerCameraController {
    distance: f32,
    euler: Vector2<f32>,
}

// Don't serialize data for this component.
rogue_engine::impl_unit_type_serde!(PlayerCameraController);

impl Default for PlayerCameraController {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerCameraController {
    pub fn new() -> Self {
        Self {
            distance: 5.0,
            // 0.1 because graphics is cooked, need to fix edge case of axis aligned camera.
            euler: Vector2::new(30.0f32.to_radians(), 0.1f32.to_radians()),
        }
    }

    pub fn on_update(ecs_world: ResMut<ECSWorld>, input: Res<Input>, mut window: ResMut<Window>) {
        if input.is_key_pressed(Key::Escape) {
            let is_locked = window.is_cursor_locked();
            window.set_cursor_lock(!is_locked);
        }

        let Some((camera_entity, (camera_transform, controller))) = ecs_world
            .query::<(&mut Transform, &mut PlayerCameraController)>()
            .into_iter()
            .next()
        else {
            return;
        };

        let Some((player_entity, (player_transform, player_rb))) = ecs_world
            .query::<(&mut Transform, &mut RigidBody)>()
            .with::<(PlayerController,)>()
            .into_iter()
            .next()
        else {
            log::error!("Can't find player entity for player camera controller.");
            return;
        };

        if window.is_cursor_locked() {
            let mouse_delta = input.mouse_delta() * 0.0005;
            controller.euler.x = (controller.euler.x - mouse_delta.y)
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            controller.euler.y += mouse_delta.x;
        }

        let target_rot = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), controller.euler.y)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), controller.euler.x);
        let target_pos = player_transform.position
            + controller.distance * target_rot.transform_vector(&-Vector3::z());
        camera_transform.position = camera_transform.position.lerp(&target_pos, 0.1);
        camera_transform.rotation = target_rot;
        player_transform.rotation = UnitQuaternion::from_axis_angle(
            &Vector3::y_axis(),
            controller.euler.y + std::f32::consts::PI,
        );
        // Small epsilon cause physics is cooked.
        player_transform.rotation *= UnitQuaternion::from_euler_angles(0.001, 0.001, 0.001);
    }
}
