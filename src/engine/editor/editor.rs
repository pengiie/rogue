use core::f32;
use std::time::Duration;

use hecs::With;
use nalgebra::{
    ComplexField, Rotation3, Translation3, Unit, UnitQuaternion, Vector2, Vector3, Vector4,
};
use rogue_macros::Resource;

use crate::{
    common::{animate::Animation, color::Color, ray::Ray},
    consts::{
        self,
        editor::gizmo::{DRAGGING_ROTATION_SENSITIVITY, DRAGGING_TRANSFORM_SENSITIVITY},
    },
    engine::{
        asset::{
            asset::{AssetPath, Assets},
            repr::editor_settings::EditorSessionAsset,
        },
        debug::{
            DebugCapsule, DebugFlags, DebugLine, DebugOBB, DebugPlane, DebugRenderer, DebugRing,
        },
        editor::ui::init_editor_ui_textures,
        entity::{
            ecs_world::{ECSWorld, Entity},
            RenderableVoxelEntity,
        },
        graphics::camera::{Camera, MainCamera},
        input::{
            keyboard::{self, Key, Modifier},
            mouse, Input,
        },
        physics::{capsule_collider, physics_world::Colliders, transform::Transform},
        resource::{Res, ResMut},
        ui::UI,
        voxel::{
            attachment::{Attachment, AttachmentId, AttachmentInfoMap, AttachmentMap, PTMaterial},
            chunk_generator::ChunkGenerator,
            cursor::{VoxelEditEntityInfo, VoxelEditInfo},
            voxel_world::{self, VoxelEdit, VoxelTraceInfo, VoxelWorld},
        },
        window::{
            time::{Instant, Time},
            window::Window,
        },
    },
    game::entity::player::Player,
    session::Session,
    settings::Settings,
};

pub enum EditorGizmo {
    Translate,
    Rotate,
    Scale,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EditorView {
    PanOrbit,
    Fps,
}

#[derive(Resource)]
pub struct Editor {
    last_main_camera: Option<(Entity, String)>,
    pub editor_camera_entity: Option<Entity>,
    pub editor_camera: EditorCamera,
    pub curr_editor_view: EditorView,

    pub saved_ecs_state: Option<ECSWorld>,
    pub is_active: bool,
    pub initialized: bool,

    pub selected_gizmo: EditorGizmo,
    pub selected_entity: Option<Entity>,
    pub hovered_entity: Option<Entity>,

    pub focus_animation: Animation<Vector3<f32>>,
    pub double_clicker_buffer: [Option<Instant>; 2],

    pub dragging_gizmo_axis: Option<Vector3<f32>>,
    pub hover_gizmo_axis: Option<Vector3<f32>>,

    pub world_editing: EditorWorldEditing,
    pub terrain_generation: EditorTerrainGeneration,
}

pub struct EditorWorldEditing {
    pub entity_enabled: bool,
    pub terrain_enabled: bool,
    pub size: u32,
    pub color: Color,
    pub tool: EditorEditingTool,
}

pub struct EditorTerrainGeneration {
    pub chunk_generator: ChunkGenerator,
    pub generation_radius: u32,
}

impl EditorTerrainGeneration {
    pub fn new() -> Self {
        EditorTerrainGeneration {
            chunk_generator: ChunkGenerator::new(0),
            generation_radius: 0,
        }
    }
}

impl EditorWorldEditing {
    pub fn new() -> Self {
        Self {
            terrain_enabled: true,
            entity_enabled: false,
            size: 2,
            color: Color::new_srgb(0.5, 0.5, 0.5),
            tool: EditorEditingTool::Pencil,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorEditingTool {
    Pencil,
    Eraser,
    Brush,
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
            saved_ecs_state: None,
            is_active: !is_inactive,
            initialized: false,
            curr_editor_view: EditorView::PanOrbit,

            selected_gizmo: EditorGizmo::Translate,
            selected_entity: None,
            hovered_entity: None,

            focus_animation: Animation::new(Duration::ZERO),
            double_clicker_buffer: [const { None }; 2],

            dragging_gizmo_axis: None,
            hover_gizmo_axis: None,

            world_editing: EditorWorldEditing::new(),
            terrain_generation: EditorTerrainGeneration::new(),
        }
    }

    pub fn init_editor_session(
        &mut self,
        session: &mut Session,
        main_camera: &mut MainCamera,
        ecs_world: &mut ECSWorld,
    ) {
        assert!(self.editor_camera_entity.is_none());

        let last_session = &session.project;
        session.terrain_dir = last_session.terrain_asset_path.clone();

        let mut camera_pos = Vector3::new(5.0, 5.0, 4.9);
        let mut camera_fov = f32::consts::FRAC_PI_2;
        let mut anchor_pos = Vector3::new(0.0, 0.0, 0.0);
        camera_pos = last_session.editor_camera_transform.transform.position;
        anchor_pos = last_session.rotation_anchor;
        camera_fov = last_session.editor_camera.camera.fov();

        self.editor_camera = EditorCamera::from_pos_anchor(camera_pos, anchor_pos);
        self.editor_camera_entity =
            Some(ecs_world.spawn((Camera::new(camera_fov), Transform::new())));

        self.initialized = true;
    }

    pub fn update_editor_fps(
        mut editor: ResMut<Editor>,
        input: Res<Input>,
        ecs_world: ResMut<ECSWorld>,
        settings: Res<Settings>,
        mut voxel_world: ResMut<VoxelWorld>,
        time: Res<Time>,
        mut window: ResMut<Window>,
    ) {
        let editor: &mut Editor = &mut editor;
        let mut editor_camera_query = ecs_world
            .query_one::<(&mut Transform, &Camera)>(editor.editor_camera_entity.unwrap())
            .unwrap();
        let (mut editor_transform, camera) = editor_camera_query.get().unwrap();
        let editor_camera = &mut editor.editor_camera;

        voxel_world.update_render_center(editor_transform.position);

        if input.is_key_pressed(Key::Escape) {
            let is_locked = window.is_cursor_locked();
            window.set_curser_lock(!is_locked);
        }

        let mut md = input.mouse_delta();
        if (md.x != 0.0 || md.y != 0.0) && window.is_cursor_locked() {
            // Clamp up and down yaw.
            md *= settings.mouse_sensitivity;

            editor_camera.euler.x = (editor_camera.euler.x - md.y)
                .clamp(-f32::consts::FRAC_PI_2, f32::consts::FRAC_PI_2);
            editor_camera.euler.y += md.x;
        }
        editor_transform.rotation =
            UnitQuaternion::from_axis_angle(
                &Vector3::y_axis(),
                editor_camera.euler.y - f32::consts::PI,
            ) * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), editor_camera.euler.x);

        let input_axes = input.movement_axes();

        let mut translation = Vector3::new(0.0, 0.0, 0.0);
        if input_axes.x != 0.0 || input_axes.y != 0.0 {
            let yaw_quaternion = UnitQuaternion::from_axis_angle(
                &Vector3::y_axis(),
                editor_camera.euler.y - f32::consts::PI,
            );
            let rotated_xz = yaw_quaternion
                .transform_vector(&Vector3::new(input_axes.x, 0.0, input_axes.y))
                .normalize();
            translation.x = rotated_xz.x;
            translation.z = rotated_xz.z;
        }

        if input.is_key_down(Key::Space) {
            translation.y = 1.0;
        }
        if input.is_key_down(Key::LShift) {
            translation.y = -1.0;
        }

        let mut speed = 5.0;
        if input.is_key_down(Key::LControl) {
            speed = 10.0;
        }

        editor_transform.position += translation * speed * time.delta_time().as_secs_f32();
    }

    pub fn update_editor_pan_orbit(
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
    }

    pub fn switch_to_fps(&mut self, window: &mut Window) {
        self.curr_editor_view = EditorView::Fps;
        window.set_curser_lock(true);
    }

    pub fn switch_to_pan_orbit(&mut self, ecs_world: &mut ECSWorld, window: &mut Window) {
        self.curr_editor_view = EditorView::PanOrbit;
        window.set_curser_lock(false);
        let mut editor_camera_query = ecs_world
            .query_one::<(&mut Transform, &Camera)>(self.editor_camera_entity.unwrap())
            .unwrap();
        let (mut editor_transform, camera) = editor_camera_query.get().unwrap();
        self.editor_camera.rotation_anchor =
            editor_transform.position + editor_transform.forward() * self.editor_camera.distance;
    }

    pub fn update_camera_animations(mut editor: ResMut<Editor>, mut ecs_world: ResMut<ECSWorld>) {
        let (mut editor_transform) = ecs_world
            .get::<&mut Transform>(editor.editor_camera_entity.unwrap())
            .unwrap();
        editor
            .focus_animation
            .update(&mut editor_transform.position);
    }

    /// The first editor update function called.
    pub fn update_editor_actions(
        mut editor: ResMut<Editor>,
        input: Res<Input>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
        settings: Res<Settings>,
        mut window: ResMut<Window>,
        mut ui: ResMut<UI>,
        mut debug_renderer: ResMut<DebugRenderer>,
        mut main_camera: ResMut<MainCamera>,
        session: Res<Session>,
    ) {
        let editor: &mut Editor = &mut editor;

        if input.is_key_pressed_with_modifiers(Key::Z, &[Modifier::Control]) {
            todo!("you should implement this.");
        } else if input.is_key_pressed_with_modifiers(Key::Y, &[Modifier::Control]) {
            todo!("you should implement this.");
        }

        if input.is_key_pressed(Key::G) {
            if let Some(game_camera) = &session.game_camera {
                main_camera.set_camera(*game_camera, "Game view");
            }
        }
        if input.is_key_pressed(Key::B) {
            main_camera.set_camera(
                editor
                    .editor_camera_entity
                    .expect("Editor camera should be a thing"),
                "Game view",
            );
        }

        if main_camera.camera() != editor.editor_camera_entity {
            return;
        }

        if input.is_key_pressed(Key::E) {
            match editor.curr_editor_view {
                EditorView::PanOrbit => {
                    editor.switch_to_fps(&mut window);
                }
                EditorView::Fps => {
                    editor.switch_to_pan_orbit(&mut ecs_world, &mut window);
                }
            }
        }

        if input.is_action_pressed(consts::actions::EDITOR_GIZMO_TRANSLATION) {
            editor.selected_gizmo = EditorGizmo::Translate;
        }
        if input.is_action_pressed(consts::actions::EDITOR_GIZMO_ROTATION) {
            editor.selected_gizmo = EditorGizmo::Rotate;
        }

        let mut editor_camera_query = ecs_world
            .query_one::<(&mut Transform, &Camera)>(editor.editor_camera_entity.unwrap())
            .unwrap();
        let (mut editor_transform, camera) = editor_camera_query.get().unwrap();
        let editor_camera = &mut editor.editor_camera;

        let world_center = match editor.curr_editor_view {
            EditorView::PanOrbit => editor_camera.rotation_anchor,
            EditorView::Fps => editor_transform.position,
        };
        voxel_world.update_render_center(world_center);

        let mouse_ray = if window.is_cursor_locked() {
            editor_transform.get_ray()
        } else {
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

        let not_using_editor_camera = main_camera.camera.as_ref().map_or(false, |(camera, _)| {
            camera != editor.editor_camera_entity.as_ref().unwrap()
        });
        // Whether to show debug lines of the gizmos or entities.
        let show_entity_outlines =
            !(editor.world_editing.entity_enabled || not_using_editor_camera);

        let mut has_valid_double_click = false;
        'double_click: {
            if input.is_mouse_button_pressed(mouse::Button::Right) {
                let buf = &mut editor.double_clicker_buffer;
                buf.swap(0, 1);
                buf[0] = Some(Instant::now());
                if let [Some(recent), Some(old)] = buf {
                    if (recent - old).as_secs_f32() < consts::editor::DOUBLE_CLICK_TIME_SECS {
                        has_valid_double_click = true;
                    }
                }
                if has_valid_double_click {
                    editor.double_clicker_buffer.fill(None);
                }
            }
        }

        'collider_draw: {
            if let Some(selected_entity) = editor.selected_entity {
                let Ok(mut selected_entity_query) =
                    ecs_world.query_one::<(&Transform, &Colliders)>(selected_entity)
                else {
                    break 'collider_draw;
                };
                let Some((model_local_transform, colliders)) = selected_entity_query.get() else {
                    break 'collider_draw;
                };
                let model_world_transform =
                    ecs_world.get_world_transform(selected_entity, model_local_transform);
                //for capsule_collider in &colliders.capsule_colliders {
                //    debug_renderer.draw_ellipsoid(DebugEllipsoid {
                //        center: capsule_collider.center + model_world_transform.position,
                //        orientation: capsule_collider.orientation * model_world_transform.rotation,
                //        radius: capsule_collider.radius,
                //        height: capsule_collider.height,
                //        color: Color::new_srgb(0.7, 0.1, 0.3),
                //        alpha: 0.3,
                //        flags: DebugFlags::SHADING,
                //    });
                //}
                //for plane_collider in &colliders.plane_colliders {
                //    debug_renderer.draw_plane(DebugPlane {
                //        center: plane_collider.center + model_world_transform.position,
                //        normal: model_world_transform.rotation * plane_collider.normal,
                //        size: plane_collider.size,
                //        color: Color::new_srgb(0.7, 0.1, 0.3),
                //        alpha: 0.3,
                //        flags: DebugFlags::SHADING,
                //    });
                //}
            }
        }

        let mut consume_left_click = false;
        'gizmo_drag: {
            if !show_entity_outlines {
                break 'gizmo_drag;
            }
            if input.is_mouse_button_released(mouse::Button::Left) {
                editor.dragging_gizmo_axis = None;
            }

            if let Some(selected_entity) = editor.selected_entity {
                let Ok(mut selected_entity_query) =
                    ecs_world.query_one::<(&Transform)>(selected_entity)
                else {
                    break 'gizmo_drag;
                };
                let Some((model_local_transform)) = selected_entity_query.get() else {
                    break 'gizmo_drag;
                };
                let model_world_transform =
                    ecs_world.get_world_transform(selected_entity, model_local_transform);
                let center = model_world_transform.position;

                let mut min_d = 1000.0;
                let mut axis = None;

                match editor.selected_gizmo {
                    EditorGizmo::Translate => {
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
                    EditorGizmo::Rotate => {
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
                    EditorGizmo::Scale => {}
                }

                editor.hover_gizmo_axis = axis;
                if input.is_mouse_button_pressed(mouse::Button::Left) {
                    editor.dragging_gizmo_axis = axis;
                    consume_left_click = axis.is_some();
                }
            }
        }
        if input.is_mouse_button_pressed(mouse::Button::Left) && !consume_left_click {
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

        // Editing circle preview.
        'editing_preview: {
            let center = match &hovered_trace {
                Some(VoxelTraceInfo::Terrain { world_voxel_pos }) => {
                    if editor.world_editing.terrain_enabled {
                        let center = (world_voxel_pos.cast::<f32>()
                            * consts::voxel::VOXEL_METER_LENGTH)
                            .add_scalar(consts::voxel::VOXEL_METER_LENGTH * 0.5);
                        Some(center)
                    } else {
                        None
                    }
                }
                Some(VoxelTraceInfo::Entity {
                    entity_id,
                    voxel_model_id,
                    local_voxel_pos,
                }) => {
                    if editor.world_editing.entity_enabled {
                        let Ok(mut preview_entity_query) =
                            ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(*entity_id)
                        else {
                            break 'editing_preview;
                        };
                        let Some((model_local_transform, renderable)) = preview_entity_query.get()
                        else {
                            break 'editing_preview;
                        };
                        let model_world_transform =
                            ecs_world.get_world_transform(*entity_id, &model_local_transform);

                        let side_length = voxel_world
                            .get_dyn_model(renderable.voxel_model_id().unwrap())
                            .length();
                        let model_obb = model_world_transform.as_voxel_model_obb(side_length);
                        let forward = model_world_transform.forward()
                            * consts::voxel::VOXEL_METER_LENGTH
                            * model_world_transform.scale.z;
                        let right = model_world_transform.right()
                            * consts::voxel::VOXEL_METER_LENGTH
                            * model_world_transform.scale.x;
                        let up = model_world_transform.up()
                            * consts::voxel::VOXEL_METER_LENGTH
                            * model_world_transform.scale.y;

                        let center = model_obb.aabb.min
                            + forward * (local_voxel_pos.z as f32 + 0.5)
                            + right * (local_voxel_pos.x as f32 + 0.5)
                            + up * (local_voxel_pos.y as f32 + 0.5);
                        Some(center)
                    } else {
                        None
                    }
                }
                None => None,
            };

            if let Some(center) = center {
                debug_renderer.draw_line(DebugLine {
                    start: center,
                    end: center,
                    thickness: (editor.world_editing.size as f32 + 1.0)
                        * consts::voxel::VOXEL_METER_LENGTH
                        * 0.5,
                    color: editor.world_editing.color.clone(),
                    alpha: 0.4,
                    flags: DebugFlags::NONE,
                });
            }
        }

        if let Some(hovered_entity) = editor.hovered_entity {
            // Hovered entity within editor ui.
            if !(editor.selected_entity.is_some()
                && editor.selected_entity.unwrap() == hovered_entity)
                && show_entity_outlines
            {
                'hovered_entity_block: {
                    let Ok(mut hovered_entity_query) =
                        ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(hovered_entity)
                    else {
                        break 'hovered_entity_block;
                    };
                    let Some((model_local_transform, renderable_entity)) =
                        hovered_entity_query.get()
                    else {
                        break 'hovered_entity_block;
                    };
                    if let Some(voxel_model_id) = renderable_entity.voxel_model_id() {
                        let voxel_model = voxel_world.registry.get_dyn_model(voxel_model_id);
                        let model_world_transform =
                            ecs_world.get_world_transform(hovered_entity, &model_local_transform);
                        let obb = model_world_transform.as_voxel_model_obb(voxel_model.length());
                        debug_renderer.draw_obb(DebugOBB {
                            obb: &obb,
                            thickness: consts::editor::ENTITY_OUTLINE_THICKNESS,
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
            // Hovered entity with mouse.
            let model_local_transform = ecs_world.get::<&Transform>(*entity_id).unwrap();
            let model_world_transform =
                ecs_world.get_world_transform(*entity_id, &model_local_transform);
            if !(editor.selected_entity.is_some() && editor.selected_entity.unwrap() == *entity_id)
                && show_entity_outlines
            {
                let voxel_model = voxel_world.registry.get_dyn_model(*voxel_model_id);
                let obb = model_world_transform.as_voxel_model_obb(voxel_model.length());
                debug_renderer.draw_obb(DebugOBB {
                    obb: &obb,
                    thickness: consts::editor::ENTITY_OUTLINE_THICKNESS,
                    color: Color::new_srgb_hex("#4553ad"),
                    alpha: 1.0,
                });
            }

            if has_valid_double_click {
                // Animate camera onto the entity.
                // TODO: Adjust based off of entity size.
                let focus_distance = 6.0;
                let end =
                    model_world_transform.position - editor_transform.forward() * focus_distance;
                editor.focus_animation.start(
                    editor_transform.position,
                    end,
                    Duration::from_secs_f64(0.75),
                );
                *editor_camera = EditorCamera::from_pos_anchor(end, model_world_transform.position);
            }
        } else if let Some(VoxelTraceInfo::Terrain { world_voxel_pos }) = &hovered_trace {
            // Hovered terrain voxel position.
            if has_valid_double_click {
                // Animate camera onto the terrain pos.
                let voxel_world_position =
                    world_voxel_pos.cast::<f32>() * consts::voxel::VOXEL_METER_LENGTH;
                // TODO: Adjust based off of entity size.
                let focus_distance = 6.0;
                let end = voxel_world_position - editor_transform.forward() * focus_distance;
                editor.focus_animation.start(
                    editor_transform.position,
                    end,
                    Duration::from_secs_f64(0.75),
                );
                *editor_camera = EditorCamera::from_pos_anchor(end, voxel_world_position);
            }
        }

        // Gizmo and outline drawing for selected entity.
        'selected_entity_block: {
            if !show_entity_outlines {
                break 'selected_entity_block;
            }
            if let Some(selected_entity) = editor.selected_entity {
                let Ok(mut selected_entity_query) =
                    ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(selected_entity)
                else {
                    break 'selected_entity_block;
                };
                let Ok(mut model_local_transform) =
                    ecs_world.get::<(&mut Transform)>(selected_entity)
                else {
                    break 'selected_entity_block;
                };
                let model_world_transform =
                    ecs_world.get_world_transform(selected_entity, &model_local_transform);

                match editor.selected_gizmo {
                    EditorGizmo::Translate => {
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
                        if let Some(dragging_axis) = editor.dragging_gizmo_axis {
                            if dragging_axis == Vector3::x() {
                                x_line.color.multiply_gamma(1.5);
                                model_local_transform.position.x += input.mouse_delta().x
                                    * DRAGGING_TRANSFORM_SENSITIVITY
                                    * (editor_transform.position - center)
                                        .dot(&-Vector3::z())
                                        .signum()
                            } else if dragging_axis == Vector3::y() {
                                y_line.color.multiply_gamma(1.5);
                                model_local_transform.position.y +=
                                    input.mouse_delta().y * DRAGGING_TRANSFORM_SENSITIVITY;
                            } else if dragging_axis == Vector3::z() {
                                z_line.color.multiply_gamma(1.5);
                                model_local_transform.position.z += input.mouse_delta().x
                                    * DRAGGING_TRANSFORM_SENSITIVITY
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
                    EditorGizmo::Rotate => {
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
                        if let Some(dragging_axis) = editor.dragging_gizmo_axis {
                            if dragging_axis == Vector3::x() {
                                x_ring.color.multiply_gamma(1.5);
                                let delta = input.mouse_delta().y * DRAGGING_ROTATION_SENSITIVITY;

                                model_local_transform.rotation *= UnitQuaternion::from_axis_angle(
                                    &Vector3::x_axis(),
                                    delta.to_radians(),
                                );
                            } else if dragging_axis == Vector3::y() {
                                y_ring.color.multiply_gamma(1.5);
                                let delta = input.mouse_delta().x * DRAGGING_ROTATION_SENSITIVITY;
                                model_local_transform.rotation *= UnitQuaternion::from_axis_angle(
                                    &Vector3::y_axis(),
                                    delta.to_radians(),
                                );
                            } else if dragging_axis == Vector3::z() {
                                z_ring.color.multiply_gamma(1.5);
                                let delta = input.mouse_delta().y * DRAGGING_ROTATION_SENSITIVITY;
                                model_local_transform.rotation *= UnitQuaternion::from_axis_angle(
                                    &Vector3::z_axis(),
                                    delta.to_radians(),
                                );
                            }
                        } else if let Some(hovered_axis) = editor.hover_gizmo_axis {
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
                let obb = model_world_transform.as_voxel_model_obb(voxel_model.length());
                debug_renderer.draw_obb(DebugOBB {
                    obb: &obb,
                    thickness: consts::editor::ENTITY_OUTLINE_THICKNESS,
                    color: Color::new_srgb_hex("#1026b3"),
                    alpha: 1.0,
                });
            }
        }
        editor.hovered_entity = None;
        editor.hover_gizmo_axis = None;

        // Editing things.
        if input.is_mouse_button_pressed(mouse::Button::Left) {
            let size = editor.world_editing.size;
            let trace = hovered_trace;

            fn apply_edit(
                voxel_world: &mut VoxelWorld,
                editor: &Editor,
                trace: &Option<VoxelTraceInfo>,
                size: u32,
                attachment_map: AttachmentInfoMap,
                f: impl Fn(
                    /*world/model_voxel_center=*/ Vector3<i32>,
                ) -> Box<
                    dyn Fn(
                        VoxelEdit,
                        /*world/model_voxel_pos*/ Vector3<i32>,
                        /*local/model_voxel_pos=*/ Vector3<u32>,
                    ),
                >,
            ) {
                let size_vec = Vector3::new(size, size, size);
                if let Some(VoxelTraceInfo::Terrain {
                    world_voxel_pos: world_voxel_hit,
                }) = trace
                {
                    if editor.world_editing.terrain_enabled {
                        let anchor = world_voxel_hit - size_vec.cast::<i32>();
                        voxel_world.apply_voxel_edit(
                            VoxelEditInfo {
                                world_voxel_position: anchor,
                                world_voxel_length: (size_vec * 2).add_scalar(1),
                                attachment_map,
                            },
                            f(*world_voxel_hit),
                        );
                    }
                } else if let Some(VoxelTraceInfo::Entity {
                    entity_id,
                    voxel_model_id,
                    local_voxel_pos,
                }) = &trace
                {
                    if editor.world_editing.entity_enabled {
                        let local_voxel_pos = local_voxel_pos.cast::<i32>();
                        let anchor = local_voxel_pos - size_vec.cast::<i32>();
                        voxel_world.apply_voxel_edit_entity(
                            VoxelEditEntityInfo {
                                model_id: *voxel_model_id,
                                local_voxel_pos: anchor,
                                voxel_length: (size_vec * 2).add_scalar(1),
                                attachment_map,
                            },
                            f(local_voxel_pos),
                        );
                    }
                }
            }

            match editor.world_editing.tool {
                EditorEditingTool::Pencil => {
                    let mut attachment_map = AttachmentMap::new();
                    attachment_map.register_attachment(Attachment::PTMATERIAL);
                    apply_edit(
                        &mut voxel_world,
                        &editor,
                        &trace,
                        size,
                        attachment_map,
                        |center| {
                            let size = size.clone();
                            let color = editor.world_editing.color.clone();
                            Box::new(move |mut voxel, world_voxel_pos, local_voxel_pos| {
                                let distance = center
                                    .cast::<f32>()
                                    .metric_distance(&world_voxel_pos.cast::<f32>());
                                if distance <= size as f32 {
                                    voxel.set_attachment(
                                        Attachment::PTMATERIAL_ID,
                                        &[PTMaterial::diffuse(color.clone()).encode()],
                                    );
                                }
                            })
                        },
                    );
                }
                EditorEditingTool::Brush => {}
                EditorEditingTool::Eraser => {
                    let mut attachment_map = AttachmentMap::new();
                    apply_edit(
                        &mut voxel_world,
                        &editor,
                        &trace,
                        size,
                        attachment_map,
                        |center| {
                            Box::new(move |mut voxel, world_voxel_pos, local_voxel_pos| {
                                let distance = center
                                    .cast::<f32>()
                                    .metric_distance(&world_voxel_pos.cast::<f32>());
                                if distance <= size as f32 {
                                    voxel.set_removed();
                                }
                            })
                        },
                    );
                }
            }
        }

        //debug_renderer.draw_line(DebugLine {
        //    start: editor_camera.rotation_anchor,
        //    end: editor_camera.rotation_anchor,
        //    thickness: 1.0,
        //    color: Color::new_srgb(0.8, 0.1, 0.7),
        //    alpha: 1.0,
        //    flags: DebugFlags::NONE,
        //});
    }

    pub fn update_toggle(
        mut editor: ResMut<Editor>,
        mut main_camera: ResMut<MainCamera>,
        mut ecs_world: ResMut<ECSWorld>,
        mut input: ResMut<Input>,
        mut window: ResMut<Window>,
        mut session: ResMut<Session>,
    ) {
        if editor.is_active && !editor.initialized {
            editor.init_editor_session(&mut session, &mut main_camera, &mut ecs_world);
            main_camera.set_camera(editor.editor_camera_entity.unwrap(), "editor_camera");
        }

        if input.did_action(consts::actions::EDITOR_TOGGLE) {
            if editor.is_active {
                // Switch to game mode.
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
                // Switch to editor mode.
                editor.is_active = true;
                editor.last_main_camera = main_camera.camera.clone();
                if !editor.initialized {
                    editor.init_editor_session(&mut session, &mut main_camera, &mut ecs_world);
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

    pub fn from_pos_anchor(camera_pos: Vector3<f32>, anchor_pos: Vector3<f32>) -> Self {
        let anchor_to_cam = camera_pos - anchor_pos;
        let distance = anchor_to_cam.magnitude();
        let euler = Vector3::new(
            (anchor_to_cam.y / distance).asin(),
            // Flip since nalgebra rotates clockwise.
            -(f32::atan2(anchor_to_cam.z, anchor_to_cam.x) - f32::consts::FRAC_PI_2),
            0.0,
        );

        Self {
            rotation_anchor: anchor_pos,
            euler,
            distance,
        }
    }
}
