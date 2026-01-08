#![allow(warnings)]

use std::path::PathBuf;

use rogue_engine::{
    app::{App, AppCreateInfo, AppStage},
    asset::asset::{AssetPath, Assets},
    consts, impl_asset_load_save_serde,
};

mod init;
mod render_graph;

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

    let mut app = App::new(AppCreateInfo { project: todo!() });
    init::init_post_graphics(&mut app);

    app.run_with_window();
}
