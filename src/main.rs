#![allow(warnings)]
mod app;
mod common;
mod consts;
mod engine;
mod game;
mod game_loop;
mod settings;

fn main() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Debug).expect("Couldnt init console logger.");
        } else {
            env_logger::builder()
                .filter(Some("naga"), log::LevelFilter::Info)
                .filter(Some("wgpu_hal"), log::LevelFilter::Info)
                .filter(Some("wgpu_core"), log::LevelFilter::Warn).init();
        }
    }

    app::App::new().run();
}
