use nalgebra::Vector3;
use rogue_engine::{
    common::color::Color,
    consts,
    entity::{
        RenderableVoxelEntity,
        ecs_world::{self, ECSWorld},
    },
    physics::transform::Transform,
    resource::{Res, ResMut, ResourceBank},
    voxel::{
        attachment::Attachment,
        sft_compressed::VoxelModelSFTCompressed,
        voxel::{
            VoxelModelEdit, VoxelModelEditMask, VoxelModelEditMaskLayer,
            VoxelModelEditMaskModelSource, VoxelModelEditMaskSource,
            VoxelModelEditMaskSourceMethods, VoxelModelEditMaskTerrainSource, VoxelModelEditRegion,
            VoxelModelImpl, VoxelModelImplMethods,
        },
        voxel_registry::{VoxelModelId, VoxelModelRegistry},
        voxel_registry_gpu::VoxelModelRegistryGpu,
    },
    world::terrain::region_map::RegionMap,
};
use rogue_macros::Resource;

use crate::{
    editing::{
        voxel_editing::{EditorEditingTool, EditorVoxelEditing, EditorVoxelEditingTarget},
        voxel_editing_edit_tools::EditorVoxelEditingEditTools,
        voxel_editing_preview_gpu::EditorVoxelEditingPreviewGpu,
    },
    session::EditorSession,
};

#[derive(Resource)]
pub struct EditorVoxelEditingPreview {
    preview_model: Option<VoxelModelId>,
    // Tracked to update the gpu model, realistically I could just also update the gpu model here
    pub show_preview: bool,
    preview_model_transform: Transform,
}

impl EditorVoxelEditingPreview {
    pub fn new() -> Self {
        Self {
            preview_model: None,
            show_preview: false,
            preview_model_transform: Transform::new(),
        }
    }

    pub fn update_preview_systems(rb: &ResourceBank) {
        rb.run_system(Self::update_preview_model);
        rb.run_system(Self::update_preview_pencil_tool);
        rb.run_system(Self::update_preview_paint_tool);
        rb.run_system(Self::update_preview_eraser_tool);
    }

    pub fn update_preview_model(
        mut preview: ResMut<EditorVoxelEditingPreview>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
    ) {
        if preview.preview_model.is_none() {
            let mut sft_compressed_model = VoxelModelSFTCompressed::new_empty(4096);
            sft_compressed_model.initialize_attachment_buffers(&Attachment::BMAT);
            preview.preview_model =
                Some(voxel_registry.register_voxel_model(sft_compressed_model, None));
        }
    }

    pub fn update_preview_pencil_tool(
        editing: ResMut<EditorVoxelEditing>,
        mut preview: ResMut<EditorVoxelEditingPreview>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
        editor_session: Res<EditorSession>,
        ecs_world: ResMut<ECSWorld>,
    ) {
        let preview_model_id = preview.preview_model.unwrap();
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        let EditorEditingTool::Pencil {
            brush_size,
            air_place,
        } = tool
        else {
            return;
        };

        let Some(voxel_material) = editing.current_voxel_material() else {
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
                let mut preview_model = voxel_registry.get_dyn_model_mut(preview_model_id);
                let preview_model_side_length = preview_model.length();
                let preview_center = (preview_model_side_length / 2).cast::<i32>();
                let (brush_min, brush_max) = EditorVoxelEditingEditTools::calculate_brush_min_max(
                    preview_center,
                    *brush_size,
                );
                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    preview_model_side_length,
                );

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask: rogue_engine::voxel::voxel::VoxelModelEditMask {
                        layers: vec![VoxelModelEditMaskLayer::Sphere {
                            center: preview_center,
                            diameter: *brush_size,
                        }],
                        mask_source: None,
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        voxel_material,
                    )),
                };

                preview_model.clear();
                preview_model.set_voxel_range_impl(&edit);
                preview.show_preview = true;
                let mut preview_transform = entity_world_transform.clone();
                preview_transform.position += entity_world_transform.rotation.transform_vector(
                    &((hit_pos - (entity_model_side_length / 2).cast::<i32>())
                        .cast::<f32>()
                        .component_mul(&entity_world_transform.scale)
                        * consts::voxel::VOXEL_METER_LENGTH),
                );
                preview.preview_model_transform = preview_transform;
                EditorVoxelEditingPreviewGpu::update_preview_model_gpu(
                    &mut voxel_registry_gpu,
                    preview_model_id,
                );
            }
            Some(EditorVoxelEditingTarget::Terrain) => {
                //
                // Very similar to terrain paint but without the presence mask.
                //
                let Some(raycast) = &editor_session.terrain_raycast else {
                    return;
                };
                let hit_pos =
                    raycast.world_voxel_pos + raycast.model_trace.local_normal.cast::<i32>();

                preview.show_preview = true;

                let preview_model = voxel_registry.get_dyn_model_mut(preview_model_id);
                let preview_model_side_length = preview_model.length();
                let preview_center = (preview_model_side_length / 2).cast::<i32>();
                let (brush_min, brush_max) = EditorVoxelEditingEditTools::calculate_brush_min_max(
                    preview_center,
                    *brush_size,
                );

                // We render the preview at the preview models center, so if we hit voxel (0,0,0),
                // then we render the preview with its min corner at half the voxel length in world
                // space. With this same logic, we apply the same offset when sampling the terrain
                // mask for the preview model edit.
                preview.preview_model_transform = Transform {
                    position: hit_pos.cast::<f32>() * consts::voxel::VOXEL_METER_LENGTH,
                    rotation: nalgebra::UnitQuaternion::identity(),
                    scale: Vector3::new(1.0, 1.0, 1.0),
                };

                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    preview_model_side_length,
                );
                let mask = VoxelModelEditMask {
                    layers: vec![VoxelModelEditMaskLayer::Sphere {
                        center: preview_center,
                        diameter: *brush_size,
                    }],
                    mask_source: None,
                };

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask,
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        voxel_material,
                    )),
                };
                preview_model.clear();
                preview_model.set_voxel_range_impl(&edit);
                EditorVoxelEditingPreviewGpu::update_preview_model_gpu(
                    &mut voxel_registry_gpu,
                    preview_model_id,
                );
            }
            None => {
                return;
            }
        }
    }

    pub fn update_preview_paint_tool(
        editing: ResMut<EditorVoxelEditing>,
        mut preview: ResMut<EditorVoxelEditingPreview>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
        region_map: Res<RegionMap>,
        editor_session: Res<EditorSession>,
        ecs_world: ResMut<ECSWorld>,
    ) {
        let preview_model_id = preview.preview_model.unwrap();
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        let EditorEditingTool::Paint { brush_size } = tool else {
            return;
        };

        let Some(voxel_material) = editing.current_voxel_material() else {
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

                let (entity_transform, renderable) = ecs_world
                    .query_one::<(&Transform, &RenderableVoxelEntity)>(*target_entity)
                    .get()
                    .expect("Target entity should have a renderable model attached.");
                // TODO: Show that its disabled somehow.
                //if !renderable.is_dynamic() {
                //    return;
                //}
                let entity_world_transform =
                    ecs_world.get_world_transform(*target_entity, entity_transform);
                let entity_model_id = renderable
                    .voxel_model_id()
                    .expect("Target entity should have a voxel model");
                let [entity_model, preview_model] =
                    voxel_registry.get_dyn_model_mut_disjoint([entity_model_id, preview_model_id]);
                let entity_model_side_length = entity_model.length();
                let preview_model_side_length = preview_model.length();
                // Don't offset by normal.
                let hit_pos = raycast.model_trace.local_position.cast::<i32>();
                let preview_center = (preview_model_side_length / 2).cast::<i32>();
                let (brush_min, brush_max) = EditorVoxelEditingEditTools::calculate_brush_min_max(
                    preview_center,
                    *brush_size,
                );
                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    preview_model_side_length,
                );

                let mask_model_source = VoxelModelEditMaskModelSource {
                    model: entity_model,
                };
                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask: rogue_engine::voxel::voxel::VoxelModelEditMask {
                        layers: vec![
                            VoxelModelEditMaskLayer::Sphere {
                                center: preview_center,
                                diameter: *brush_size,
                            },
                            VoxelModelEditMaskLayer::Presence,
                        ],
                        mask_source: Some(rogue_engine::voxel::voxel::VoxelModelEditMaskSource {
                            source: &mask_model_source,
                            // I wrote this at one point and now i can't remember why this even
                            // works, but i'll take it.
                            offset: (preview_center - hit_pos).map(|x| x as u32),
                        }),
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        voxel_material,
                    )),
                };

                preview_model.clear();
                preview_model.set_voxel_range_impl(&edit);
                preview.show_preview = true;
                let mut preview_transform = entity_world_transform.clone();
                // Transform preview position onto where hit_pos is in world space, effectively putting the
                // preview models center at this position.
                preview_transform.position += entity_world_transform.rotation.transform_vector(
                    &((hit_pos - (entity_model_side_length / 2).cast::<i32>())
                        .cast::<f32>()
                        .component_mul(&entity_world_transform.scale)
                        * consts::voxel::VOXEL_METER_LENGTH),
                );
                preview.preview_model_transform = preview_transform;
                EditorVoxelEditingPreviewGpu::update_preview_model_gpu(
                    &mut voxel_registry_gpu,
                    preview_model_id,
                );
            }
            Some(EditorVoxelEditingTarget::Terrain) => {
                let Some(raycast) = &editor_session.terrain_raycast else {
                    return;
                };
                let hit_pos = raycast.world_voxel_pos;

                preview.show_preview = true;

                let preview_model = voxel_registry.get_dyn_model(preview_model_id);
                let preview_model_side_length = preview_model.length();
                let preview_center = (preview_model_side_length / 2).cast::<i32>();
                let (brush_min, brush_max) = EditorVoxelEditingEditTools::calculate_brush_min_max(
                    preview_center,
                    *brush_size,
                );

                // We render the preview at the preview models center, so if we hit voxel (0,0,0),
                // then we render the preview with its min corner at half the voxel length in world
                // space. With this same logic, we apply the same offset when sampling the terrain
                // mask for the preview model edit.
                preview.preview_model_transform = Transform {
                    position: hit_pos.cast::<f32>() * consts::voxel::VOXEL_METER_LENGTH,
                    rotation: nalgebra::UnitQuaternion::identity(),
                    scale: Vector3::new(1.0, 1.0, 1.0),
                };

                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    preview_model_side_length,
                );
                let (terrain_min, terrain_max) =
                    EditorVoxelEditingEditTools::calculate_brush_min_max(hit_pos, *brush_size);
                let mut terrain_mask_source =
                    VoxelModelEditMaskTerrainSource::from_voxel_min_max(terrain_min, terrain_max);
                let preview_model = terrain_mask_source.populate_from_registry(
                    &mut voxel_registry,
                    &region_map,
                    preview_model_id,
                );
                let relative_hit_pos =
                    hit_pos - terrain_mask_source.chunk_min().get_min_world_voxel_pos();
                let mask = VoxelModelEditMask {
                    layers: vec![
                        VoxelModelEditMaskLayer::Sphere {
                            center: preview_center,
                            diameter: *brush_size,
                        },
                        VoxelModelEditMaskLayer::Presence,
                    ],
                    mask_source: Some(VoxelModelEditMaskSource {
                        source: &terrain_mask_source,
                        offset: (preview_center - relative_hit_pos).map(|x| x as u32),
                    }),
                };

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask,
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        voxel_material,
                    )),
                };
                preview_model.clear();
                preview_model.set_voxel_range_impl(&edit);
                EditorVoxelEditingPreviewGpu::update_preview_model_gpu(
                    &mut voxel_registry_gpu,
                    preview_model_id,
                );
            }
            None => {
                return;
            }
        }
    }

    /// Basically copy of paint preview.
    pub fn update_preview_eraser_tool(
        editing: ResMut<EditorVoxelEditing>,
        mut preview: ResMut<EditorVoxelEditingPreview>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
        editor_session: Res<EditorSession>,
        ecs_world: ResMut<ECSWorld>,
    ) {
        let preview_model_id = preview.preview_model.unwrap();
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        let EditorEditingTool::Eraser { brush_size } = tool else {
            return;
        };

        const ERASER_COLOR: &str = "#EE2244";
        const ERASER_ALPHA: f32 = 0.2;
        match &editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(target_entity)) => {
                let Some(raycast) = &editor_session.entity_raycast else {
                    return;
                };
                if &raycast.entity != target_entity {
                    return;
                }

                let (entity_transform, renderable) = ecs_world
                    .query_one::<(&Transform, &RenderableVoxelEntity)>(*target_entity)
                    .get()
                    .expect("Target entity should have a renderable model attached.");
                //if !renderable.is_dynamic() {
                //    return;
                //}
                let entity_world_transform =
                    ecs_world.get_world_transform(*target_entity, entity_transform);
                let entity_model_id = renderable
                    .voxel_model_id()
                    .expect("Target entity should have a voxel model");
                let [entity_model, preview_model] =
                    voxel_registry.get_dyn_model_mut_disjoint([entity_model_id, preview_model_id]);
                let entity_model_side_length = entity_model.length();
                let preview_model_side_length = preview_model.length();
                // Don't offset by normal.
                let hit_pos = raycast.model_trace.local_position.cast::<i32>();
                let preview_center = (preview_model_side_length / 2).cast::<i32>();
                let (brush_min, brush_max) = EditorVoxelEditingEditTools::calculate_brush_min_max(
                    preview_center,
                    *brush_size,
                );
                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    preview_model_side_length,
                );

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask: rogue_engine::voxel::voxel::VoxelModelEditMask {
                        layers: vec![VoxelModelEditMaskLayer::Sphere {
                            center: preview_center,
                            diameter: *brush_size,
                        }],
                        mask_source: None,
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        rogue_engine::voxel::voxel::VoxelMaterialData::Baked {
                            color: Color::new_srgba_hex(ERASER_COLOR, ERASER_ALPHA),
                        },
                    )),
                };

                preview_model.clear();
                preview_model.set_voxel_range_impl(&edit);
                preview.show_preview = true;
                let mut preview_transform = entity_world_transform.clone();
                // Transform preview position onto where hit_pos is in world space, effectively putting the
                // preview models center at this position.
                preview_transform.position += entity_world_transform.rotation.transform_vector(
                    &((hit_pos - (entity_model_side_length / 2).cast::<i32>())
                        .cast::<f32>()
                        .component_mul(&entity_world_transform.scale)
                        * consts::voxel::VOXEL_METER_LENGTH),
                );
                preview.preview_model_transform = preview_transform;
                EditorVoxelEditingPreviewGpu::update_preview_model_gpu(
                    &mut voxel_registry_gpu,
                    preview_model_id,
                );
            }
            Some(EditorVoxelEditingTarget::Terrain) => {
                //
                // Literally same as pencil tool but different material.
                //
                let Some(raycast) = &editor_session.terrain_raycast else {
                    return;
                };
                let hit_pos = raycast.world_voxel_pos;

                preview.show_preview = true;

                let preview_model = voxel_registry.get_dyn_model_mut(preview_model_id);
                let preview_model_side_length = preview_model.length();
                let preview_center = (preview_model_side_length / 2).cast::<i32>();
                let (brush_min, brush_max) = EditorVoxelEditingEditTools::calculate_brush_min_max(
                    preview_center,
                    *brush_size,
                );

                // We render the preview at the preview models center, so if we hit voxel (0,0,0),
                // then we render the preview with its min corner at half the voxel length in world
                // space. With this same logic, we apply the same offset when sampling the terrain
                // mask for the preview model edit.
                preview.preview_model_transform = Transform {
                    position: hit_pos.cast::<f32>() * consts::voxel::VOXEL_METER_LENGTH,
                    rotation: nalgebra::UnitQuaternion::identity(),
                    scale: Vector3::new(1.0, 1.0, 1.0),
                };

                let brush_edit_rect = VoxelModelEditRegion::saturate_rect(
                    brush_min,
                    brush_max,
                    preview_model_side_length,
                );
                let mask = VoxelModelEditMask {
                    layers: vec![VoxelModelEditMaskLayer::Sphere {
                        center: preview_center,
                        diameter: *brush_size,
                    }],
                    mask_source: None,
                };

                let edit = VoxelModelEdit {
                    region: brush_edit_rect,
                    mask,
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(Some(
                        rogue_engine::voxel::voxel::VoxelMaterialData::Baked {
                            color: Color::new_srgba_hex(ERASER_COLOR, ERASER_ALPHA),
                        },
                    )),
                };
                preview_model.clear();
                preview_model.set_voxel_range_impl(&edit);
                EditorVoxelEditingPreviewGpu::update_preview_model_gpu(
                    &mut voxel_registry_gpu,
                    preview_model_id,
                );
            }
            None => {
                return;
            }
        }
    }

    pub fn should_show_preview(&self) -> bool {
        self.show_preview
    }

    pub fn preview_model(&self) -> Option<VoxelModelId> {
        self.preview_model
    }

    pub fn preview_model_transform(&self) -> &Transform {
        &self.preview_model_transform
    }
}
