use rogue_engine::{
    asset::repr::project::ProjectSettings,
    entity::{
        component::GameComponentCloneContext,
        ecs_world::{ECSWorld, Entity},
    },
    event::{EventReader, Events},
    graphics::camera::MainCamera,
    input::Input,
    physics::physics_world::PhysicsWorld,
    resource::{Res, ResMut, ResourceBank},
    voxel::voxel_registry::VoxelModelRegistry,
};
use rogue_macros::Resource;

#[derive(Resource)]
pub struct RuntimeSession {
    pub game_camera: Entity,
}

impl RuntimeSession {
    pub fn new(project_settings: &ProjectSettings) -> Self {
        Self {
            game_camera: project_settings
                .game_camera
                .expect("Project doesn't have a defined game camera."),
        }
    }

    pub fn init_runtime_session(
        runtime_session: Res<RuntimeSession>,
        mut main_camera: ResMut<MainCamera>,
        mut physics_world: ResMut<PhysicsWorld>,
    ) {
        main_camera.set_camera(runtime_session.game_camera, "runtime_default_camera");
        physics_world.do_dynamics = true;
    }

    pub fn run_game_on_update(rb: &ResourceBank) {
        rogue_game::on_update(rb);
    }

    pub fn run_game_on_fixed_update(rb: &ResourceBank) {
        rogue_game::on_fixed_update(rb);
    }
}
