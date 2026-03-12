use std::{collections::HashSet, path::PathBuf};

use rogue_engine::{
    asset::asset::Assets,
    entity::ecs_world::{ECSWorld, Entity},
    event::{EventReader, Events},
    graphics::camera::{Camera, MainCamera},
    input::Input,
    material::MaterialBank,
    physics::{physics_world::PhysicsWorld, transform::Transform},
    resource::{Res, ResMut},
    voxel::voxel_registry::VoxelModelRegistry,
    window::{time::Time, window::Window},
    world::region_map::RegionMap,
};
use rogue_macros::Resource;

use crate::{
    camera_controller::EditorCameraController,
    editor_settings::{UserEditorSettingsAsset, UserEditorSettingsAssetProxy},
    game_session::GameSession,
    ui::EditorUI,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorEvent {
    SaveEditorSettings,
    SaveProject,
}

pub enum SessionGameState {
    Stopped,
    Paused,
    Playing,
}

#[derive(Resource)]
pub struct EditorSession {
    pub selected_entity: Option<Entity>,
    pub hovered_entity: Option<Entity>,
    session_game_state: SessionGameState,

    editor_camera: Entity,
    editor_camera_controller: EditorCameraController,

    editor_event_reader: EventReader<EditorEvent>,
}

impl EditorSession {
    pub fn new(ecs_world: &mut ECSWorld, main_camera: &mut MainCamera) -> Self {
        let editor_camera = Self::init_editor_camera(ecs_world);
        main_camera.set_camera(editor_camera, "editor_camera");

        Self {
            selected_entity: None,
            hovered_entity: None,
            session_game_state: SessionGameState::Stopped,
            editor_camera,
            editor_camera_controller: EditorCameraController::new(),
            editor_event_reader: EventReader::new(),
        }
    }

    fn init_editor_camera(ecs_world: &mut ECSWorld) -> Entity {
        ecs_world.spawn((Transform::new(), Camera::new(90.0f32.to_radians())))
    }

    pub fn update_editor_camera_controller(
        mut session: ResMut<EditorSession>,
        ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
        time: Res<Time>,
        mut window: ResMut<Window>,
    ) {
        let camera_transform = &mut ecs_world
            .get::<&mut Transform>(session.editor_camera)
            .unwrap();
        session
            .editor_camera_controller
            .update(camera_transform, &input, &time, &mut window);
    }

    pub fn editor_camera_controller(&self) -> &EditorCameraController {
        &self.editor_camera_controller
    }

    pub fn update_editor_events(
        assets: Res<Assets>,
        editor_ui: Res<EditorUI>,
        events: Res<Events>,
        mut session: ResMut<EditorSession>,
        ecs_world: Res<ECSWorld>,
        physics_world: Res<PhysicsWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        material_bank: Res<MaterialBank>,
        main_camera: Res<MainCamera>,
        region_map: Res<RegionMap>,
        game_session: Res<GameSession>,
    ) {
        let mut unique_events = HashSet::new();
        for event in session.editor_event_reader.read(&events) {
            unique_events.insert(event);
        }

        for event in unique_events {
            match event {
                EditorEvent::SaveEditorSettings => {
                    log::info!("Saving editor settings");
                    let editor_settings = UserEditorSettingsAssetProxy {
                        last_project_dir: assets.project_dir(),
                        editor_ui: &editor_ui,
                    };
                    editor_settings.save_settings();
                }
                EditorEvent::SaveProject => {
                    log::info!("Saving project");
                    assets.save_project(
                        rogue_engine::asset::repr::project::ProjectSerializeContext {
                            ecs_world: &ecs_world,
                            physics_world: &physics_world,
                            voxel_registry: &voxel_registry,
                            material_bank: &material_bank,
                            main_camera: &main_camera,
                            region_map: &region_map,
                            game_camera: game_session.game_camera.clone(),
                        },
                    );
                }
            }
        }
    }
}
