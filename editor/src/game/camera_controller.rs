use nalgebra::{UnitQuaternion, Vector2, Vector3};
use rogue_macros::game_component;

use crate::{
    common::serde_util::impl_unit_type_serde,
    engine::{
        entity::{ecs_world::ECSWorld, EntityParent},
        input::{keyboard::Key, Input},
        physics::{rigid_body::RigidBody, transform::Transform},
        resource::{Res, ResMut},
        window::window::Window,
    },
    game::player_controller::PlayerController,
};

#[derive(Clone)]
#[game_component(name = "CameraController")]
pub struct CameraController {
    distance: f32,
    euler: Vector2<f32>,
}

// Don't serialize data for this component.
impl_unit_type_serde!(CameraController);

impl Default for CameraController {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraController {
    pub fn new() -> Self {
        CameraController {
            distance: 5.0,
            // 0.1 because graphics is cooked, need to fix edge case of axis aligned camera.
            euler: Vector2::new(30.0f32.to_radians(), 0.1f32.to_radians()),
        }
    }

    pub fn on_update(ecs_world: ResMut<ECSWorld>, input: Res<Input>, mut window: ResMut<Window>) {
        if input.is_key_pressed(Key::Escape) {
            let is_locked = window.is_cursor_locked();
            window.set_curser_lock(!is_locked);
        }

        let Some((camera_entity, (camera_transform, controller))) = ecs_world
            .query::<(&mut Transform, &mut CameraController)>()
            .into_iter()
            .next()
        else {
            return;
        };

        let (player_entity, (player_transform, player_rb)) = ecs_world
            .query::<(&mut Transform, &mut RigidBody)>()
            .with::<(PlayerController,)>()
            .into_iter()
            .next()
            .unwrap();

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
