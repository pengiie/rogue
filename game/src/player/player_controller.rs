use nalgebra::{UnitQuaternion, Vector2, Vector3};
use rogue_engine::animation::animator::Animator;
use rogue_engine::asset::asset::GameAssetPath;
use rogue_engine::audio::{AudioPlayer, PlaySoundInfo};
use rogue_engine::common::geometry::ray::Ray;
use rogue_engine::consts;
use rogue_engine::input::gamepad;
use rogue_engine::voxel::voxel_registry::VoxelModelRegistry;
use rogue_engine::window::time::{Instant, Time};
use rogue_engine::window::window::Window;
use rogue_engine::world::terrain::region_map::RegionMap;
use rogue_macros::game_component;

use rogue_engine::entity::ecs_world::ECSWorld;
use rogue_engine::input::{Input, keyboard::Key};
use rogue_engine::physics::{
    physics_world::PhysicsWorld,
    rigid_body::{ForceType, RigidBody},
    transform::Transform,
};
use rogue_engine::resource::{Res, ResMut, ResourceBank};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "PlayerController")]
#[serde(default = "PlayerController::new")]
pub struct PlayerController {
    #[serde(skip)]
    input_state: PlayerControllerInputState,
    #[serde(skip)]
    speed: f32,
    #[serde(skip)]
    pub looking: PlayerControllerLooking,
    #[serde(skip)]
    is_grounded: bool,

    pub idle_animation: Option<GameAssetPath>,
}

#[derive(Clone)]
pub struct PlayerControllerLooking {
    pub aim_rot: Vector2<f32>, // pitch, yaw
    moving_rot: f32,           // yaw
}

#[derive(Clone)]
pub struct PlayerControllerInputState {
    movement_axes: Vector2<f32>,
    last_jump: Option<Instant>,
    running: bool,
}

impl PlayerControllerInputState {
    pub fn new() -> Self {
        PlayerControllerInputState {
            movement_axes: Vector2::new(0.0, 0.0),
            last_jump: None,
            running: false,
        }
    }

    pub fn reset(&mut self) {
        self.movement_axes = Vector2::new(0.0, 0.0);
        self.running = false;
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
            is_grounded: false,

            idle_animation: None,
        }
    }

    pub fn on_update(
        mut ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
        mut window: ResMut<Window>,
        time: Res<Time>,
    ) {
        let Some((entity, (mut transform, rigid_body, controller, animator, audio_player))) =
            ecs_world
                .query_mut::<(
                    &mut Transform,
                    &RigidBody,
                    &mut PlayerController,
                    &mut Animator,
                    &mut AudioPlayer,
                )>()
                .into_iter()
                .next()
        else {
            return;
        };
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
        controller.input_state.running =
            input.is_controller_button_down(gamepad::Button::RightTrigger);

        let did_input_jump = input.is_key_pressed(Key::Space)
            || input.is_controller_button_pressed(gamepad::Button::East);
        if did_input_jump {
            controller.input_state.last_jump = Some(time.curr_time());
        }

        if input.is_controller_button_pressed(gamepad::Button::West) {
            audio_player.play_sound(
                "footstep",
                PlaySoundInfo {
                    speed: 1.0,
                    pitch_shift: 1.0,
                },
            );
        }

        // Update animation.
        if let Some(idle_animation) = controller.idle_animation.as_ref() {
            if !animator.is_animation_playing(idle_animation) {
                animator.play_animation(
                    idle_animation,
                    rogue_engine::animation::animator::AnimatorPlayAnimationInfo {
                        repeat: true,
                        speed: 0.5,
                    },
                );
            }
        }
    }

    pub fn on_fixed_update(
        mut ecs_world: ResMut<ECSWorld>,
        physics_world: Res<PhysicsWorld>,
        region_map: Res<RegionMap>,
        voxel_registry: Res<VoxelModelRegistry>,
        time: Res<Time>,
    ) {
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
        // Velocity dampening
        rigid_body.velocity.x *= 0.8 * physics_world.time_step().as_secs_f32();
        rigid_body.velocity.z *= 0.8 * physics_world.time_step().as_secs_f32();

        let player_gravity = 8.0f32;

        let ground_ray = Ray::new(
            transform.position,
            ((-Vector3::y() + Vector3::new(0.01, 0.0, 0.01)).normalize()),
        );
        if transform.position.y < -3.5 {
            let x = 0.5;
        }

        const JUMP_BUFFER_MS: u32 = 100;
        let mut gravity_dv =
            Vector3::y() * -player_gravity * physics_world.time_step().as_secs_f32();
        if let Some(mut hit) = region_map.raycast_terrain(&voxel_registry, &ground_ray, 10.0) {
            let post_gravity_velocity =
                (rigid_body.velocity + gravity_dv).y * physics_world.time_step().as_secs_f32();
            if !controller.is_grounded && post_gravity_velocity < 0.0 {
                let predicted_ground_pos =
                    transform.position + Vector3::y() * post_gravity_velocity;
                if hit.model_trace.depth_t < gravity_dv.y.abs() {
                    rigid_body.set_position(predicted_ground_pos);
                    hit.model_trace.depth_t = 0.0;
                    rigid_body.velocity.y = 0.0;
                }
            }
            controller.is_grounded = hit.model_trace.depth_t <= 0.01;
        } else {
            controller.is_grounded = false;
        }

        // Time until apex of the jump.
        let jump_height = 6.0 * consts::voxel::VOXEL_METER_LENGTH;
        let jump_time = (2.0 * jump_height / player_gravity).sqrt();
        let did_jump = controller.input_state.last_jump.map_or(false, |jump_time| {
            (time.curr_time() - jump_time).as_millis() < JUMP_BUFFER_MS as u128
        });
        if did_jump && controller.is_grounded {
            controller.input_state.last_jump = None;
            let impulse = player_gravity * jump_time;
            rigid_body.apply_force(ForceType::VelocityChange, impulse * Vector3::y());
        }

        // Apply gravity
        if !controller.is_grounded {
            rigid_body.apply_force(ForceType::VelocityChange, gravity_dv);
        } else if !did_jump {
            rigid_body.velocity.y = 0.0;
        }

        controller.input_state.reset();
    }
}
