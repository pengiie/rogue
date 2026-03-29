use nalgebra::{UnitQuaternion, Vector2, Vector3};
use rogue_engine::{
    common::geometry::ray::Ray,
    entity::ecs_world::ECSWorld,
    input::{Input, keyboard::Key},
    physics::{physics_world::PhysicsWorld, rigid_body::RigidBody, transform::Transform},
    resource::{Res, ResMut},
    voxel::voxel_registry::VoxelModelRegistry,
    window::{time::Time, window::Window},
    world::region_map::RegionMap,
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
            distance: 2.0,
            // 0.1 because graphics is cooked, need to fix edge case of axis aligned camera.
            euler: Vector2::new(
                30.0f32.to_radians(),
                0.1f32.to_radians() + std::f32::consts::PI,
            ),
        }
    }

    pub fn on_update(
        ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
        mut window: ResMut<Window>,
        physics_world: Res<PhysicsWorld>,
        time: Res<Time>,
        region_map: Res<RegionMap>,
        voxel_registry: Res<VoxelModelRegistry>,
    ) {
        let Some((camera_entity, (camera_transform, controller))) = ecs_world
            .query::<(&mut Transform, &mut PlayerCameraController)>()
            .into_iter()
            .next()
        else {
            return;
        };

        let Some((player_entity, (player_transform, player_rb, player_controller))) = ecs_world
            .query::<(&mut Transform, &mut RigidBody, &PlayerController)>()
            .into_iter()
            .next()
        else {
            log::error!("Can't find player entity for player camera controller.");
            return;
        };

        let look_at = player_controller.looking.aim_rot;
        controller.euler = Vector2::new(look_at.x, look_at.y);
        let target_rot = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), controller.euler.y)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -controller.euler.x);
        let anchor_pos = player_transform.position + Vector3::y() * 0.63;
        let ray = Ray::new(anchor_pos, target_rot.transform_vector(&-Vector3::z()));
        // Raycast needs more testing first.
        //let raycast = region_map.raycast_terrain(&voxel_registry, &ray, controller.distance);
        //let raycast_t = raycast.as_ref().map_or(controller.distance, |hit| {
        //    (hit.model_trace.depth_t - 1.0).clamp(0.0, controller.distance)
        //});
        let mut target_pos = anchor_pos + ray.dir * controller.distance;
        //if let Some(raycast) = raycast {}
        camera_transform.position = camera_transform.position.lerp(&target_pos, 1.0);
        camera_transform.rotation = target_rot;
    }
}
