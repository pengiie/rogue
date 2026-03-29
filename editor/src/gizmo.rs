use nalgebra::Vector3;
use rogue_engine::{
    common::color::{Color, ColorSrgba},
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

/// Tool for modifying the currently selected entity.
#[derive(Resource)]
pub struct EditorGizmo {
    hovering_gizmo: bool,
    dragging_axis: Vector3<bool>,
}

impl EditorGizmo {
    pub fn new() -> Self {
        Self {
            hovering_gizmo: false,
            dragging_axis: Vector3::new(false, false, false),
        }
    }

    pub fn is_hovering(&self) -> bool {
        self.hovering_gizmo
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging_axis.x || self.dragging_axis.y || self.dragging_axis.z
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

        // Calculate which translation axis is currently being hovered.
        const SCALE: f32 = 0.5;
        let hover_x = debug_renderer.raycast_arrow(
            &editor_session.editor_camera_ray,
            world_transform.position,
            world_transform.position + Vector3::x(),
            SCALE,
        );
        let hover_y = debug_renderer.raycast_arrow(
            &editor_session.editor_camera_ray,
            world_transform.position,
            world_transform.position + Vector3::y(),
            SCALE,
        );
        let hover_z = debug_renderer.raycast_arrow(
            &editor_session.editor_camera_ray,
            world_transform.position,
            world_transform.position + Vector3::z(),
            SCALE,
        );
        let hover_axis_vals = Vector3::new(
            hover_x.unwrap_or(f32::MAX),
            hover_y.unwrap_or(f32::MAX),
            hover_z.unwrap_or(f32::MAX),
        );
        let hover_min = hover_axis_vals.min();
        let hover_axis = hover_axis_vals.map(|x| x != f32::MAX && x == hover_min);
        gizmo.hovering_gizmo = hover_min != f32::MAX;
        if !gizmo.is_dragging()
            && gizmo.hovering_gizmo
            && input.is_mouse_button_pressed(mouse::Button::Left)
        {
            gizmo.dragging_axis = hover_axis;
        }

        // Calculate axes colors.
        let mut x_color = Color::new_srgba(1.0, 0.0, 0.0, 1.0);
        let mut y_color = Color::new_srgba(0.0, 1.0, 0.0, 1.0);
        let mut z_color = Color::new_srgba(0.0, 0.0, 1.0, 1.0);
        if hover_axis.x || gizmo.dragging_axis.x {
            x_color = x_color.mix_white(0.5);
        } else if hover_axis.y || gizmo.dragging_axis.y {
            y_color = y_color.mix_white(0.5);
        } else if hover_axis.z || gizmo.dragging_axis.z {
            z_color = z_color.mix_white(0.5);
        }

        // Draw translation arrows.
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::x(),
            SCALE,
            x_color,
            DebugShapeFlags::NONE,
        );
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::y(),
            SCALE,
            y_color,
            DebugShapeFlags::NONE,
        );
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::z(),
            SCALE,
            z_color,
            DebugShapeFlags::NONE,
        );

        let mouse_delta = input.mouse_delta();
        if gizmo.is_dragging() && (mouse_delta.x != 0.0 || mouse_delta.y != 0.0) {
            let backbuffer_size = editor_ui.backbuffer_size(&window).cast::<f32>();
            let mut world_transform = {
                let mut local_transform = ecs_world
                    .get::<&Transform>(selected_entity)
                    .expect("Should have a transform");
                let mut world_transform =
                    ecs_world.get_world_transform(selected_entity, &local_transform);
                const DRAGGING_SENS: f32 = 0.001;
                let diff = world_transform.position - camera_transform.position;
                // Projection along of axis scaled by 1.0 / z and we scale mouse pixel delta by half
                // our window size vertically since that represent 1.0 ndc. Distance in this case
                // isn't exactly our z distance so may overshoot but that is okay.
                let drag_speed = diff.norm() / (backbuffer_size.y * 0.5);
                // Camera faces -Z.
                if gizmo.dragging_axis.x {
                    world_transform.position.x +=
                        mouse_delta.x * drag_speed * diff.dot(&Vector3::z()).signum();
                    world_transform.position.x +=
                        mouse_delta.y * drag_speed * diff.dot(&Vector3::z()).signum();
                } else if gizmo.dragging_axis.y {
                    world_transform.position.y += mouse_delta.y * drag_speed;
                } else if gizmo.dragging_axis.z {
                    world_transform.position.z +=
                        mouse_delta.x * drag_speed * diff.dot(&-Vector3::x()).signum();
                    world_transform.position.z +=
                        -mouse_delta.y * drag_speed * diff.dot(&-Vector3::x()).signum();
                }
                world_transform
            };
            let new_local_transform =
                ecs_world.get_world_to_local_transform(selected_entity, &world_transform);
            *ecs_world
                .get::<&mut Transform>(selected_entity)
                .expect("Should have a transform") = new_local_transform;
        }

        if input.is_mouse_button_released(mouse::Button::Left) {
            gizmo.dragging_axis = Vector3::new(false, false, false);
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
