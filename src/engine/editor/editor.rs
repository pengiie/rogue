use core::f32;

use hecs::With;
use nalgebra::{ComplexField, Rotation3, Translation3, Unit, UnitQuaternion, Vector3};
use rogue_macros::Resource;

use crate::{
    common::color::Color,
    consts,
    engine::{
        asset::{
            asset::{AssetPath, Assets},
            repr::editor_settings::EditorSessionAsset,
        },
        debug::{DebugLine, DebugRenderer},
        entity::ecs_world::{ECSWorld, Entity},
        graphics::camera::{Camera, MainCamera},
        input::{keyboard, mouse, Input},
        physics::transform::Transform,
        resource::{Res, ResMut},
    },
    game::entity::player::Player,
    settings::Settings,
};

#[derive(Resource)]
pub struct Editor {
    last_main_camera: Option<(Entity, String)>,
    editor_camera: Option<Entity>,
    pub is_active: bool,

    pub initialized: bool,
}

impl Editor {
    pub fn new() -> Self {
        let mut is_active = std::env::var("ROGUE_EDITOR").is_ok();

        Self {
            last_main_camera: None,
            editor_camera: None,
            is_active,

            initialized: false,
        }
    }

    pub fn init_editor_session(&mut self, main_camera: &mut MainCamera, ecs_world: &mut ECSWorld) {
        assert!(self.editor_camera.is_none());

        let last_session = Assets::load_asset_sync::<EditorSessionAsset>(AssetPath::new_user_dir(
            consts::io::EDITOR_SETTINGS_FILE,
        ))
        .ok();

        let mut camera_pos = Vector3::zeros();
        let mut camera_fov = f32::consts::FRAC_PI_2;
        let mut anchor_pos = Vector3::new(0.0, 2.0, -2.0);
        if let Some(last_session) = last_session {
            camera_pos = last_session.editor_camera_transform.transform.position;
            anchor_pos = last_session.rotation_anchor;
            camera_fov = last_session.editor_camera.camera.fov();
        }

        let anchor_to_cam = Unit::new_normalize(camera_pos - anchor_pos);
        let distance = anchor_pos.magnitude();
        let euler = Vector3::new(
            (anchor_to_cam.y / distance).asin(),
            // Flip since nalgebra rotates clockwise.
            (anchor_to_cam.z).atan2(anchor_to_cam.x) - f32::consts::FRAC_PI_2,
            0.0,
        );
        log::info!("euler is {:?}", euler);
        self.editor_camera = Some(ecs_world.spawn((
            EditorCamera {
                rotation_anchor: anchor_pos,
                euler,
                distance,
            },
            Camera::new(camera_fov),
            Transform::new(),
        )));
        self.initialized = true;
    }

    pub fn update_editor(
        mut editor: ResMut<Editor>,
        input: Res<Input>,
        ecs_world: ResMut<ECSWorld>,
        settings: Res<Settings>,
        mut debug_renderer: ResMut<DebugRenderer>,
    ) {
        let mut editor_camera_query = ecs_world
            .query_one::<(&mut EditorCamera, &mut Transform)>(editor.editor_camera.unwrap())
            .unwrap();
        let (mut editor_camera, mut editor_transform) = editor_camera_query.get().unwrap();

        if input.is_mouse_button_pressed(mouse::Button::Left) {}

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
        });
    }

    pub fn update_toggle(
        mut editor: ResMut<Editor>,
        mut main_camera: ResMut<MainCamera>,
        mut ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
    ) {
        if editor.is_active && !editor.initialized {
            editor.init_editor_session(&mut main_camera, &mut ecs_world);
            main_camera.set_camera(editor.editor_camera.unwrap(), "editor_camera");
        }

        if input.is_key_pressed(keyboard::Key::E) {
            if editor.is_active {
                if let Some(last_camera) = editor.last_main_camera.clone() {
                    main_camera.set_camera(last_camera.0, &last_camera.1);
                    editor.is_active = false;
                } else {
                    let mut player_query = ecs_world.query::<With<(), &Player>>();
                    if let Some((player_entity, _)) = player_query.into_iter().next() {
                        editor.is_active = false;
                        main_camera.set_camera(player_entity, "player_cam");
                    } else {
                        log::warn!("No camera to switch to from the editor.");
                    }
                }
            } else {
                editor.is_active = true;
                editor.last_main_camera = main_camera.camera.clone();
                if !editor.initialized {
                    editor.init_editor_session(&mut main_camera, &mut ecs_world);
                }
                main_camera.set_camera(editor.editor_camera.unwrap(), "editor_camera");
            }
        }
    }
}

pub struct EditorCamera {
    rotation_anchor: Vector3<f32>,
    euler: Vector3<f32>,
    distance: f32,
}

impl EditorCamera {
    pub fn update(ecs_world: ResMut<ECSWorld>, editor: ResMut<Editor>) {
        let Some(editor_cam_entity) = &editor.editor_camera else {
            return;
        };
        let mut cam_query = ecs_world
            .query_one::<&EditorCamera>(*editor_cam_entity)
            .unwrap();
        let (editor_camera) = cam_query.get().unwrap();
    }
}
