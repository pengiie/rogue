use log::debug;
use nalgebra::Vector3;
use noise::Vector4;
use rogue_macros::Resource;

use crate::{
    common::color::Color,
    consts,
    engine::{
        debug::{DebugFlags, DebugLine, DebugRenderer},
        entity::{
            ecs_world::{ECSWorld, Entity},
            RenderableVoxelEntity,
        },
        graphics::camera::Camera,
        input::{mouse, Input},
        physics::transform::Transform,
        resource::{Res, ResMut},
        ui::UI,
    },
    game::entity::player::Player,
};

use super::{
    attachment::{Attachment, AttachmentInfoMap, AttachmentMap, PTMaterial},
    voxel_registry::VoxelModelId,
    voxel_world::{VoxelTraceInfo, VoxelWorld},
};

#[derive(Resource)]
pub struct VoxelCursor {
    data: Option<VoxelCursorData>,
    selected_entity: Option<Entity>,
}

pub struct VoxelCursorData {
    brush: Option<VoxelBrush>,
    brush_size: u32,
    // The locked distance the edit will be.
    ray_distance: f32,
    last_voxel: Vector3<i32>,
}

impl VoxelCursor {
    pub fn new() -> Self {
        Self {
            data: None,
            selected_entity: None,
        }
    }

    pub fn update_post_physics(
        mut cursor: ResMut<VoxelCursor>,
        mut voxel_world: ResMut<VoxelWorld>,
        ecs_world: Res<ECSWorld>,
        input: Res<Input>,
        ui: Res<UI>,
        mut debug: ResMut<DebugRenderer>,
    ) {
        let mut player_query =
            ecs_world.player_query::<(&mut Transform, &mut Camera, &mut Player)>();
        let Some((player_entity, (player_local_transform, player_camera, player))) =
            player_query.try_player()
        else {
            return;
        };
        let player_world_transform =
            ecs_world.get_world_transform(player_entity, &player_local_transform);

        if cursor.selected_entity.is_some() && !ecs_world.contains(cursor.selected_entity.unwrap())
        {
            cursor.selected_entity = None;
        }
        if let Some(entity) = &cursor.selected_entity {
            let mut query = ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(*entity);
            let (entity_transform, entity_renderable) = query.get().unwrap();
            let dimensions = voxel_world
                .get_dyn_model(entity_renderable.voxel_model_id_unchecked())
                .length();

            let thickness = 0.1;
            let color = Color::new_srgb(1.0, 1.0, 1.0);
            let alpha = 1.0;
            let line = |start, end| DebugLine {
                start,
                end,
                thickness,
                color: color.clone(),
                alpha,
                flags: DebugFlags::NONE,
            };

            let obb = entity_transform.as_voxel_model_obb(dimensions);
            let rot = obb.rotation;
            let (min, max) = obb.rotated_min_max();
            let forward = rot.transform_vector(&Vector3::z())
                * dimensions.z as f32
                * consts::voxel::VOXEL_METER_LENGTH;
            let right = rot.transform_vector(&Vector3::x())
                * dimensions.x as f32
                * consts::voxel::VOXEL_METER_LENGTH;
            let up = rot.transform_vector(&Vector3::y())
                * dimensions.y as f32
                * consts::voxel::VOXEL_METER_LENGTH;
            // Draws the edges of an OBB.
            debug.draw_line(line(min, min + forward));
            debug.draw_line(line(min, min + right));
            debug.draw_line(line(min + forward, min + right + forward));
            debug.draw_line(line(min + right, min + right + forward));

            debug.draw_line(line(min + up, min + forward + up));
            debug.draw_line(line(min + up, min + right + up));
            debug.draw_line(line(min + forward + up, max));
            debug.draw_line(line(min + right + up, max));

            debug.draw_line(line(min, min + up));
            debug.draw_line(line(min + right, min + right + up));
            debug.draw_line(line(min + forward, min + forward + up));
            debug.draw_line(line(min + forward + right, max));

            //debug.draw_line(line(max, Vector3::new(min.x, max.y, max.z)));
            //debug.draw_line(line(max, Vector3::new(max.x, min.y, max.z)));
            //debug.draw_line(line(max, Vector3::new(max.x, max.y, min.z)));

            //debug.draw_line(line(
            //    Vector3::new(min.x, max.y, min.z),
            //    Vector3::new(max.x, max.y, min.z),
            //));
            //debug.draw_line(line(
            //    Vector3::new(min.x, max.y, min.z),
            //    Vector3::new(min.x, max.y, max.z),
            //));
            //debug.draw_line(line(
            //    Vector3::new(min.x, max.y, max.z),
            //    Vector3::new(min.x, min.y, max.z),
            //));

            //debug.draw_line(line(
            //    Vector3::new(max.x, min.y, max.z),
            //    Vector3::new(max.x, min.y, min.z),
            //));
            //debug.draw_line(line(
            //    Vector3::new(max.x, min.y, max.z),
            //    Vector3::new(min.x, min.y, max.z),
            //));
            //debug.draw_line(line(
            //    Vector3::new(max.x, min.y, min.z),
            //    Vector3::new(max.x, max.y, min.z),
            //));
        }

        if input.is_mouse_button_pressed(mouse::Button::Middle) {
            let voxel_pos = player_local_transform
                .position
                .map(|x| (x / consts::voxel::VOXEL_METER_LENGTH).floor() as i32);

            let mut attachment_map = AttachmentMap::new();
            attachment_map.register_attachment(Attachment::PTMATERIAL);
            voxel_world.apply_voxel_edit(
                VoxelEditInfo {
                    world_voxel_position: voxel_pos,
                    world_voxel_length: Vector3::new(1, 1, 1),
                    attachment_map,
                },
                |mut voxel, world_position, local_position| {
                    voxel.set_attachment(
                        Attachment::PTMATERIAL_ID,
                        &[PTMaterial::diffuse(Color::new_srgb(1.0, 0.0, 0.0)).encode()],
                    );
                },
            );
        }

        if input.is_mouse_button_pressed(mouse::Button::Right)
            || input.is_mouse_button_pressed(mouse::Button::Left)
        {
            if let Some(trace_info) =
                voxel_world.trace_world(&&ecs_world, player_local_transform.get_ray())
            {
                match trace_info {
                    VoxelTraceInfo::Terrain { world_voxel_pos } => {
                        let hit_voxel_meter = world_voxel_pos.cast::<f32>()
                            * consts::voxel::VOXEL_METER_LENGTH
                            + Vector3::new(
                                consts::voxel::VOXEL_METER_LENGTH * 0.5,
                                consts::voxel::VOXEL_METER_LENGTH * 0.5,
                                consts::voxel::VOXEL_METER_LENGTH * 0.5,
                            );
                        let player_voxel_distance =
                            (hit_voxel_meter - player_local_transform.position).magnitude();

                        let mut cursor_data = VoxelCursorData {
                            brush: Some(VoxelBrush::new(ui.debug_state.brush_color.clone())),
                            brush_size: ui.debug_state.brush_size,
                            ray_distance: player_voxel_distance,
                            last_voxel: world_voxel_pos,
                        };
                        if input.is_mouse_button_pressed(mouse::Button::Left) {
                            cursor_data.brush = None;
                        }

                        Self::apply_edit(&cursor_data, &mut voxel_world);
                        cursor.data = Some(cursor_data);
                    }
                    VoxelTraceInfo::Entity {
                        entity_id,
                        voxel_model_id,
                        local_voxel_pos,
                    } => {
                        if input.is_mouse_button_pressed(mouse::Button::Left) {
                            assert!(ecs_world.get::<&Transform>(entity_id).is_ok());
                            if cursor.selected_entity.is_some()
                                && cursor.selected_entity.unwrap() == entity_id
                            {
                                cursor.selected_entity = None;
                            } else {
                                cursor.selected_entity = Some(entity_id);
                            }
                        }
                    }
                }
            }
        }

        if let Some(cursor_data) = &mut cursor.data {
            if input.is_mouse_button_down(mouse::Button::Right)
                || input.is_mouse_button_down(mouse::Button::Left)
            {
                let mut player_ray = player_local_transform.get_ray();
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
        if let Some(brush) = &cursor_data.brush {
            let half_length = Vector3::new(3, 3, 3);
            let mut attachment_map = AttachmentMap::new();
            attachment_map.register_attachment(Attachment::PTMATERIAL);
            let edit = VoxelEditInfo {
                world_voxel_position: cursor_data.last_voxel - half_length,
                world_voxel_length: half_length.map(|x| x as u32) * 2 + Vector3::new(1, 1, 1),
                attachment_map,
            };
            let center = half_length.cast::<f32>();
            voxel_world.apply_voxel_edit(edit, |mut voxel, world_position, local_position| {
                let distance = local_position.cast::<f32>().metric_distance(&center);
                if distance <= 4.0 {
                    voxel.set_attachment(
                        Attachment::PTMATERIAL_ID,
                        &[PTMaterial::diffuse(brush.color.clone()).encode()],
                    );
                }
            });
        } else {
            let half_length = Vector3::new(7, 7, 7);
            let edit = VoxelEditInfo {
                world_voxel_position: cursor_data.last_voxel - half_length,
                world_voxel_length: half_length.map(|x| x as u32) * 2 + Vector3::new(1, 1, 1),
                attachment_map: AttachmentMap::new(),
            };
            let center = half_length.cast::<f32>();
            voxel_world.apply_voxel_edit(edit, |mut voxel, world_position, local_position| {
                let distance = local_position.cast::<f32>().metric_distance(&center);
                if distance <= 8.0 {
                    voxel.set_removed();
                }
            });
        }
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

    pub fn create_edit(&self, position: Vector3<i32>, length: Vector3<u32>) -> VoxelEditInfo {
        let mut attachment_map = AttachmentInfoMap::new();
        attachment_map.register_attachment(Attachment::PTMATERIAL);
        VoxelEditInfo {
            world_voxel_position: position,
            world_voxel_length: length,
            attachment_map,
        }
    }
}

pub struct VoxelEditInfo {
    // Minimum bottom-down-left origin.
    pub world_voxel_position: Vector3<i32>,
    // Length in voxels of the edit.
    pub world_voxel_length: Vector3<u32>,
    // Known attachment map so we can skip checking that for each voxel.
    pub attachment_map: AttachmentInfoMap,
}

pub struct VoxelEditEntityInfo {
    pub model_id: VoxelModelId,
    // Minimum bottom-down-left origin.
    pub local_voxel_pos: Vector3<i32>,
    // Length in voxels of the edit.
    pub voxel_length: Vector3<u32>,
    // Known attachment map so we can skip checking that for each voxel.
    pub attachment_map: AttachmentInfoMap,
}

#[derive(Clone)]
pub enum VoxelEditMask {
    Sphere { center: Vector3<i32>, rad_perc: f32 },
    Plane { axis: Vector3<bool> },
}
