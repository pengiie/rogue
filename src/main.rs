mod app;
mod common;
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
            env_logger::init();
        }
    }

    app::App::new().run();
}
