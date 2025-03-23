use log::debug;
use nalgebra::Vector3;
use noise::Vector4;
use rogue_macros::Resource;

use crate::{
    common::color::Color,
    consts,
    engine::{
        ecs::ecs_world::ECSWorld,
        graphics::camera::Camera,
        input::{mouse, Input},
        physics::transform::Transform,
        resource::{Res, ResMut},
        ui::UI,
    },
    game::entity::player::Player,
};

use super::{
    attachment::{Attachment, PTMaterial},
    voxel_world::VoxelWorld,
};

#[derive(Resource)]
pub struct VoxelCursor {
    data: Option<VoxelCursorData>,
}

pub struct VoxelCursorData {
    brush: VoxelBrush,
    brush_size: u32,
    // The locked distance the edit will be.
    ray_distance: f32,
    last_voxel: Vector3<i32>,
}

impl VoxelCursor {
    pub fn new() -> Self {
        Self { data: None }
    }

    pub fn update_post_physics(
        mut cursor: ResMut<VoxelCursor>,
        mut voxel_world: ResMut<VoxelWorld>,
        ecs_world: Res<ECSWorld>,
        input: Res<Input>,
        ui: Res<UI>,
    ) {
        let mut player_query =
            ecs_world.player_query::<(&mut Transform, &mut Camera, &mut Player)>();
        let Some((_player_entity, (player_transform, player_camera, player))) =
            player_query.try_player()
        else {
            return;
        };

        if input.is_mouse_button_pressed(mouse::Button::Right) {
            if let Some(hit_voxel) = voxel_world.trace_terrain(player_transform.get_ray(), 5.0) {
                let hit_voxel_meter = hit_voxel.cast::<f32>() * consts::voxel::VOXEL_METER_LENGTH
                    + Vector3::new(
                        consts::voxel::VOXEL_METER_LENGTH * 0.5,
                        consts::voxel::VOXEL_METER_LENGTH * 0.5,
                        consts::voxel::VOXEL_METER_LENGTH * 0.5,
                    );
                let player_voxel_distance =
                    (hit_voxel_meter - player_transform.isometry.translation.vector).magnitude();
                let cursor_data = VoxelCursorData {
                    brush: VoxelBrush::new(ui.debug_state.brush_color.clone()),
                    brush_size: ui.debug_state.brush_size,
                    ray_distance: player_voxel_distance,
                    last_voxel: hit_voxel,
                };
                Self::apply_edit(&cursor_data, &mut voxel_world);
                cursor.data = Some(cursor_data);
            } else {
                log::info!("Missed terrain when tracing");
            }
        }

        if let Some(cursor_data) = &mut cursor.data {
            if input.is_mouse_button_down(mouse::Button::Right) {
                let mut player_ray = player_transform.get_ray();
                player_ray.advance(cursor_data.ray_distance);
                let new_voxel = player_ray
                    .origin
                    .map(|x| (x / consts::voxel::VOXEL_METER_LENGTH).floor() as i32);
                if cursor_data.last_voxel != new_voxel {
                    cursor_data.last_voxel = new_voxel;
                    Self::apply_edit(&cursor_data, &mut voxel_world);
                }
            } else {
                cursor.data = None;
            }
        }
    }

    fn apply_edit(cursor_data: &VoxelCursorData, voxel_world: &mut VoxelWorld) {
        let edit = cursor_data
            .brush
            .create_edit(cursor_data.last_voxel, Vector3::new(2, 2, 2));
        voxel_world.apply_voxel_edit(edit, |mut flat, world_position, local_position| {
            flat.get_voxel_mut(local_position).set_attachment(
                Attachment::PTMATERIAL,
                Some(PTMaterial::diffuse(cursor_data.brush.color.clone()).encode()),
            );
        });
    }
}

pub struct VoxelBrush {
    pub color: Color,
    pub mask: Option<VoxelEditMask>,
}

impl VoxelBrush {
    pub fn new(color: Color) -> Self {
        Self { color, mask: None }
    }

    pub fn create_edit(&self, position: Vector3<i32>, length: Vector3<u32>) -> VoxelEdit {
        VoxelEdit {
            world_voxel_position: position,
            world_voxel_length: length,
            mask: self.mask.clone(),
            color: self.color.clone(),
        }
    }
}

pub struct VoxelEdit {
    // Minimum bottom-down-left origin.
    pub world_voxel_position: Vector3<i32>,
    // Length in voxels of the edit.
    pub world_voxel_length: Vector3<u32>,
    pub color: Color,
    // In order list of masks to apply.
    pub mask: Option<VoxelEditMask>,
}

#[derive(Clone)]
pub enum VoxelEditMask {
    Sphere { center: Vector3<i32>, rad_perc: f32 },
    Plane { axis: Vector3<bool> },
}
