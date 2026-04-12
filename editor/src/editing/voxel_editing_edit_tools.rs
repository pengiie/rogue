use nalgebra::Vector3;
use rogue_engine::{
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    event::Events,
    input::{Input, mouse},
    physics::transform::Transform,
    resource::{Res, ResMut, ResourceBank},
    voxel::{
        voxel::{VoxelModelEdit, VoxelModelEditMaskLayer, VoxelModelEditRegion},
        voxel_registry::VoxelModelRegistry,
    },
    world::terrain::region_map::{
        RegionMap, VoxelTerrainEdit, VoxelTerrainEditMask, VoxelTerrainEditMaskLayer,
        VoxelTerrainRegion,
    },
};
use rogue_macros::Resource;

use crate::{
    editing::{
        voxel_editing::{
            EditorEditingTool, EditorEditingToolType, EditorVoxelEditing, EditorVoxelEditingTarget,
        },
        voxel_editing_selection::EditorVoxelEditingSelections,
    },
    session::EditorSession,
};

/// Handles the application of tools that performs some sort of edit, like make the pencil apply
/// voxels to its target. Or eraser erases voxels.
#[derive(Resource)]
pub struct EditorVoxelEditingEditTools {
    paint: EditorVoxelEditingPaintState,
}

pub struct EditorVoxelEditingPaintState {
    is_down: bool,
}

impl EditorVoxelEditingEditTools {
    pub fn new() -> Self {
        Self {
            paint: EditorVoxelEditingPaintState { is_down: false },
        }
    }

    pub fn update_edit_application_systems(rb: &ResourceBank) {
        if rb.get_resource::<EditorVoxelEditing>().is_click_consumed {
            return;
        }
        rb.run_system(Self::update_pencil_tool);
        rb.run_system(Self::update_paint_tool);
        rb.run_system(Self::update_eraser_tool);
    }

    fn update_pencil_tool(
        mut editing: ResMut<EditorVoxelEditing>,
        editing_selection: Res<EditorVoxelEditingSelections>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        ecs_world: Res<ECSWorld>,
        mut region_map: ResMut<RegionMap>,
        input: Res<Input>,
        editor_session: Res<EditorSession>,
        mut events: ResMut<Events>,
    ) {
        if !input.is_mouse_button_pressed(mouse::Button::Left) {
            return;
        }
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        let EditorEditingTool::Pencil {
            brush_size,
            air_place,
        } = tool
        else {
            return;
        };

        match &editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(target_entity)) => {
                let valid_raycast_hit = &editor_session
                    .entity_raycast
                    .as_ref()
                    .filter(|hit| hit.entity == *target_entity);
                let entity_transform = ecs_world
                    .get::<&Transform>(*target_entity)
                    .expect("Target entity should have a renderable model attached.");
                let entity_world_transform =
                    ecs_world.get_world_transform(*target_entity, &entity_transform);
                let renderable = ecs_world
                    .get::<&RenderableVoxelEntity>(*target_entity)
                    .expect("Target entity should have a renderable model attached.");
                if !renderable.is_dynamic() {
                    return;
                }
                let entity_model_id = renderable
                    .voxel_model_id()
                    .expect("Target entity should have a voxel model");
                let entity_model = voxel_registry.get_dyn_model_mut(entity_model_id);
                let entity_model_side_length = entity_model.length();
                let entity_model_obb =
                    entity_world_transform.as_voxel_model_obb(entity_model_side_length);
                let ray = &editor_session.editor_camera_ray;

                let (mut hit_pos, hit_normal) = if let Some(raycast_hit) = valid_raycast_hit {
                    (
                        raycast_hit.model_trace.local_position,
                        raycast_hit.model_trace.local_normal,
                    )
                } else if *air_place && let Some(hit_info) = ray.intersect_obb(&entity_model_obb) {
                    let center = entity_world_transform.position;
                    let inv_rot = entity_model_obb.rotation.inverse();
                    let rotated_ray_pos = inv_rot.transform_vector(&(ray.origin - center)) + center;
                    let rotated_ray_dir = inv_rot.transform_vector(&ray.dir);
                    let exit_pos = rotated_ray_pos + rotated_ray_dir * hit_info.t_exit;
                    let norm_pos = (exit_pos - entity_model_obb.aabb.min)
                        .component_div(&entity_model_obb.aabb.side_length())
                        .map(|x| x.clamp(0.0, 1.0));
                    let exit_voxel = norm_pos
                        .component_mul(&entity_model_side_length.cast::<f32>())
                        .map(|x| x.floor() as u32);
                    (exit_voxel, Vector3::new(0, 0, 0))
                } else {
                    return;
                };

                let hit_pos = hit_pos.cast::<i32>() + hit_normal;
                let (brush_min, brush_max) = Self::calculate_brush_min_max(hit_pos, *brush_size);
                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    entity_model_side_length,
                );

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask: rogue_engine::voxel::voxel::VoxelModelEditMask {
                        layers: vec![VoxelModelEditMaskLayer::Sphere {
                            center: hit_pos,
                            diameter: *brush_size,
                        }],
                        mask_source: None,
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        editing.current_voxel_material(),
                    )),
                };
                editing.apply_entity_edit(
                    &mut voxel_registry,
                    &mut events,
                    edit,
                    entity_model_id,
                    true,
                );
            }
            Some(EditorVoxelEditingTarget::Terrain) => {
                let Some(raycast) = &editor_session.terrain_raycast else {
                    return;
                };

                let hit_pos =
                    raycast.world_voxel_pos + raycast.model_trace.local_normal.cast::<i32>();
                let (brush_min, brush_max) = Self::calculate_brush_min_max(hit_pos, *brush_size);
                let edit = VoxelTerrainEdit {
                    region: VoxelTerrainRegion::new_rect(brush_min, brush_max),
                    mask: VoxelTerrainEditMask {
                        layers: vec![VoxelTerrainEditMaskLayer(VoxelModelEditMaskLayer::Sphere {
                            center: hit_pos,
                            diameter: *brush_size,
                        })],
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        editing.current_voxel_material(),
                    )),
                };
                editing.apply_terrain_edit(&mut region_map, &mut voxel_registry, edit, true);
            }
            None => {
                return;
            }
        }
    }

    fn update_paint_tool(
        mut edit_tools: ResMut<EditorVoxelEditingEditTools>,
        mut editing: ResMut<EditorVoxelEditing>,
        editing_selection: Res<EditorVoxelEditingSelections>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        ecs_world: Res<ECSWorld>,
        mut region_map: ResMut<RegionMap>,
        input: Res<Input>,
        editor_session: Res<EditorSession>,
        mut events: ResMut<Events>,
    ) {
        let mut save_history = false;
        if input.is_mouse_button_pressed(mouse::Button::Left) {
            edit_tools.paint.is_down = true;
            save_history = true;
        }
        if !input.is_mouse_button_down(mouse::Button::Left) {
            edit_tools.paint.is_down = false;
        }
        if !edit_tools.paint.is_down {
            return;
        }

        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        let EditorEditingTool::Paint { brush_size } = tool else {
            return;
        };

        match &editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(target_entity)) => {
                let Some(raycast) = &editor_session.entity_raycast else {
                    return;
                };
                if &raycast.entity != target_entity {
                    return;
                }
                let renderable = ecs_world
                    .get::<&RenderableVoxelEntity>(*target_entity)
                    .expect("Target entity should have a renderable model attached.");
                if !renderable.is_dynamic() {
                    return;
                }
                let entity_model_id = renderable
                    .voxel_model_id()
                    .expect("Target entity should have a voxel model");
                let entity_model = voxel_registry.get_dyn_model_mut(entity_model_id);
                let entity_model_side_length = entity_model.length();
                let hit_pos = raycast.model_trace.local_position.cast::<i32>();
                let (brush_min, brush_max) = Self::calculate_brush_min_max(hit_pos, *brush_size);
                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    entity_model_side_length,
                );

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask: rogue_engine::voxel::voxel::VoxelModelEditMask {
                        layers: vec![
                            VoxelModelEditMaskLayer::Sphere {
                                center: hit_pos,
                                diameter: *brush_size,
                            },
                            VoxelModelEditMaskLayer::Presence,
                        ],
                        mask_source: None,
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        editing.current_voxel_material(),
                    )),
                };
                editing.apply_entity_edit(
                    &mut voxel_registry,
                    &mut events,
                    edit,
                    entity_model_id,
                    save_history,
                );
            }
            Some(EditorVoxelEditingTarget::Terrain) => {
                let Some(raycast) = &editor_session.terrain_raycast else {
                    return;
                };

                let hit_pos = raycast.world_voxel_pos;
                let (brush_min, brush_max) = Self::calculate_brush_min_max(hit_pos, *brush_size);
                let edit = VoxelTerrainEdit {
                    region: VoxelTerrainRegion::new_rect(brush_min, brush_max),
                    mask: VoxelTerrainEditMask {
                        layers: vec![
                            VoxelTerrainEditMaskLayer(VoxelModelEditMaskLayer::Sphere {
                                center: hit_pos,
                                diameter: *brush_size,
                            }),
                            VoxelTerrainEditMaskLayer(VoxelModelEditMaskLayer::Presence),
                        ],
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        editing.current_voxel_material(),
                    )),
                };
                editing.apply_terrain_edit(
                    &mut region_map,
                    &mut voxel_registry,
                    edit,
                    save_history,
                );
            }
            None => {
                return;
            }
        }
    }

    fn update_eraser_tool(
        mut editing: ResMut<EditorVoxelEditing>,
        editing_selection: Res<EditorVoxelEditingSelections>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        ecs_world: Res<ECSWorld>,
        input: Res<Input>,
        mut region_map: ResMut<RegionMap>,
        editor_session: Res<EditorSession>,
        mut events: ResMut<Events>,
    ) {
        if !input.is_mouse_button_pressed(mouse::Button::Left) {
            return;
        }
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        let EditorEditingTool::Eraser { brush_size } = tool else {
            return;
        };

        match &editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(target_entity)) => {
                let Some(raycast) = &editor_session.entity_raycast else {
                    return;
                };
                if &raycast.entity != target_entity {
                    return;
                }
                let renderable = ecs_world
                    .get::<&RenderableVoxelEntity>(*target_entity)
                    .expect("Target entity should have a renderable model attached.");
                if !renderable.is_dynamic() {
                    return;
                }
                let entity_model_id = renderable
                    .voxel_model_id()
                    .expect("Target entity should have a voxel model");
                let entity_model = voxel_registry.get_dyn_model_mut(entity_model_id);
                let entity_model_side_length = entity_model.length();
                let hit_pos = raycast.model_trace.local_position.cast::<i32>();
                let (brush_min, brush_max) = Self::calculate_brush_min_max(hit_pos, *brush_size);
                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    entity_model_side_length,
                );

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask: rogue_engine::voxel::voxel::VoxelModelEditMask {
                        layers: vec![VoxelModelEditMaskLayer::Sphere {
                            center: hit_pos,
                            diameter: *brush_size,
                        }],
                        mask_source: None,
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(None),
                };
                editing.apply_entity_edit(
                    &mut voxel_registry,
                    &mut events,
                    edit,
                    entity_model_id,
                    true,
                );
            }
            Some(EditorVoxelEditingTarget::Terrain) => {
                let Some(raycast) = &editor_session.terrain_raycast else {
                    return;
                };

                let hit_pos = raycast.world_voxel_pos;
                let (brush_min, brush_max) = Self::calculate_brush_min_max(hit_pos, *brush_size);
                let edit = VoxelTerrainEdit {
                    region: VoxelTerrainRegion::new_rect(brush_min, brush_max),
                    mask: VoxelTerrainEditMask {
                        layers: vec![VoxelTerrainEditMaskLayer(VoxelModelEditMaskLayer::Sphere {
                            center: hit_pos,
                            diameter: *brush_size,
                        })],
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(None),
                };
                editing.apply_terrain_edit(&mut region_map, &mut voxel_registry, edit, true);
            }
            None => {
                return;
            }
        }
    }

    pub fn calculate_brush_min_max(
        hit_pos: Vector3<i32>,
        brush_size: u32,
    ) -> (/*min*/ Vector3<i32>, /*max*/ Vector3<i32>) {
        let br = brush_size / 2;
        let br_i32 = br as i32;
        let min = if brush_size % 2 == 0 {
            hit_pos.map(|x| x - br.saturating_sub(1) as i32)
        } else {
            hit_pos.map(|x| x - br_i32)
        };
        let max = hit_pos.map(|x| x + br_i32);
        (min, max)
    }
}
