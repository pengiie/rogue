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
    editing::{
        voxel_editing::EditorVoxelEditing, voxel_editing_preview_gpu::EditorVoxelEditingPreviewGpu,
    },
    editor_input::EditorInput,
    editor_project_settings::EditorProjectSettings,
    editor_settings::UserEditorSettingsAsset,
    game_session::EditorGameSession,
    gizmo::EditorGizmo,
    render_graph::EditorRenderGraph,
    session::EditorSession,
    ui::EditorUI,
    world::generator::WorldGenerator,
};

pub mod camera_controller;
pub mod editing;
pub mod editor_input;
pub mod editor_project_settings;
pub mod editor_settings;
pub mod game_session;
pub mod gizmo;
mod render_graph;
pub mod session;
pub mod ui;
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

    // Setup game session early since it relys on ProjectSettings.
    let game_session = EditorGameSession::new(&project.settings);
    let mut app = App::new(AppCreateInfo {
        project,
        on_post_graphics_init_fn: Some(Box::new(on_post_graphics_init)),
        on_window_event_fn: Some(Box::new(on_window_event)),
        on_device_event_fn: Some(Box::new(on_device_event)),
    });
    app.insert_resource(editor_settings.user_project_settings);
    app.insert_resource(game_session);

    /// Initialize the saved editor ui layout.
    app.insert_resource(editor_settings.editor_ui);

    setup_systems(&mut app);

    app.run_with_window();
}

/// Called only once after graphics initialization.
fn on_post_graphics_init(rb: &mut ResourceBank) {
    let editor_project_settings = rb.get_resource::<EditorProjectSettings>();
    let project_settings =
        editor_project_settings.get_project_settings(&rb.get_resource::<Assets>());
    let session = EditorSession::new(
        &mut rb.get_resource_mut::<ECSWorld>(),
        &mut rb.get_resource_mut::<MainCamera>(),
        project_settings,
    );
    drop(editor_project_settings);
    rb.insert(session);

    rb.insert(DebugRenderer::new());

    let egui = Egui::new(&rb.get_resource::<Window>());
    rb.insert(egui);
    rb.insert(EguiGpu::new());

    let world_generator = WorldGenerator::new(&rb.get_resource::<Tasks>());
    rb.insert(world_generator);

    rb.insert(EditorVoxelEditing::new());
    rb.insert(EditorVoxelEditingPreviewGpu::new());
    rb.insert(EditorGizmo::new());

    rb.insert(EditorInput::new());

    rb.run_system(EditorRenderGraph::init_render_graph);
}

fn on_window_event(rb: &mut ResourceBank, event: &mut winit::event::WindowEvent) -> bool {
    if rb.has_resource::<Egui>() {
        let window = rb.get_resource::<Window>();
        // We can't force the cursor position on wayland only confine it so ignore any cursor inputs when it is locked.
        if !window.is_cursor_locked() {
            if rb
                .get_resource_mut::<Egui>()
                .handle_window_event(&window, &event)
            {
                return true;
            }
        }
    }

    if let Some(editor_ui) = rb.try_get_resource::<EditorUI>() {
        match event {
            winit::event::WindowEvent::CursorMoved {
                device_id,
                position,
            } => {
                if let Some(mut editor_input) = rb.try_get_resource_mut::<EditorInput>() {
                    editor_input.global_mouse_pos =
                        Vector2::new(position.x as f32, position.y as f32);
                }
                let offset = editor_ui.backbuffer_offset();
                *position = winit::dpi::PhysicalPosition::new(
                    position.x - offset.x as f64,
                    position.y - offset.y as f64,
                );
            }
            _ => {}
        }
    }

    return false;
}

fn on_device_event(rb: &mut ResourceBank, event: &mut winit::event::DeviceEvent) -> bool {
    if !rb.has_resource::<EditorUI>()
        || !rb.has_resource::<EditorInput>()
        || !rb.has_resource::<Window>()
    {
        return false;
    }
    let window = rb.get_resource::<Window>();
    if window.is_cursor_locked() {
        // Locked cursor always belongs to the game window.
        return false;
    }

    let editor_ui = rb.get_resource_mut::<EditorUI>();
    let editor_input = rb.get_resource::<EditorInput>();
    let window = rb.get_resource::<Window>();
    let window_size = window.size();
    let content_padding = editor_ui.content_padding();
    let mouse_pos = editor_input.global_mouse_pos.map(|x| x.floor() as u32);
    let mouse_in_game_window = mouse_pos.x > content_padding.z
        && mouse_pos.x < window_size.x - content_padding.w
        && mouse_pos.y > content_padding.x
        && mouse_pos.y < window_size.y - content_padding.y;

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

fn setup_systems(app: &mut App) {
    // Update editor session raycast which is re-used throughout the frame.
    app.insert_system(AppStage::Update, EditorSession::update_raycast);
    // Update editor camera controller and EditorSession::is_editor_camera_focused().
    app.insert_system(
        AppStage::Update,
        EditorSession::update_editor_camera_controller,
    );
    // Update editor gizmo actions and rendering.
    // Do this before updating the selected entity since the gizmo can consume clicks.
    app.insert_system(AppStage::Update, EditorGizmo::update);
    app.insert_system(AppStage::Update, EditorGizmo::visualize_selected_entity);
    // Update editor session selected entity based on the raycast.
    app.insert_system(AppStage::Update, EditorSession::update_selected_entity);

    // Update editor voxel editing for entities and terrain.
    app.insert_system(
        AppStage::Update,
        EditorVoxelEditing::update_voxel_editing_systems,
    );
    // Ensure the preview model exists on the gpu voxel registry.
    app.insert_system(
        AppStage::Update,
        EditorVoxelEditingPreviewGpu::update_preview_gpu,
    );
    // Render the selections.
    app.insert_system(
        AppStage::Update,
        EditorVoxelEditingPreviewGpu::update_selections_preview_gpu,
    );

    // Update the voxel-based world generator.
    app.insert_system(AppStage::Update, WorldGenerator::update);

    // Handles events such as project/settings saving and loading.
    app.insert_system(AppStage::Update, EditorSession::update_editor_events);

    // Update game state from any events.
    app.insert_system(
        AppStage::Update,
        EditorGameSession::update_game_session_state,
    );
    // Insert game script systems which run conditionally on editor game state.
    app.insert_system(AppStage::Update, EditorGameSession::try_run_game_on_update);
    app.insert_system(
        AppStage::FixedUpdate,
        EditorGameSession::try_run_game_on_fixed_update,
    );

    // Calls the immediate mode ui stuff.
    app.insert_system(AppStage::RenderWrite, EditorUI::resolve_egui_ui);

    app.insert_system(
        AppStage::PreUniformsRenderWrite,
        EditorRenderGraph::write_general_inputs,
    );
    // Write the world raytrace pass.
    app.insert_system(AppStage::RenderWrite, WorldRTPass::write_graph_rt_pass);
    // Write the images and vertex/index buffers to render the ui.
    app.insert_system(AppStage::RenderWrite, EguiGpu::write_render_data);
    // Write the render graph pass input for rasterizing the ui.
    app.insert_system(AppStage::RenderWrite, EguiGpu::write_ui_pass);

    // Write the render graph pass input for rasterizing the debug shapes.
    app.insert_system(AppStage::RenderWrite, DebugRenderer::write_graph_pass);
    app.insert_system(
        AppStage::RenderWrite,
        EditorVoxelEditingPreviewGpu::write_render_preview_pass,
    );
}

fn init_ecs_world() -> ECSWorld {
    let mut e = ECSWorld::new();
    rogue_game::register_game_types(&mut e);
    e
}
