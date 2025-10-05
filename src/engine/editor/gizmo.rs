use nalgebra::{UnitQuaternion, Vector3};

use crate::common::geometry::ray::Ray;
use crate::{
    common::color::Color,
    consts,
    engine::{
        debug::{DebugFlags, DebugLine, DebugRenderer, DebugRing},
        entity::{
            ecs_world::{ECSWorld, Entity},
            RenderableVoxelEntity,
        },
        input::{mouse, Input},
        physics::transform::Transform,
    },
};

pub enum EditorGizmoType {
    Translate,
    Rotate,
    Scale,
}

pub struct EditorGizmo {
    pub selected_gizmo: EditorGizmoType,

    pub dragging_gizmo_axis: Option<Vector3<f32>>,
    pub hover_gizmo_axis: Option<Vector3<f32>>,
}

impl EditorGizmo {
    pub fn new() -> Self {
        Self {
            selected_gizmo: EditorGizmoType::Translate,

            dragging_gizmo_axis: None,
            hover_gizmo_axis: None,
        }
    }

    pub fn update_gizmo_selection(&mut self, input: &Input) {
        if input.is_action_pressed(consts::actions::EDITOR_GIZMO_TRANSLATION) {
            self.selected_gizmo = EditorGizmoType::Translate;
        }
        if input.is_action_pressed(consts::actions::EDITOR_GIZMO_ROTATION) {
            self.selected_gizmo = EditorGizmoType::Rotate;
        }
    }

    pub fn update_gizmo_axes(
        &mut self,
        input: &Input,
        selected_entity: Entity,
        consumes_left_click: &mut bool,
        ecs_world: &ECSWorld,
        mouse_ray: &Ray,
    ) {
        if input.is_mouse_button_released(mouse::Button::Left) {
            self.dragging_gizmo_axis = None;
        }

        let mut selected_entity_query = ecs_world.query_one::<(&Transform)>(selected_entity);
        let Some((model_local_transform)) = selected_entity_query.get() else {
            return;
        };
        let model_world_transform =
            ecs_world.get_world_transform(selected_entity, model_local_transform);
        let center = model_world_transform.position;

        let mut min_d = 1000.0;
        let mut axis = None;
        match self.selected_gizmo {
            EditorGizmoType::Translate => {
                if let Some(d) = mouse_ray.intersect_line_segment(
                    center,
                    center + Vector3::x() * consts::editor::gizmo::TRANSLATION_LENGTH,
                    consts::editor::gizmo::TRANSLATION_THICKNESS,
                    1000.0,
                ) {
                    axis = Some(Vector3::x());
                    min_d = d;
                }
                if let Some(d) = mouse_ray.intersect_line_segment(
                    center,
                    center + Vector3::y() * consts::editor::gizmo::TRANSLATION_LENGTH,
                    consts::editor::gizmo::TRANSLATION_THICKNESS,
                    1000.0,
                ) {
                    if (d < min_d) {
                        axis = Some(Vector3::y());
                        min_d = d;
                    }
                }
                if let Some(d) = mouse_ray.intersect_line_segment(
                    center,
                    center + Vector3::z() * consts::editor::gizmo::TRANSLATION_LENGTH,
                    consts::editor::gizmo::TRANSLATION_THICKNESS,
                    1000.0,
                ) {
                    if (d < min_d) {
                        axis = Some(Vector3::z());
                        min_d = d;
                    }
                }
            }
            EditorGizmoType::Rotate => {
                let thickness = consts::editor::gizmo::ROTATION_THICKNESS;
                let max_t = 1000.0;
                if let Some(d) = mouse_ray.intersect_ring_segment(
                    center,
                    model_local_transform
                        .rotation
                        .transform_vector(&Vector3::x()),
                    consts::editor::gizmo::ROTATION_DISTANCE_X,
                    thickness,
                    max_t,
                ) {
                    axis = Some(Vector3::x());
                    min_d = d;
                }
                if let Some(d) = mouse_ray.intersect_ring_segment(
                    center,
                    model_local_transform
                        .rotation
                        .transform_vector(&Vector3::y()),
                    consts::editor::gizmo::ROTATION_DISTANCE_Y,
                    thickness,
                    max_t,
                ) {
                    if (d < min_d) {
                        axis = Some(Vector3::y());
                        min_d = d;
                    }
                }
                if let Some(d) = mouse_ray.intersect_ring_segment(
                    center,
                    model_local_transform
                        .rotation
                        .transform_vector(&Vector3::z()),
                    consts::editor::gizmo::ROTATION_DISTANCE_Z,
                    thickness,
                    max_t,
                ) {
                    if (d < min_d) {
                        axis = Some(Vector3::z());
                        min_d = d;
                    }
                }
            }
            EditorGizmoType::Scale => {}
        }

        self.hover_gizmo_axis = axis;
        if input.is_mouse_button_pressed(mouse::Button::Left) {
            self.dragging_gizmo_axis = axis;
            *consumes_left_click = axis.is_some();
        }
    }

    pub fn update_and_render(
        &mut self,
        debug_renderer: &mut DebugRenderer,
        input: &Input,
        selected_entity: Entity,
        ecs_world: &ECSWorld,
        editor_camera_transform: &Transform,
    ) {
        let mut selected_entity_query =
            ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(selected_entity);
        let Ok(mut model_local_transform) = ecs_world.get::<(&mut Transform)>(selected_entity)
        else {
            return;
        };
        let model_world_transform =
            ecs_world.get_world_transform(selected_entity, &model_local_transform);

        match self.selected_gizmo {
            EditorGizmoType::Translate => {
                let center = model_world_transform.position;
                let flags = DebugFlags::XRAY | DebugFlags::SHADING;
                let mut x_line = DebugLine {
                    start: center,
                    end: center + Vector3::x() * consts::editor::gizmo::TRANSLATION_LENGTH,
                    thickness: consts::editor::gizmo::TRANSLATION_THICKNESS,
                    color: Color::new_srgb(1.0, 0.0, 0.0),
                    alpha: 1.0,
                    flags,
                };
                let mut y_line = DebugLine {
                    start: center,
                    end: center + Vector3::y() * consts::editor::gizmo::TRANSLATION_LENGTH,
                    thickness: consts::editor::gizmo::TRANSLATION_THICKNESS,
                    color: Color::new_srgb(0.0, 1.0, 0.0),
                    alpha: 1.0,
                    flags,
                };
                let mut z_line = DebugLine {
                    start: center,
                    end: center + Vector3::z() * consts::editor::gizmo::TRANSLATION_LENGTH,
                    thickness: consts::editor::gizmo::TRANSLATION_THICKNESS,
                    color: Color::new_srgb(0.0, 0.0, 1.0),
                    alpha: 1.0,
                    flags,
                };
                if let Some(dragging_axis) = self.dragging_gizmo_axis {
                    if dragging_axis == Vector3::x() {
                        x_line.color.multiply_gamma(1.5);
                        model_local_transform.position.x += input.mouse_delta().x
                            * consts::editor::gizmo::DRAGGING_TRANSFORM_SENSITIVITY
                            * (editor_camera_transform.position - center)
                                .dot(&-Vector3::z())
                                .signum()
                    } else if dragging_axis == Vector3::y() {
                        y_line.color.multiply_gamma(1.5);
                        model_local_transform.position.y += input.mouse_delta().y
                            * consts::editor::gizmo::DRAGGING_TRANSFORM_SENSITIVITY;
                    } else if dragging_axis == Vector3::z() {
                        z_line.color.multiply_gamma(1.5);
                        model_local_transform.position.z += input.mouse_delta().x
                            * consts::editor::gizmo::DRAGGING_TRANSFORM_SENSITIVITY
                            * (editor_camera_transform.position - center)
                                .dot(&Vector3::x())
                                .signum()
                    }
                } else if let Some(hovered_axis) = self.hover_gizmo_axis {
                    if hovered_axis == Vector3::x() {
                        x_line.color.multiply_gamma(1.15);
                    } else if hovered_axis == Vector3::y() {
                        y_line.color.multiply_gamma(1.15);
                    } else if hovered_axis == Vector3::z() {
                        z_line.color.multiply_gamma(1.15);
                    }
                }

                debug_renderer.draw_line(x_line);
                debug_renderer.draw_line(y_line);
                debug_renderer.draw_line(z_line);
            }
            EditorGizmoType::Rotate => {
                let center = model_world_transform.position;
                let flags = DebugFlags::XRAY | DebugFlags::SHADING;
                let thickness = consts::editor::gizmo::ROTATION_THICKNESS;
                let mut x_ring = DebugRing {
                    center,
                    normal: Vector3::x(),
                    stretch: consts::editor::gizmo::ROTATION_DISTANCE_X,
                    thickness,
                    color: Color::new_srgb(1.0, 0.0, 0.0),
                    alpha: 1.0,
                    flags,
                };
                let mut y_ring = DebugRing {
                    center,
                    stretch: consts::editor::gizmo::ROTATION_DISTANCE_Y,
                    normal: Vector3::y(),
                    thickness,
                    color: Color::new_srgb(0.0, 1.0, 0.0),
                    alpha: 1.0,
                    flags,
                };
                let mut z_ring = DebugRing {
                    center,
                    stretch: consts::editor::gizmo::ROTATION_DISTANCE_Z,
                    normal: Vector3::z(),
                    thickness,
                    color: Color::new_srgb(0.0, 0.0, 1.0),
                    alpha: 1.0,
                    flags,
                };
                if let Some(dragging_axis) = self.dragging_gizmo_axis {
                    if dragging_axis == Vector3::x() {
                        x_ring.color.multiply_gamma(1.5);
                        let delta = input.mouse_delta().y
                            * consts::editor::gizmo::DRAGGING_ROTATION_SENSITIVITY;

                        model_local_transform.rotation *=
                            UnitQuaternion::from_axis_angle(&Vector3::x_axis(), delta.to_radians());
                    } else if dragging_axis == Vector3::y() {
                        y_ring.color.multiply_gamma(1.5);
                        let delta = input.mouse_delta().x
                            * consts::editor::gizmo::DRAGGING_ROTATION_SENSITIVITY;
                        model_local_transform.rotation *=
                            UnitQuaternion::from_axis_angle(&Vector3::y_axis(), delta.to_radians());
                    } else if dragging_axis == Vector3::z() {
                        z_ring.color.multiply_gamma(1.5);
                        let delta = input.mouse_delta().y
                            * consts::editor::gizmo::DRAGGING_ROTATION_SENSITIVITY;
                        model_local_transform.rotation *=
                            UnitQuaternion::from_axis_angle(&Vector3::z_axis(), delta.to_radians());
                    }
                } else if let Some(hovered_axis) = self.hover_gizmo_axis {
                    if hovered_axis == Vector3::x() {
                        x_ring.color.multiply_gamma(1.15);
                    } else if hovered_axis == Vector3::y() {
                        y_ring.color.multiply_gamma(1.15);
                    } else if hovered_axis == Vector3::z() {
                        z_ring.color.multiply_gamma(1.15);
                    }
                }

                // Do this after so rotation is most up to date.
                x_ring.normal = model_local_transform
                    .rotation
                    .transform_vector(&x_ring.normal);
                y_ring.normal = model_local_transform
                    .rotation
                    .transform_vector(&y_ring.normal);
                z_ring.normal = model_local_transform
                    .rotation
                    .transform_vector(&z_ring.normal);

                debug_renderer.draw_ring(x_ring);
                debug_renderer.draw_ring(y_ring);
                debug_renderer.draw_ring(z_ring);
            }
            EditorGizmoType::Scale => todo!(),
        }
        self.hover_gizmo_axis = None;
    }
}
