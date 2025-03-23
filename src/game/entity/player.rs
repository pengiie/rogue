use core::f32;
use std::borrow::Borrow;

use log::debug;
use nalgebra::{AbstractRotation, Rotation3, Translation3, UnitQuaternion, Vector2, Vector3};

use crate::{
    engine::{
        ecs::ecs_world::ECSWorld,
        graphics::camera::{Camera, MainCamera},
        input::{keyboard::Key, Input},
        physics::transform::Transform,
        resource::{Res, ResMut},
        ui::UI,
        window::{time::Time, window::Window},
    },
    settings::Settings,
};

use super::{GameEntity, GameEntityType};

pub struct Player {
    euler: Vector3<f32>,
    movement_speed: f32,
    paused: bool,
}

impl Player {
    pub fn new() -> Self {
        Self {
            euler: Vector3::zeros(),
            paused: true,
            movement_speed: 4.0,
        }
    }

    pub fn update(
        ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
        time: Res<Time>,
        settings: Res<Settings>,
        window: Res<Window>,
        ui: Res<UI>,
    ) {
        let mut player_query =
            ecs_world.player_query::<(&mut Transform, &mut Camera, &mut Player)>();
        let (_player_entity, (transform, camera, player)) = player_query.player();

        if input.is_key_pressed(Key::Escape) || input.is_key_pressed(Key::Tab) {
            player.paused = !player.paused;
            window.set_cursor_grabbed(!player.paused);
            window.set_cursor_visible(player.paused);
        }

        let md = input.mouse_delta();
        if (md.0 != 0.0 || md.1 != 0.0) && !player.paused {
            // Clamp up and down yaw.
            player.euler.x = (player.euler.x + md.1 * settings.mouse_sensitivity)
                .clamp(-f32::consts::FRAC_PI_2, f32::consts::FRAC_PI_2);
            player.euler.y += md.0 * settings.mouse_sensitivity;
        }
        transform.isometry.rotation =
            UnitQuaternion::from_euler_angles(player.euler.x, player.euler.y, 0.0);

        let input_axes = Vector2::new(input.horizontal_axis(), input.vertical_axis());

        let mut translation = Vector3::new(0.0, 0.0, 0.0);
        if input_axes.x != 0.0 || input_axes.y != 0.0 {
            let yaw_quaternion = UnitQuaternion::from_euler_angles(0.0, player.euler.y, 0.0);
            let rotated_xz = yaw_quaternion
                .transform_vector(&Vector3::new(input_axes.x, 0.0, input_axes.y))
                .normalize();
            translation.x = rotated_xz.x;
            translation.z = rotated_xz.z;
        }

        if input.is_key_down(Key::Space) {
            translation.y = 1.0;
        }
        if input.is_key_down(Key::LShift) {
            translation.y = -1.0;
        }

        let mut speed = player.movement_speed;
        if input.is_key_down(Key::LControl) {
            speed = 10.0;
        }

        transform.isometry.translation.vector +=
            translation * speed * time.delta_time().as_secs_f32();
    }

    pub fn spawn(mut ecs_world: ResMut<ECSWorld>, mut main_camera: ResMut<MainCamera>) {
        if ecs_world.query::<()>().with::<&Player>().iter().len() > 0 {
            panic!("Player already spawned.");
        }

        let player = ecs_world.spawn((
            GameEntity::new(GameEntityType::Player).set_name("a_player_name"),
            Player::new(),
            Camera::new(90.0),
            Transform::with_translation(Translation3::new(-0.0, -2.0, -6.0)),
        ));
        main_camera.set_camera(player, "player_camera");
    }
}
