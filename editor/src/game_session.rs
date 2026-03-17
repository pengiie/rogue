use rogue_engine::{
    asset::repr::project::ProjectSettings,
    entity::{
        component::GameComponentCloneContext,
        ecs_world::{ECSWorld, Entity},
    },
    event::{EventReader, Events},
    graphics::camera::MainCamera,
    physics::physics_world::PhysicsWorld,
    resource::{Res, ResMut, ResourceBank},
    voxel::voxel_registry::VoxelModelRegistry,
};
use rogue_macros::Resource;

use crate::session::EditorSession;

pub enum EditorGameSessionEvent {
    StartGame,
    PauseGame,
    StopGame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionGameState {
    Stopped,
    Paused,
    Playing,
}

#[derive(Resource)]
pub struct EditorGameSession {
    pub game_camera: Option<Entity>,
    game_state: SessionGameState,
    saved_game_world: Option<ECSWorld>,

    game_session_event_reader: EventReader<EditorGameSessionEvent>,
}

impl EditorGameSession {
    pub fn new(project_settings: &ProjectSettings) -> Self {
        Self {
            game_camera: project_settings.game_camera,
            game_state: SessionGameState::Stopped,
            saved_game_world: None,

            game_session_event_reader: EventReader::new(),
        }
    }

    pub fn update_game_session_state(
        mut game_session: ResMut<EditorGameSession>,
        events: Res<Events>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut physics_world: ResMut<PhysicsWorld>,
        mut main_camera: ResMut<MainCamera>,
        mut editor_session: ResMut<EditorSession>,
    ) {
        let game_session = &mut *game_session;
        let mut new_game_state = None;

        // Only use resulting event cause its easier to deal with rust when borrowing self not in
        // this loop.
        for event in game_session.game_session_event_reader.read(&events) {
            match event {
                EditorGameSessionEvent::StartGame => {
                    new_game_state = Some(SessionGameState::Playing);
                }
                EditorGameSessionEvent::PauseGame => {
                    new_game_state = Some(SessionGameState::Paused);
                }
                EditorGameSessionEvent::StopGame => {
                    new_game_state = Some(SessionGameState::Stopped);
                }
            }
        }

        let Some(new_game_state) = new_game_state else {
            return;
        };

        match new_game_state {
            SessionGameState::Playing => {
                if game_session.game_state == SessionGameState::Paused {
                    game_session.resume_game(&mut physics_world);
                } else {
                    game_session.start_game(
                        &mut ecs_world,
                        &mut physics_world,
                        &mut voxel_registry,
                        &mut main_camera,
                    );
                }
            }
            SessionGameState::Paused => {
                game_session.pause_game(&mut physics_world);
            }
            SessionGameState::Stopped => {
                game_session.stop_game(
                    &mut ecs_world,
                    &mut physics_world,
                    &mut voxel_registry,
                    &mut editor_session,
                    &mut main_camera,
                );
            }
        }
        game_session.game_state = new_game_state;
    }

    pub fn can_start_game(&self) -> bool {
        self.game_state != SessionGameState::Playing && self.game_camera.is_some()
    }

    pub fn can_pause_game(&self) -> bool {
        self.game_state == SessionGameState::Playing
    }

    pub fn can_stop_game(&self) -> bool {
        self.game_state != SessionGameState::Stopped
    }

    pub fn resume_game(&mut self, physics_world: &mut PhysicsWorld) {
        assert_eq!(self.game_state, SessionGameState::Paused);
        physics_world.do_dynamics = true;
    }

    pub fn start_game(
        &mut self,
        ecs_world: &mut ECSWorld,
        physics_world: &mut PhysicsWorld,
        voxel_registry: &mut VoxelModelRegistry,
        main_camera: &mut MainCamera,
    ) {
        assert!(self.can_start_game() && self.game_state == SessionGameState::Stopped);

        let fresh_ecs_world = ecs_world.clone_game_entities(&mut GameComponentCloneContext {
            voxel_registry,
            collider_registry: &mut physics_world.colliders,
        });
        self.saved_game_world = Some(fresh_ecs_world);
        main_camera.set_camera(self.game_camera.clone().unwrap(), "game_camera");

        physics_world.do_dynamics = true;
    }

    pub fn pause_game(&mut self, physics_world: &mut PhysicsWorld) {
        assert_eq!(self.game_state, SessionGameState::Playing);
        physics_world.do_dynamics = false;
    }

    pub fn stop_game(
        &mut self,
        ecs_world: &mut ECSWorld,
        physics_world: &mut PhysicsWorld,
        voxel_registry: &mut VoxelModelRegistry,
        editor_session: &mut EditorSession,
        main_camera: &mut MainCamera,
    ) {
        assert_ne!(self.game_state, SessionGameState::Stopped);
        let saved_game_world = self.saved_game_world.take().unwrap();
        *ecs_world = saved_game_world;

        let editor_camera = EditorSession::init_editor_camera(ecs_world);
        main_camera.set_camera(editor_camera, "editor_camera");
        physics_world.do_dynamics = false;
    }

    pub fn try_run_game_on_update(rb: &ResourceBank) {
        let game_state = rb.get_resource::<EditorGameSession>().game_state;
        if game_state != SessionGameState::Playing {
            return;
        }

        log::info!("Running game update");
    }

    pub fn try_run_game_on_fixed_update(rb: &ResourceBank) {
        let game_state = rb.get_resource::<EditorGameSession>().game_state;
        if game_state != SessionGameState::Playing {
            return;
        }

        log::info!("Running game fixed update");
    }
}
