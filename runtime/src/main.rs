#![allow(warnings)]

use std::path::PathBuf;

use nalgebra::Vector2;
use rogue_engine::{
    app::{App, AppCreateInfo, AppStage},
    asset::{
        asset::{AssetPath, Assets},
        repr::project::ProjectAsset,
    },
    consts::{self, editor},
    debug::debug_renderer::DebugRenderer,
    egui::{Egui, egui_gpu::EguiGpu},
    entity::ecs_world::ECSWorld,
    graphics::camera::MainCamera,
    impl_asset_load_save_serde,
    input::Input,
    resource::ResourceBank,
    task::tasks::Tasks,
    window::window::Window,
    world::renderable::rt_pass::WorldRTPass,
};
use winit::event::{DeviceEvent, ElementState};

use crate::{
    render_graph::RuntimeRenderGraph, runtime_project_loader::RuntimeProjectLoader,
    runtime_session::RuntimeSession,
};

mod render_graph;
mod runtime_project_loader;
mod runtime_session;

fn main() {
    std::panic::set_hook(Box::new(rogue_engine::util::fun_panic_hook));
    const default_level: log::LevelFilter = log::LevelFilter::Debug;
    env_logger::builder()
        .filter_level(
            std::env::var(env_logger::DEFAULT_FILTER_ENV)
                .ok()
                .map(|filter_str| {
                    <log::LevelFilter as std::str::FromStr>::from_str(&filter_str)
                        .unwrap_or(default_level)
                })
                .unwrap_or(default_level),
        )
        .filter(Some("naga"), log::LevelFilter::Info)
        .filter(Some("wgpu_hal"), log::LevelFilter::Info)
        .filter(Some("wgpu_core"), log::LevelFilter::Warn)
        .filter(Some("sctk"), log::LevelFilter::Info)
        .init();

    let project = RuntimeProjectLoader::load_project();

    // Setup runtime session early since it relys on ProjectSettings.
    let runtime_session = RuntimeSession::new(&project.settings);
    let mut app = App::new(AppCreateInfo {
        project,
        on_post_graphics_init_fn: Some(Box::new(on_post_graphics_init)),
        on_window_event_fn: None,
        on_device_event_fn: None,
    });
    app.insert_resource(runtime_session);

    setup_systems(&mut app);

    app.run_with_window();
}

/// Called only once after graphics initialization.
fn on_post_graphics_init(rb: &mut ResourceBank) {
    rb.run_system(RuntimeSession::init_runtime_session);
    rb.run_system(RuntimeRenderGraph::init_render_graph);
}

fn setup_systems(app: &mut App) {
    // ======== RUNTIME SESSION =========
    // Insert game script systems.
    app.insert_system(AppStage::Update, RuntimeSession::run_game_on_update);
    app.insert_system(
        AppStage::FixedUpdate,
        RuntimeSession::run_game_on_fixed_update,
    );

    // ======== RENDER GRAPH =========
    app.insert_system(
        AppStage::PreUniformsRenderWrite,
        RuntimeRenderGraph::write_general_inputs,
    );
    // Write the world raytrace pass.
    app.insert_system(AppStage::RenderWrite, WorldRTPass::write_graph_rt_pass);
}

fn init_ecs_world() -> ECSWorld {
    let mut e = ECSWorld::new();
    rogue_game::register_game_types(&mut e);
    e
}
