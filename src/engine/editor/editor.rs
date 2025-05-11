use core::f32;

use hecs::With;
use nalgebra::{
    ComplexField, Rotation3, Translation3, Unit, UnitQuaternion, Vector2, Vector3, Vector4,
};
use rogue_macros::Resource;

use crate::{
    common::{color::Color, ray::Ray},
    consts::{self, editor::gizmo::DRAGGING_SENSITIVITY},
    engine::{
        asset::{
            asset::{AssetPath, Assets},
            repr::editor_settings::EditorSessionAsset,
        },
        debug::{DebugFlags, DebugLine, DebugOBB, DebugRenderer},
        editor::ui::init_editor_ui_textures,
        entity::{
            ecs_world::{ECSWorld, Entity},
            RenderableVoxelEntity,
        },
        graphics::camera::{Camera, MainCamera},
        input::{keyboard, mouse, Input},
        physics::transform::Transform,
        resource::{Res, ResMut},
        ui::UI,
        voxel::voxel_world::{VoxelTraceInfo, VoxelWorld},
        window::window::Window,
    },
    game::entity::player::Player,
    session::Session,
    settings::Settings,
};

use super::ui::EditorTab;

pub enum EditorGizmo {
    Translate,
    Rotate,
    Scale,
}

#[derive(Resource)]
pub struct Editor {
    last_main_camera: Option<(Entity, String)>,
    pub editor_camera_entity: Option<Entity>,
    pub editor_camera: EditorCamera,
    pub is_active: bool,
    pub initialized: bool,

    pub selected_gizmo: EditorGizmo,
    pub selected_entity: Option<Entity>,
    pub hovered_entity: Option<Entity>,

    pub dragging_gizmo_axis: Option<Vector3<f32>>,
    pub hover_gizmo_axis: Option<Vector3<f32>>,
}

pub struct EditorCameraMarker;

impl Editor {
    pub fn new() -> Self {
        let mut is_inactive = std::env::var("ROGUE_EDITOR")
            .map(|var| var.eq_ignore_ascii_case("off"))
            .unwrap_or(false);

        Self {
            last_main_camera: None,
            editor_camera: EditorCamera::new(),
            editor_camera_entity: None,
            is_active: !is_inactive,
            initialized: false,

            selected_gizmo: EditorGizmo::Translate,
            selected_entity: None,
            hovered_entity: None,

            dragging_gizmo_axis: None,
            hover_gizmo_axis: None,
        }
    }

    pub fn init_editor_session(
        &mut self,
        session: &Session,
        main_camera: &mut MainCamera,
        ecs_world: &mut ECSWorld,
    ) {
        assert!(self.editor_camera_entity.is_none());

        let last_session = &session.project;

        let mut camera_pos = Vector3::new(5.0, 5.0, 4.9);
        let mut camera_fov = f32::consts::FRAC_PI_2;
        let mut anchor_pos = Vector3::new(0.0, 0.0, 0.0);
        camera_pos = last_session.editor_camera_transform.transform.position;
        anchor_pos = last_session.rotation_anchor;
        camera_fov = last_session.editor_camera.camera.fov();

        let anchor_to_cam = camera_pos - anchor_pos;
        let distance = anchor_to_cam.magnitude();
        let euler = Vector3::new(
            (anchor_to_cam.y / distance).asin(),
            // Flip since nalgebra rotates clockwise.
            f32::atan2(anchor_to_cam.z, anchor_to_cam.x) - f32::consts::FRAC_PI_2,
            0.0,
        );
        self.editor_camera = EditorCamera {
            rotation_anchor: anchor_pos,
            euler,
            distance,
        };
        self.editor_camera_entity =
            Some(ecs_world.spawn((Camera::new(camera_fov), Transform::new())));

        self.initialized = true;
    }

    pub fn update_editor(
        mut editor: ResMut<Editor>,
        input: Res<Input>,
        ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
        settings: Res<Settings>,
        window: Res<Window>,
        ui: Res<UI>,
        mut debug_renderer: ResMut<DebugRenderer>,
    ) {
        let editor: &mut Editor = &mut editor;
        let mut editor_camera_query = ecs_world
            .query_one::<(&mut Transform, &Camera)>(editor.editor_camera_entity.unwrap())
            .unwrap();
        let (mut editor_transform, camera) = editor_camera_query.get().unwrap();
        let editor_camera = &mut editor.editor_camera;

        voxel_world.update_render_center(editor_camera.rotation_anchor);

        let mouse_ray = {
            let content_size = ui.content_size(window.inner_size_vec2().cast::<f32>());
            let aspect_ratio = content_size.x / content_size.y;
            let mouse_pos_uv =
                (input.mouse_position() - ui.content_offset()).component_div(&content_size);
            let mouse_pos_ndc =
                Vector2::new(mouse_pos_uv.x * 2.0 - 1.0, 1.0 - mouse_pos_uv.y * 2.0);
            let scaled_ndc = Vector2::new(mouse_pos_ndc.x * aspect_ratio, mouse_pos_ndc.y)
                * (camera.fov() * 0.5).tan();
            let ray_origin = editor_transform.position;
            let ray_dir = (editor_transform.rotation
                * Vector3::new(scaled_ndc.x, scaled_ndc.y, 1.0))
            .normalize();
            Ray::new(ray_origin, ray_dir)
        };
        let hovered_trace = voxel_world.trace_world(&ecs_world, mouse_ray.clone());

        'gizmo_drag: {
            if input.is_mouse_button_released(mouse::Button::Left) {
                editor.dragging_gizmo_axis = None;
            }

            if let Some(selected_entity) = editor.selected_entity {
                let Ok(mut selected_entity_query) =
                    ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(selected_entity)
                else {
                    break 'gizmo_drag;
                };
                let Some((model_transform, renderable_entity)) = selected_entity_query.get() else {
                    break 'gizmo_drag;
                };
                let center = model_transform.position();

                let mut min_d = 1000.0;
                let mut axis = None;
                if let Some(d) = mouse_ray.intersect_line_segment(
                    center,
                    center + Vector3::x() * consts::editor::gizmo::LENGTH,
                    consts::editor::gizmo::THICKNESS,
                    1000.0,
                ) {
                    axis = Some(Vector3::x());
                    min_d = d;
                }
                if let Some(d) = mouse_ray.intersect_line_segment(
                    center,
                    center + Vector3::y() * consts::editor::gizmo::LENGTH,
                    consts::editor::gizmo::THICKNESS,
                    1000.0,
                ) {
                    if (d < min_d) {
                        axis = Some(Vector3::y());
                        min_d = d;
                    }
                }
                if let Some(d) = mouse_ray.intersect_line_segment(
                    center,
                    center + Vector3::z() * consts::editor::gizmo::LENGTH,
                    consts::editor::gizmo::THICKNESS,
                    1000.0,
                ) {
                    if (d < min_d) {
                        axis = Some(Vector3::z());
                        min_d = d;
                    }
                }

                editor.hover_gizmo_axis = axis;
                if input.is_mouse_button_pressed(mouse::Button::Left) {
                    editor.dragging_gizmo_axis = axis;
                }
            }
        }
        if input.is_mouse_button_pressed(mouse::Button::Left) {
            if let Some(VoxelTraceInfo::Entity {
                entity_id,
                voxel_model_id,
                local_voxel_pos,
            }) = &hovered_trace
            {
                editor.selected_entity = Some(*entity_id);
            } else {
                editor.selected_entity = None;
            }
        }
        if let Some(hovered_entity) = editor.hovered_entity {
            if !(editor.selected_entity.is_some()
                && editor.selected_entity.unwrap() == hovered_entity)
            {
                'hovered_entity_block: {
                    let Ok(mut hovered_entity_query) =
                        ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(hovered_entity)
                    else {
                        break 'hovered_entity_block;
                    };
                    let Some((model_transform, renderable_entity)) = hovered_entity_query.get()
                    else {
                        break 'hovered_entity_block;
                    };
                    if let Some(voxel_model_id) = renderable_entity.voxel_model_id() {
                        let voxel_model = voxel_world.registry.get_dyn_model(voxel_model_id);
                        let obb = model_transform.as_voxel_model_obb(voxel_model.length());
                        debug_renderer.draw_obb(DebugOBB {
                            obb: &obb,
                            thickness: 0.1,
                            color: Color::new_srgb_hex("#4553ad"),
                            alpha: 1.0,
                        });
                    }
                }
            }
        } else if let Some(VoxelTraceInfo::Entity {
            entity_id,
            voxel_model_id,
            local_voxel_pos,
        }) = &hovered_trace
        {
            if !(editor.selected_entity.is_some() && editor.selected_entity.unwrap() == *entity_id)
            {
                let voxel_model = voxel_world.registry.get_dyn_model(*voxel_model_id);
                let model_transform = ecs_world.get::<&Transform>(*entity_id).unwrap();
                let obb = model_transform.as_voxel_model_obb(voxel_model.length());
                debug_renderer.draw_obb(DebugOBB {
                    obb: &obb,
                    thickness: 0.1,
                    color: Color::new_srgb_hex("#4553ad"),
                    alpha: 1.0,
                });
            }
        }

        'selected_entity_block: {
            if let Some(selected_entity) = editor.selected_entity {
                let Ok(mut selected_entity_query) =
                    ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(selected_entity)
                else {
                    break 'selected_entity_block;
                };
                let Ok(mut model_transform) = ecs_world.get::<(&mut Transform)>(selected_entity)
                else {
                    break 'selected_entity_block;
                };

                match editor.selected_gizmo {
                    EditorGizmo::Translate => {
                        let center = model_transform.position();
                        let flags = DebugFlags::XRAY | DebugFlags::SHADING;
                        let mut x_line = DebugLine {
                            start: center,
                            end: center + Vector3::x() * consts::editor::gizmo::LENGTH,
                            thickness: consts::editor::gizmo::THICKNESS,
                            color: Color::new_srgb(1.0, 0.0, 0.0),
                            alpha: 1.0,
                            flags,
                        };
                        let mut y_line = DebugLine {
                            start: center,
                            end: center + Vector3::y() * consts::editor::gizmo::LENGTH,
                            thickness: consts::editor::gizmo::THICKNESS,
                            color: Color::new_srgb(0.0, 1.0, 0.0),
                            alpha: 1.0,
                            flags,
                        };
                        let mut z_line = DebugLine {
                            start: center,
                            end: center + Vector3::z() * consts::editor::gizmo::LENGTH,
                            thickness: consts::editor::gizmo::THICKNESS,
                            color: Color::new_srgb(0.0, 0.0, 1.0),
                            alpha: 1.0,
                            flags,
                        };
                        if let Some(dragging_axis) = editor.dragging_gizmo_axis {
                            if dragging_axis == Vector3::x() {
                                x_line.color.multiply_gamma(1.5);
                                model_transform.position.x += input.mouse_delta().x
                                    * DRAGGING_SENSITIVITY
                                    * (editor_transform.position - center)
                                        .dot(&-Vector3::z())
                                        .signum()
                            } else if dragging_axis == Vector3::y() {
                                y_line.color.multiply_gamma(1.5);
                                model_transform.position.y +=
                                    input.mouse_delta().y * DRAGGING_SENSITIVITY;
                            } else if dragging_axis == Vector3::z() {
                                z_line.color.multiply_gamma(1.5);
                                model_transform.position.z += input.mouse_delta().x
                                    * DRAGGING_SENSITIVITY
                                    * (editor_transform.position - center)
                                        .dot(&Vector3::x())
                                        .signum()
                            }
                        } else if let Some(hovered_axis) = editor.hover_gizmo_axis {
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
                    EditorGizmo::Rotate => todo!(),
                    EditorGizmo::Scale => todo!(),
                }

                let Ok(renderable_entity) =
                    ecs_world.get::<(&RenderableVoxelEntity)>(selected_entity)
                else {
                    break 'selected_entity_block;
                };
                if renderable_entity.is_null() {
                    break 'selected_entity_block;
                }
                let voxel_model = voxel_world
                    .registry
                    .get_dyn_model(renderable_entity.voxel_model_id_unchecked());
                let obb = model_transform.as_voxel_model_obb(voxel_model.length());
                debug_renderer.draw_obb(DebugOBB {
                    obb: &obb,
                    thickness: 0.1,
                    color: Color::new_srgb_hex("#1026b3"),
                    alpha: 1.0,
                });
            }
        }
        editor.hovered_entity = None;
        editor.hover_gizmo_axis = None;

        if input.is_mouse_button_down(mouse::Button::Middle) {
            let delta =
                -input.mouse_delta() * settings.mouse_sensitivity * editor_camera.distance.max(1.0);
            let up = editor_transform.rotation.transform_vector(&Vector3::y());
            let right = editor_transform.rotation.transform_vector(&Vector3::x());
            editor_camera.rotation_anchor += delta.x * right + delta.y * up;
        }

        if input.is_mouse_button_down(mouse::Button::Right) {
            let delta = input.mouse_delta() * settings.mouse_sensitivity * 0.8;
            editor_camera.euler.x = (editor_camera.euler.x - delta.y)
                .clamp(-f32::consts::FRAC_PI_2, f32::consts::FRAC_PI_2);
            editor_camera.euler.y += delta.x;
        }

        let scroll_delta = input.mouse().scroll_delta();
        editor_camera.distance = (editor_camera.distance.powf(1.0 / 1.7) + scroll_delta * 0.07)
            .powf(1.7)
            .max(0.01);

        let rot = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), editor_camera.euler.y)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -editor_camera.euler.x);
        let pos = editor_camera.rotation_anchor
            + editor_camera.distance * (rot.transform_vector(&Vector3::z()));
        editor_transform.position = pos;
        editor_transform.rotation =
            UnitQuaternion::from_axis_angle(
                &Vector3::y_axis(),
                editor_camera.euler.y - f32::consts::PI,
            ) * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), editor_camera.euler.x);

        debug_renderer.draw_line(DebugLine {
            start: editor_camera.rotation_anchor,
            end: editor_camera.rotation_anchor,
            thickness: 1.0,
            color: Color::new_srgb(0.8, 0.1, 0.7),
            alpha: 1.0,
            flags: DebugFlags::NONE,
        });
    }

    pub fn update_toggle(
        mut editor: ResMut<Editor>,
        mut main_camera: ResMut<MainCamera>,
        mut ecs_world: ResMut<ECSWorld>,
        mut input: ResMut<Input>,
        mut window: ResMut<Window>,
        session: Res<Session>,
    ) {
        if editor.is_active && !editor.initialized {
            editor.init_editor_session(&session, &mut main_camera, &mut ecs_world);
            main_camera.set_camera(editor.editor_camera_entity.unwrap(), "editor_camera");
        }

        if input.is_key_pressed(keyboard::Key::E) {
            if editor.is_active {
                if let Some(last_camera) = editor.last_main_camera.clone() {
                    main_camera.set_camera(last_camera.0, &last_camera.1);
                    editor.is_active = false;
                } else {
                    let mut player_query = ecs_world.query::<&Player>();
                    if let Some((player_entity, player)) = player_query.into_iter().next() {
                        editor.is_active = false;
                        main_camera.set_camera(player_entity, "player_cam");
                        window.set_curser_lock(!player.paused);
                    } else {
                        log::warn!("No camera to switch to from the editor.");
                    }
                }
            } else {
                editor.is_active = true;
                editor.last_main_camera = main_camera.camera.clone();
                if !editor.initialized {
                    editor.init_editor_session(&session, &mut main_camera, &mut ecs_world);
                }
                main_camera.set_camera(editor.editor_camera_entity.unwrap(), "editor_camera");
                window.set_curser_lock(false);
            }
        }
    }
}

pub struct EditorCamera {
    pub rotation_anchor: Vector3<f32>,
    pub euler: Vector3<f32>,
    pub distance: f32,
}

impl EditorCamera {
    pub fn new() -> Self {
        Self {
            rotation_anchor: Vector3::zeros(),
            euler: Vector3::zeros(),
            distance: 1.0,
        }
    }
}
