use nalgebra::{UnitQuaternion, Vector3};
use rogue_engine::{
    common::{
        color::{Color, ColorSrgba},
        geometry::ray::Ray,
    },
    debug::debug_renderer::{DebugRenderer, DebugShapeFlags},
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    graphics::camera::MainCamera,
    input::{Input, mouse},
    physics::{
        collider_component::EntityColliders, physics_world::PhysicsWorld, transform::Transform,
    },
    resource::{Res, ResMut},
    voxel::voxel_registry::VoxelModelRegistry,
    window::window::Window,
};
use rogue_macros::Resource;

use crate::{editing::voxel_editing::EditorVoxelEditing, session::EditorSession, ui::EditorUI};

#[derive(Copy, Clone, strum_macros::EnumDiscriminants)]
enum GizmoType {
    Translation { start_proj: f32 },
    Rotation { last_rot: f32 },
}

struct ActiveGizmo {
    gizmo_type: GizmoType,
    axis: Vector3<f32>,
    initial_entity_pos: Vector3<f32>,
}

impl ActiveGizmo {
    pub fn plane_axes(axis: &Vector3<f32>) -> (Vector3<f32>, Vector3<f32>) {
        if axis.x != 0.0 {
            (Vector3::y(), Vector3::z())
        } else if axis.y != 0.0 {
            (Vector3::x(), Vector3::z())
        } else {
            (Vector3::x(), Vector3::y())
        }
    }

    pub fn plane_proj_axis(pos: &Vector3<f32>, axis: &Vector3<f32>, ray: &Ray) -> Option<f32> {
        let (pa, pb) = Self::plane_axes(axis);
        let pta = ray.intersect_plane(*pos, pa);
        let ptb = ray.intersect_plane(*pos, pb);
        let pt = match (pta, ptb) {
            (Some(ta), Some(tb)) => ta.min(tb),
            (Some(ta), None) => ta,
            (None, Some(tb)) => tb,
            (None, None) => {
                return None;
            }
        };
        let hit_pos = ray.origin + pt * ray.dir;
        let proj = (hit_pos - pos).dot(axis);
        return Some(proj);
    }

    pub fn plane_proj_rotation(
        pos: &Vector3<f32>,
        axis: &Vector3<f32>,
        ray: &Ray,
    ) -> Option</*rotation*/ f32> {
        let Some(pt) = ray.intersect_plane(*pos, *axis) else {
            return None;
        };
        let hit_pos = ray.origin + pt * ray.dir;
        let diff = hit_pos - pos;
        if axis.x != 0.0 {
            Some(diff.y.atan2(diff.z))
        } else if axis.y != 0.0 {
            Some(diff.z.atan2(diff.x))
        } else {
            Some(diff.x.atan2(diff.y))
        }
    }

    pub fn apply_update(&mut self, ray: &Ray, world_transform: &mut Transform) {
        match &mut self.gizmo_type {
            GizmoType::Translation { start_proj } => {
                let Some(mut proj) =
                    Self::plane_proj_axis(&self.initial_entity_pos, &self.axis, ray)
                else {
                    return;
                };
                proj -= *start_proj;
                world_transform.position = self.initial_entity_pos + self.axis * proj;
            }
            GizmoType::Rotation { last_rot } => {
                let Some(mut rot) =
                    Self::plane_proj_rotation(&self.initial_entity_pos, &self.axis, ray)
                else {
                    return;
                };
                let drot = rot - *last_rot;
                *last_rot = rot;
                let rot_quat = UnitQuaternion::from_axis_angle(
                    &nalgebra::Unit::new_unchecked(self.axis),
                    -drot,
                );
                world_transform.rotation *= rot_quat;
            }
        }
    }
}

/// Tool for modifying the currently selected entity.
#[derive(Resource)]
pub struct EditorGizmo {
    hovering_gizmo: bool,
    active_gizmo: Option<ActiveGizmo>,
}

impl EditorGizmo {
    pub fn new() -> Self {
        Self {
            hovering_gizmo: false,
            active_gizmo: None,
        }
    }

    pub fn is_hovering(&self) -> bool {
        self.hovering_gizmo
    }

    pub fn update(
        mut gizmo: ResMut<EditorGizmo>,
        mut editor_session: ResMut<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
        main_camera: Res<MainCamera>,
        voxel_editing: Res<EditorVoxelEditing>,
        input: Res<Input>,
        editor_ui: Res<EditorUI>,
        window: Res<Window>,
    ) {
        gizmo.hovering_gizmo = false;
        if voxel_editing.is_enabled() || !editor_session.is_editor_camera_focused() {
            return;
        }
        let Some(selected_entity) = editor_session.selected_entity else {
            return;
        };
        let camera_transform = ecs_world
            .get::<&Transform>(editor_session.editor_camera())
            .expect("Editor camera should have a transform");
        let camera_rot = editor_session.editor_camera_controller().euler;

        let world_transform = {
            let local_transform = ecs_world
                .get::<&Transform>(selected_entity)
                .expect("Should have a transform");
            ecs_world.get_world_transform(selected_entity, &local_transform)
        };

        struct AxisInfo {
            hover_t: Option<f32>,
            axis: Vector3<f32>,
            gizmo_type: GizmoTypeDiscriminants,
            color: ColorSrgba,
        }
        const TRANSLATION_SCALE: f32 = 0.3;
        let create_translation_axis = |debug_renderer: &mut DebugRenderer,
                                       axis: Vector3<f32>,
                                       color: ColorSrgba|
         -> AxisInfo {
            let hover_t = debug_renderer.raycast_arrow(
                &editor_session.editor_camera_ray,
                world_transform.position,
                world_transform.position + axis,
                TRANSLATION_SCALE,
            );
            AxisInfo {
                hover_t,
                axis,
                gizmo_type: GizmoTypeDiscriminants::Translation,
                color,
            }
        };
        const RADIUS: f32 = 0.6;
        const THICKNESS: f32 = 0.02;
        let mut create_rotation_axis = |debug_renderer: &mut DebugRenderer,
                                        axis: Vector3<f32>,
                                        color: ColorSrgba|
         -> AxisInfo {
            let rot = world_transform.rotation
                * UnitQuaternion::rotation_between(&Vector3::y(), &axis).unwrap();
            let hover_t = debug_renderer.raycast_ring(
                &editor_session.editor_camera_ray,
                world_transform.position,
                rot,
                RADIUS,
                THICKNESS,
            );
            AxisInfo {
                hover_t,
                axis,
                gizmo_type: GizmoTypeDiscriminants::Rotation,
                color,
            }
        };
        const ALPHA: f32 = 0.6;
        let dr = &mut *debug_renderer;
        let mut axes = [
            create_translation_axis(dr, Vector3::x(), Color::new_srgba(1.0, 0.0, 0.0, ALPHA)),
            create_translation_axis(dr, Vector3::y(), Color::new_srgba(0.0, 1.0, 0.0, ALPHA)),
            create_translation_axis(dr, Vector3::z(), Color::new_srgba(0.0, 0.0, 1.0, ALPHA)),
            create_rotation_axis(dr, Vector3::x(), Color::new_srgba(1.0, 0.0, 0.0, ALPHA)),
            create_rotation_axis(dr, Vector3::y(), Color::new_srgba(0.0, 1.0, 0.0, ALPHA)),
            create_rotation_axis(dr, Vector3::z(), Color::new_srgba(0.0, 0.0, 1.0, ALPHA)),
        ];
        let mut closest_axis = None;
        let mut closest_t = None;
        for axis in &mut axes {
            if let Some(t) = axis.hover_t {
                if t < closest_t.unwrap_or(f32::MAX) {
                    closest_t = Some(t);
                    closest_axis = Some(axis);
                }
            }
        }

        gizmo.hovering_gizmo = closest_axis.is_some();
        if gizmo.active_gizmo.is_none()
            && let Some(axis) = closest_axis
        {
            axis.color = axis.color.mix_white(0.5);
            if input.is_mouse_button_pressed(mouse::Button::Left) {
                let gizmo_type = match axis.gizmo_type {
                    GizmoTypeDiscriminants::Translation => GizmoType::Translation {
                        start_proj: ActiveGizmo::plane_proj_axis(
                            &world_transform.position,
                            &axis.axis,
                            &editor_session.editor_camera_ray,
                        )
                        .unwrap_or(0.0),
                    },
                    GizmoTypeDiscriminants::Rotation => GizmoType::Rotation {
                        last_rot: ActiveGizmo::plane_proj_rotation(
                            &world_transform.position,
                            &axis.axis,
                            &editor_session.editor_camera_ray,
                        )
                        .unwrap_or(0.0),
                    },
                };
                gizmo.active_gizmo = Some(ActiveGizmo {
                    gizmo_type,
                    axis: axis.axis,
                    initial_entity_pos: world_transform.position,
                });
            }
        }

        for axis in axes {
            match axis.gizmo_type {
                GizmoTypeDiscriminants::Translation => {
                    debug_renderer.draw_arrow(
                        world_transform.position,
                        world_transform.position + axis.axis,
                        TRANSLATION_SCALE,
                        axis.color,
                        DebugShapeFlags::NONE,
                    );
                }
                GizmoTypeDiscriminants::Rotation => {
                    let rot = world_transform.rotation
                        * UnitQuaternion::rotation_between(&Vector3::y(), &axis.axis).unwrap();
                    debug_renderer.draw_ring(
                        world_transform.position,
                        rot,
                        RADIUS,
                        THICKNESS,
                        axis.color,
                        DebugShapeFlags::NONE,
                    );
                }
            }
        }

        if let Some(active_gizmo) = &mut gizmo.active_gizmo {
            let backbuffer_size = editor_ui.backbuffer_size(&window).cast::<f32>();
            let mut world_transform = {
                let mut local_transform = ecs_world
                    .get::<&Transform>(selected_entity)
                    .expect("Should have a transform");
                let mut world_transform =
                    ecs_world.get_world_transform(selected_entity, &local_transform);
                // Apply translation/rotation/scale.
                active_gizmo.apply_update(&editor_session.editor_camera_ray, &mut world_transform);
                world_transform
            };
            let new_local_transform =
                ecs_world.get_world_to_local_transform(selected_entity, &world_transform);
            *ecs_world
                .get::<&mut Transform>(selected_entity)
                .expect("Should have a transform") = new_local_transform;
        }

        if input.is_mouse_button_released(mouse::Button::Left) {
            gizmo.active_gizmo = None;
        }
    }

    pub fn visualize_selected_entity(
        mut editor_session: ResMut<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        main_camera: Res<MainCamera>,
        voxel_editing: Res<EditorVoxelEditing>,
        physics_world: Res<PhysicsWorld>,
    ) {
        if voxel_editing.is_enabled() || !editor_session.is_editor_camera_focused() {
            return;
        }
        const SELECTION_COLOR: &'static str = "#ffffff";
        let Some(selected_entity) = editor_session.selected_entity else {
            return;
        };

        let local_transform = ecs_world
            .get::<&Transform>(selected_entity)
            .expect("Should have a transform");
        let world_transform = ecs_world.get_world_transform(selected_entity, &local_transform);

        let color = Color::new_srgba_hex(SELECTION_COLOR, 1.0);
        if let Ok(renderable) = ecs_world.get::<&RenderableVoxelEntity>(selected_entity)
            && let Some(model_id) = renderable.voxel_model_id()
        {
            let side_length = voxel_registry.get_dyn_model(model_id).length();
            let obb = world_transform.as_voxel_model_obb(side_length);
            debug_renderer.draw_obb_outline(
                &obb,
                0.025 * world_transform.scale.min(),
                color,
                DebugShapeFlags::NONE,
            );
        } else {
            //debug_renderer.draw_sphere(world_transform.position, 0.2, color, DebugShapeFlags::NONE);
        }

        if let Ok(colliders) = ecs_world.get::<&EntityColliders>(selected_entity) {
            for collider_id in &colliders.colliders {
                physics_world
                    .colliders
                    .get_collider_dyn(collider_id)
                    .render_debug(
                        &world_transform,
                        &mut debug_renderer,
                        ColorSrgba::new_srgb_hex("#22FF22", 0.1),
                    );
            }
        }
    }
}
