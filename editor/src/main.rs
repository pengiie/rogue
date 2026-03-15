#![allow(warnings)]

use std::path::PathBuf;

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
};
use winit::event::{DeviceEvent, ElementState};

use crate::{
    editor_settings::UserEditorSettingsAsset, game_session::GameSession, gizmo::EditorGizmo,
    render_graph::EditorRenderGraph, session::EditorSession, ui::EditorUI,
    voxel_editing::EditorVoxelEditing, world::generator::WorldGenerator,
};

pub mod camera_controller;
pub mod editor_settings;
pub mod game_session;
pub mod gizmo;
mod render_graph;
pub mod session;
pub mod ui;
pub mod voxel_editing;
pub mod world;

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

    let editor_settings = UserEditorSettingsAsset::load_editor_settings();
    let project = editor_settings.load_project();

    let game_session = GameSession {
        game_camera: project.settings.game_camera,
    };

    let mut app = App::new(AppCreateInfo {
        project,
        on_post_graphics_init_fn: Some(Box::new(on_post_graphics_init)),
        on_window_event_fn: Some(Box::new(on_window_event)),
        on_device_event_fn: Some(Box::new(on_device_event)),
    });
    app.insert_resource(game_session);

    /// Initialize the saved editor ui layout.
    app.insert_resource(editor_settings.editor_ui);

    setup_editor_systems(&mut app);

    app.run_with_window();
}

/// Called only once after graphics initialization.
fn on_post_graphics_init(rb: &mut ResourceBank) {
    let session = EditorSession::new(
        &mut rb.get_resource_mut::<ECSWorld>(),
        &mut rb.get_resource_mut::<MainCamera>(),
    );
    rb.insert(session);

    rb.insert(DebugRenderer::new());

    let egui = Egui::new(&rb.get_resource::<Window>());
    rb.insert(egui);
    rb.insert(EguiGpu::new());

    let world_generator = WorldGenerator::new(&rb.get_resource::<Tasks>());
    rb.insert(world_generator);

    rb.insert(EditorVoxelEditing::new());
    rb.insert(EditorGizmo::new());

    rb.run_system(EditorRenderGraph::init_render_graph);
}

fn on_window_event(rb: &mut ResourceBank, event: &winit::event::WindowEvent) -> bool {
    if rb.has_resource::<Egui>() {
        let window = rb.get_resource::<Window>();
        // We can't force the cursor position on wayland only confine it so ignore any cursor inputs when it is locked.
        if !window.is_cursor_locked() {
            return rb
                .get_resource_mut::<Egui>()
                .handle_window_event(&window, &event);
        }
    }
    return false;
}

fn on_device_event(rb: &mut ResourceBank, event: &winit::event::DeviceEvent) -> bool {
    if !rb.has_resource::<EditorUI>() || !rb.has_resource::<Input>() || !rb.has_resource::<Window>()
    {
        return false;
    }
    let window = rb.get_resource::<Window>();
    if window.is_cursor_locked() {
        // Locked cursor always belongs to the game window.
        return false;
    }

    let editor_ui = rb.get_resource_mut::<EditorUI>();
    let padding = editor_ui.content_padding();
    let input = rb.get_resource::<Input>();
    let window_size = rb.get_resource::<Window>().size();
    let mouse_pos = input.mouse_position().map(|x| x.floor() as u32);
    let mouse_in_game_window = mouse_pos.x >= padding.z
        && mouse_pos.x <= window_size.x - padding.w
        && mouse_pos.y >= padding.x
        && mouse_pos.y <= window_size.y - padding.y;

    match event {
        DeviceEvent::MouseWheel { .. } | DeviceEvent::Key(_) => {
            return !mouse_in_game_window;
        }
        DeviceEvent::Button { state, .. } => match state {
            ElementState::Pressed => {}
            ElementState::Released => {
                return !mouse_in_game_window;
            }
        },
        _ => {}
    }

    return false;
}

fn setup_editor_systems(app: &mut App) {
    app.insert_system(
        AppStage::Update,
        EditorSession::update_selected_entity_and_raycast,
    );
    app.insert_system(
        AppStage::Update,
        EditorSession::update_editor_camera_controller,
    );

    app.insert_system(
        AppStage::Update,
        EditorVoxelEditing::update_voxel_editing_entity,
    );

    app.insert_system(AppStage::Update, WorldGenerator::update);
    // Handles events such as project/settings saving and loading.
    app.insert_system(AppStage::Update, EditorSession::update_editor_events);

    app.insert_system(AppStage::Update, EditorGizmo::update);
    app.insert_system(AppStage::Update, EditorGizmo::visualize_selected_entity);

    // Calls the immediate mode ui stuff.
    app.insert_system(AppStage::RenderWrite, EditorUI::resolve_egui_ui);

    app.insert_system(
        AppStage::PreUniformsRenderWrite,
        EditorRenderGraph::write_general_inputs,
    );
    // Write the images and vertex/index buffers to render the ui.
    app.insert_system(AppStage::RenderWrite, EguiGpu::write_render_data);
    // Write the render graph pass input for rasterizing the ui.
    app.insert_system(AppStage::RenderWrite, EguiGpu::write_ui_pass);

    // Write the render graph pass input for rasterizing the debug shapes.
    app.insert_system(AppStage::RenderWrite, DebugRenderer::write_graph_pass);
}

fn init_ecs_world() -> ECSWorld {
    let mut e = ECSWorld::new();
    // TODO: Register game components
    e
}
