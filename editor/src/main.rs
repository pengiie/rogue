#![allow(warnings)]

use std::path::PathBuf;

use rogue_engine::{
    app::{App, AppCreateInfo, AppStage},
    asset::{
        asset::{AssetPath, Assets},
        repr::project::ProjectAsset,
    },
    consts,
    egui::{egui_gpu::EguiGpu, Egui},
    entity::ecs_world::ECSWorld,
    impl_asset_load_save_serde,
    resource::ResourceBank,
    window::window::Window,
};

use crate::{render_graph::EditorRenderGraph, session::Session, ui::EditorUI};

mod render_graph;
pub mod session;
pub mod ui;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct EditorUserSettingsAsset {
    pub last_project_dir: Option<PathBuf>,
}

impl_asset_load_save_serde!(EditorUserSettingsAsset);

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

    let mut editor_settings = Assets::load_asset_sync::<EditorUserSettingsAsset>(
        AssetPath::new_user_dir(consts::io::EDITOR_USER_SETTINGS_FILE),
    )
    .unwrap_or(EditorUserSettingsAsset {
        last_project_dir: None,
    });

    let project = editor_settings
        .last_project_dir
        .as_ref()
        .map(|last_project_dir| {
            ProjectAsset::from_existing_raw(last_project_dir, init_ecs_world())
                .map_err(|err| {
                    log::error!(
                        "Error when trying to deserialize last project. Error: {:?}",
                        err
                    );
                    err
                })
                .ok()
        })
        .flatten()
        .unwrap_or_else(|| ProjectAsset::new_empty(init_ecs_world()));

    let mut app = App::new(AppCreateInfo {
        project,
        post_graphics_fn: Some(Box::new(init_post_graphics)),
    });

    // Calls the immediate mode ui stuff.
    app.insert_system(AppStage::RenderWrite, EditorUI::resolve_egui_ui);

    app.insert_system(
        AppStage::RenderWrite,
        EditorRenderGraph::write_general_inputs,
    );
    // Write the images and vertex/index buffers to render the ui.
    app.insert_system(AppStage::RenderWrite, EguiGpu::write_render_data);
    // Write the render graph pass input for rasterizing the ui.
    app.insert_system(AppStage::RenderWrite, EguiGpu::write_ui_pass);

    app.run_with_window();
}

fn init_post_graphics(rb: &mut ResourceBank) {
    rb.insert(Session::new());
    rb.insert(EditorUI::new());

    let egui = Egui::new(&rb.get_resource::<Window>());
    rb.insert(egui);
    rb.insert(EguiGpu::new());

    rb.run_system(EditorRenderGraph::init_render_graph);
}

fn init_ecs_world() -> ECSWorld {
    let mut e = ECSWorld::new();
    // TODO: Register game components
    e
}
