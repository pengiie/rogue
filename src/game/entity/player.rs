use core::f32;
use std::borrow::Borrow;

use log::debug;
use nalgebra::{AbstractRotation, Rotation3, Translation3, UnitQuaternion, Vector2, Vector3};

use crate::{
    engine::{
        entity::{ecs_world::ECSWorld, GameEntity},
        graphics::camera::{Camera, MainCamera},
        input::{keyboard::Key, Input},
        physics::transform::Transform,
        resource::{Res, ResMut},
        ui::UI,
        window::{time::Time, window::Window},
    },
    settings::Settings,
};

pub struct Player {
    euler: Vector3<f32>,
    movement_speed: f32,
    pub paused: bool,
}

impl Player {
    pub fn new(euler: Vector3<f32>) -> Self {
        Self {
            euler,
            paused: true,
            movement_speed: 4.0,
        }
    }

    pub fn update_from_input(
        ecs_world: ResMut<ECSWorld>,
        mut input: ResMut<Input>,
        time: Res<Time>,
        mut settings: ResMut<Settings>,
        mut window: ResMut<Window>,
        ui: Res<UI>,
    ) {
        let mut player_query =
            ecs_world.player_query::<(&mut Transform, &mut Camera, &mut Player)>();
        let (_player_entity, (transform, camera, player)) = player_query.player();

        if input.is_key_pressed(Key::Escape) || input.is_key_pressed(Key::Tab) {
            player.paused = !player.paused;
            window.set_curser_lock(!player.paused);
        }

        let mut md = input.camera_axes();
        if (md.x != 0.0 || md.y != 0.0) && (!player.paused || input.is_controller_camera()) {
            // Clamp up and down yaw.
            if input.is_controller_camera() {
                md *= settings.controller_sensitity * time.delta_time().as_secs_f32();
            } else {
                md *= settings.mouse_sensitivity;
            }

            player.euler.x =
                (player.euler.x - md.y).clamp(-f32::consts::FRAC_PI_2, f32::consts::FRAC_PI_2);
            player.euler.y += md.x;
        }
        transform.rotation = UnitQuaternion::from_euler_angles(player.euler.x, player.euler.y, 0.0);

        let input_axes = input.movement_axes();

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

        transform.position += translation * speed * time.delta_time().as_secs_f32();
        settings.player_position = transform.position;
        settings.player_rotation = player.euler;
    }

    pub fn spawn(
        mut ecs_world: ResMut<ECSWorld>,
        mut main_camera: ResMut<MainCamera>,
        settings: Res<Settings>,
    ) {
        if ecs_world.query::<()>().with::<&Player>().iter().len() > 0 {
            panic!("Player already spawned.");
        }

        let player = ecs_world.spawn((
            GameEntity::new("a_player_name"),
            Player::new(settings.player_rotation),
            Camera::new(90.0f32.to_radians()),
            Transform::with_translation(Translation3::from(settings.player_position)),
        ));
        main_camera.set_camera(player, "player_camera");
    }
}
