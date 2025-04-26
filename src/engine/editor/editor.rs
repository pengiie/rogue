use core::f32;

use hecs::With;
use rogue_macros::Resource;

use crate::{
    engine::{
        entity::ecs_world::{ECSWorld, Entity},
        graphics::camera::{Camera, MainCamera},
        input::{keyboard, Input},
        physics::transform::Transform,
        resource::{Res, ResMut},
    },
    game::entity::player::Player,
};

#[derive(Resource)]
pub struct Editor {
    last_main_camera: Option<Entity>,
    editor_camera: Option<Entity>,
    is_active: bool,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            last_main_camera: None,
            editor_camera: None,
            is_active: true,
        }
    }

    pub fn update(
        mut editor: ResMut<Editor>,
        mut main_camera: ResMut<MainCamera>,
        mut ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
    ) {
        if editor.is_active && editor.editor_camera.is_none() {}
        if input.is_key_pressed(keyboard::Key::R) {
            if editor.is_active {
                if let Some(last_camera) = editor.last_main_camera {
                    editor.is_active = false;
                    main_camera.set_camera(last_camera, "player_cam");
                } else {
                    let mut player_query = ecs_world.query::<With<(), &Player>>();
                    if let Some((player_entity, _)) = player_query.into_iter().next() {
                        editor.is_active = false;
                        main_camera.set_camera(player_entity, "player_cam");
                    }
                }
            } else {
                editor.is_active = true;
                editor.last_main_camera = main_camera.camera();
                let editor_camera = editor.editor_camera.get_or_insert_with(|| {
                    ecs_world.spawn((
                        EditorCamera::new(),
                        Camera::new(f32::consts::FRAC_PI_2),
                        Transform::new(),
                    ))
                });
                main_camera.set_camera(*editor_camera, "editor_camera");
            }
        }
        if !editor.is_active {
            return;
        }
    }
}

pub struct EditorCamera {}

impl EditorCamera {
    pub fn new() -> Self {
        Self {}
    }

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
