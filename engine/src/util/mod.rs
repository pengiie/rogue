use std::panic::PanicHookInfo;

pub fn fun_panic_hook(panic_info: &PanicHookInfo<'_>) {
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
}
