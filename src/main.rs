#![allow(warnings)]

mod app;
mod common;
mod consts;
mod editor;
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
            let default_level = log::LevelFilter::Debug;
            env_logger::builder()
                .filter_level(
                    std::env::var(env_logger::DEFAULT_FILTER_ENV)
                        .ok()
                        .map(|filter_str| <log::LevelFilter as std::str::FromStr>::from_str(&filter_str).unwrap_or(default_level))
                        .unwrap_or(default_level),
                )
                .filter(Some("naga"), log::LevelFilter::Info)
                .filter(Some("wgpu_hal"), log::LevelFilter::Info)
                .filter(Some("wgpu_core"), log::LevelFilter::Warn)
                .filter(Some("sctk"), log::LevelFilter::Info)
                .init();
        }
    }

    std::panic::set_hook(Box::new(|panic_info| {
        let panic_location = panic_info
            .location()
            .map(|location| location.to_string())
            .unwrap_or("i actually dunno".to_string());
        log::error!(
            "\x1b[1;31mUh oh, ferris is angry \u{1F980}, we got an big error at {}, tsk, tsk...\x1b[0m",
            panic_location
        );

        let ty = panic_info.payload().type_id();
        let panic_message = if ty == std::any::TypeId::of::<&'static str>() {
            panic_info
                .payload()
                .downcast_ref::<&'static str>()
                .unwrap()
                .to_string()
        } else if ty == std::any::TypeId::of::<String>() {
            panic_info
                .payload()
                .downcast_ref::<String>()
                .unwrap()
                .clone()
        } else {
            "Unknown panic message type".to_owned()
        };
        log::error!("\x1b[1;31m{}\x1b[0m", panic_message);
        log::error!("");

        let backtrace_enabled = std::env::var("RUST_BACKTRACE").map_or(false, |env| env == "1");
        if backtrace_enabled {
            log::error!("\x1b[1;31mBacktrace:\x1b[0m");
            let backtrace = std::backtrace::Backtrace::capture();
            for line in backtrace.to_string().lines() {
                log::error!("\x1b[1;31m{}\x1b[0m", line);
            }
        } else {
            log::error!("\x1b[1;31mBacktrace is disabled, enable it with RUST_BACKTRACE=1\x1b[0m");
        }

        std::process::exit(1);
    }));

    // Wayland gives weird issues with the swapchain for me, so default to xwayland.
    // if std::env::var("WAYLAND_DISPLAY").is_ok() {
    //     std::env::remove_var("WAYLAND_DISPLAY");
    // }

    crate::app::App::new().run();
}
