use core::f32;
use std::time::Duration;

use hecs::With;
use nalgebra::{
    ComplexField, Rotation3, Translation3, Unit, UnitQuaternion, Vector2, Vector3, Vector4,
};
use rogue_macros::Resource;

use crate::common::geometry::ray::Ray;
use crate::engine::editor::events::EventEditorZoom;
use crate::engine::event::{EventReader, Events};
use crate::{
    common::{animate::Animation, color::Color},
    consts::{
        self,
        editor::gizmo::{DRAGGING_ROTATION_SENSITIVITY, DRAGGING_TRANSFORM_SENSITIVITY},
    },
    engine::{
        asset::asset::{AssetPath, Assets},
        debug::{
            DebugCapsule, DebugFlags, DebugLine, DebugOBB, DebugPlane, DebugRenderer, DebugRing,
        },
        editor::{brush::EditorWorldEditing, gizmo::EditorGizmo, ui::init_editor_ui_textures},
        entity::{
            ecs_world::{ECSWorld, Entity},
            RenderableVoxelEntity,
        },
        graphics::camera::{Camera, MainCamera},
        input::{
            keyboard::{self, Key, Modifier},
            mouse, Input,
        },
        physics::{capsule_collider, collider::Colliders, transform::Transform},
        resource::{Res, ResMut},
        ui::UI,
        voxel::{
            attachment::{
                Attachment, AttachmentId, AttachmentInfoMap, AttachmentMap, BuiltInMaterial,
                PTMaterial,
            },
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

    pub gizmo: EditorGizmo,
    pub selected_entity: Option<Entity>,
    pub hovered_entity: Option<Entity>,

    pub focus_event_reader: EventReader<EventEditorZoom>,
    pub focus_animation: Animation<Vector3<f32>>,
    pub double_clicker_buffer: [Option<Instant>; 2],

    pub world_editing: EditorWorldEditing,
    pub terrain_generation: EditorTerrainGeneration,
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

            focus_event_reader: EventReader::new(),

            gizmo: EditorGizmo::new(),
            selected_entity: None,
            hovered_entity: None,

            focus_animation: Animation::new(Duration::ZERO),
            double_clicker_buffer: [const { None }; 2],

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

    pub fn update_editor_zoom(
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
        events: Res<Events>,
    ) {
        let Some(zoom_event) = editor.focus_event_reader.read(&events).last() else {
            return;
        };

        let target_position = match zoom_event {
            EventEditorZoom::Entity { target_entity } => {
                // Hovered entity with mouse.
                let model_local_transform = ecs_world.get::<&Transform>(*target_entity).unwrap();
                let model_world_transform =
                    ecs_world.get_world_transform(*target_entity, &model_local_transform);
                model_world_transform.position
            }
            EventEditorZoom::Position { position } => *position,
        };

        let mut editor_camera_query = ecs_world
            .query_one::<(&mut Transform, &Camera)>(editor.editor_camera_entity.unwrap())
            .unwrap();
        let (mut editor_transform, camera) = editor_camera_query.get().unwrap();

        let focus_distance = 16.0;
        let end = target_position - editor_transform.forward() * focus_distance;
        editor.focus_animation.start(
            editor_transform.position,
            end,
            Duration::from_secs_f64(0.75),
        );
        editor.editor_camera = EditorCamera::from_pos_anchor(end, target_position);
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
        mut events: ResMut<Events>,
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

        // Updates the current selected gizmo off of any keybinds.
        editor.gizmo.update_gizmo_selection(&input);

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

        let mouse_ray = match editor.curr_editor_view {
            EditorView::PanOrbit => input.mouse_ray(
                ui.content_offset(),
                ui.content_size(window.inner_size_vec2().cast::<f32>()),
                camera.fov(),
                &editor_transform,
            ),
            EditorView::Fps => editor_transform.get_ray(),
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

        // Update which gizmo axis is selected or hovered.
        let mut consume_left_click = false;
        if let Some(selected_entity) = editor.selected_entity {
            editor.gizmo.update_gizmo_axes(
                &input,
                selected_entity,
                &mut consume_left_click,
                &ecs_world,
                &mouse_ray,
            );
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

        // Draw bounding box of hovered entity within editor ui..
        if let Some(hovered_entity) = editor.hovered_entity {
            let is_hovering_selected_entity = editor
                .selected_entity
                .map(|selected_entity| selected_entity == hovered_entity)
                .unwrap_or(false);
            if !is_hovering_selected_entity && show_entity_outlines {
                if let Some(obb) = ecs_world.get_entity_obb(hovered_entity, &voxel_world) {
                    debug_renderer.draw_obb(DebugOBB {
                        obb: &obb,
                        thickness: consts::editor::ENTITY_OUTLINE_THICKNESS,
                        color: Color::new_srgb_hex("#4553ad"),
                        alpha: 0.5,
                        flags: DebugFlags::XRAY,
                    });
                }
            }
        }

        if let Some(VoxelTraceInfo::Entity {
            entity_id: hovered_entity_id,
            voxel_model_id,
            local_voxel_pos,
        }) = &hovered_trace
        {
            // Hovered entity with mouse.
            let model_local_transform = ecs_world.get::<&Transform>(*hovered_entity_id).unwrap();
            let model_world_transform =
                ecs_world.get_world_transform(*hovered_entity_id, &model_local_transform);
            if !(editor.selected_entity.is_some()
                && editor.selected_entity.unwrap() == *hovered_entity_id)
                && show_entity_outlines
            {
                if let Some(obb) = ecs_world.get_entity_obb(*hovered_entity_id, &voxel_world) {
                    debug_renderer.draw_obb(DebugOBB {
                        obb: &obb,
                        thickness: consts::editor::ENTITY_OUTLINE_THICKNESS,
                        color: Color::new_srgb_hex("#4553ad"),
                        alpha: 0.5,
                        flags: DebugFlags::XRAY,
                    });
                }
            }

            // Update double click to focus-zoom camera on entity.
            if has_valid_double_click {
                events.push(EventEditorZoom::Entity {
                    target_entity: *hovered_entity_id,
                });
            }
        }

        // Update double click to focus-zoom camera on terrain.
        if let Some(VoxelTraceInfo::Terrain { world_voxel_pos }) = &hovered_trace {
            // Hovered terrain voxel position.
            if has_valid_double_click {
                // Animate camera onto the terrain pos.
                let voxel_world_position =
                    world_voxel_pos.cast::<f32>() * consts::voxel::VOXEL_METER_LENGTH;
                events.push(EventEditorZoom::Position {
                    position: voxel_world_position,
                });
            }
        }

        // Update the entity transform and render the gizmo.
        if show_entity_outlines {
            if let Some(selected_entity) = editor.selected_entity {
                editor.gizmo.update_and_render(
                    &mut debug_renderer,
                    &input,
                    selected_entity,
                    &ecs_world,
                    editor_transform,
                );
            }
        }

        // Render the selected entity's bounding box if it contains a model.
        if let Some(selected_entity_obb) = editor
            .selected_entity
            .map(|selected_entity| ecs_world.get_entity_obb(selected_entity, &voxel_world))
            .unwrap_or(None)
        {
            debug_renderer.draw_obb(DebugOBB {
                obb: &selected_entity_obb,
                thickness: consts::editor::ENTITY_OUTLINE_THICKNESS,
                color: Color::new_srgb_hex("#1026b3"),
                alpha: 0.5,
                flags: DebugFlags::XRAY,
            });
        }

        // Update any brush actions.
        editor
            .world_editing
            .update_brush(&input, &mut voxel_world, &hovered_trace);

        // Editing circle preview.
        editor.world_editing.render_preview(
            &mut debug_renderer,
            &hovered_trace,
            &ecs_world,
            &voxel_world,
        );

        // Clear any frame state.
        editor.hovered_entity = None;
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
