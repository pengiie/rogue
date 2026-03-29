use nalgebra::{UnitQuaternion, Vector2, Vector3};
use rogue_engine::input::gamepad;
use rogue_engine::window::time::Time;
use rogue_engine::window::window::Window;
use rogue_macros::game_component;

use rogue_engine::entity::ecs_world::ECSWorld;
use rogue_engine::input::{Input, keyboard::Key};
use rogue_engine::physics::{
    physics_world::PhysicsWorld,
    rigid_body::{ForceType, RigidBody},
    transform::Transform,
};
use rogue_engine::resource::{Res, ResMut};

#[derive(Clone)]
#[game_component(name = "PlayerController")]
pub struct PlayerController {
    input_state: PlayerControllerInputState,
    speed: f32,
    pub looking: PlayerControllerLooking,
}

#[derive(Clone)]
pub struct PlayerControllerLooking {
    pub aim_rot: Vector2<f32>, // pitch, yaw
    moving_rot: f32,           // yaw
}

// Don't serialize data for this component.
rogue_engine::impl_unit_type_serde!(PlayerController);

#[derive(Clone)]
pub struct PlayerControllerInputState {
    movement_axes: Vector2<f32>,
    did_jump: bool,
    running: bool,
}

impl PlayerControllerInputState {
    pub fn new() -> Self {
        PlayerControllerInputState {
            movement_axes: Vector2::new(0.0, 0.0),
            did_jump: false,
            running: false,
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
            speed: 2.0,
            looking: PlayerControllerLooking {
                aim_rot: Vector2::zeros(),
                moving_rot: 0.0,
            },
        }
    }

    pub fn on_update(
        mut ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
        mut window: ResMut<Window>,
        time: Res<Time>,
    ) {
        let Some((entity, (mut transform, rigid_body, controller))) = ecs_world
            .query_mut::<(&mut Transform, &RigidBody, &mut PlayerController)>()
            .into_iter()
            .next()
        else {
            return;
        };
        controller.input_state.reset();
        if input.is_key_pressed(Key::Escape) {
            let is_locked = window.is_cursor_locked();
            window.set_cursor_lock(!is_locked);
        }

        if input.is_controller_camera() {
            window.set_cursor_lock(true);
        }
        if window.is_cursor_locked() {
            let mut rot_delta = input.camera_axes();
            if input.is_controller_camera() {
                rot_delta *= 145.0f32.to_radians() * time.delta_time().as_secs_f32();
            } else {
                rot_delta *= 0.0005;
            }
            controller.looking.aim_rot.x = (controller.looking.aim_rot.x + rot_delta.y)
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            controller.looking.aim_rot.y += rot_delta.x;
        }

        let movement_axes = input.movement_axes();
        if movement_axes.x != 0.0 || movement_axes.y != 0.0 {
            // Update the player rotation.
            let y_rotation =
                UnitQuaternion::from_axis_angle(&Vector3::y_axis(), controller.looking.aim_rot.y);
            let new_moving_rot = y_rotation
                .transform_vector(&Vector3::new(movement_axes.x, 0.0, movement_axes.y))
                .normalize();
            // Lerp towards target position.
            let t = 0.1;
            let mut target_rot = new_moving_rot.x.atan2(new_moving_rot.z);
            let mut d_rot = target_rot - controller.looking.moving_rot;
            while d_rot > std::f32::consts::PI {
                d_rot -= 2.0 * std::f32::consts::PI;
            }
            while d_rot < -std::f32::consts::PI {
                d_rot += 2.0 * std::f32::consts::PI;
            }
            controller.looking.moving_rot = controller.looking.moving_rot + t * d_rot;
        }
        transform.rotation =
            UnitQuaternion::from_axis_angle(&Vector3::y_axis(), controller.looking.moving_rot);

        controller.input_state.movement_axes = movement_axes;
        controller.input_state.did_jump |= input.is_key_pressed(Key::Space);
        controller.input_state.running =
            input.is_controller_button_down(gamepad::Button::RightTrigger);
    }

    pub fn on_fixed_update(mut ecs_world: ResMut<ECSWorld>, physics_world: Res<PhysicsWorld>) {
        let Some((entity, (transform, mut rigid_body, controller))) = ecs_world
            .query_mut::<(&Transform, &mut RigidBody, &mut PlayerController)>()
            .into_iter()
            .next()
        else {
            return;
        };

        let mut speed = controller.speed;
        if controller.input_state.running {
            speed *= 1.7;
        }

        if controller.input_state.movement_axes.norm_squared() != 0.0 {
            // Get yaw from rotation
            let y_rotation =
                UnitQuaternion::from_axis_angle(&Vector3::y_axis(), controller.looking.aim_rot.y);
            let translation = y_rotation
                * Vector3::new(
                    controller.input_state.movement_axes.x,
                    0.0,
                    controller.input_state.movement_axes.y,
                )
                .normalize();
            rigid_body.apply_force(ForceType::VelocityChange, translation * speed);
        }

        let jump_height = 6.0;
        // Time until apex of the jump.
        let jump_time = 0.75;
        let player_gravity = 75.0;
        if controller.input_state.did_jump {
            rigid_body.apply_force(ForceType::VelocityChange, Vector3::new(0.0, 30.0, 0.0));
        }

        // Velocity dampening
        rigid_body.velocity *= 0.8 * physics_world.time_step().as_secs_f32();

        controller.input_state.did_jump = false;
    }
}
