use rogue_engine::{app::App, system::SystemErased};

mod init;

pub fn add_init_resources(app: &mut App) {}

pub fn collect_init_systems() -> Vec<SystemErased> {
    todo!()
}

pub fn collect_on_update_systems() -> Vec<SystemErased> {
    todo!()
}

pub fn collect_on_fixed_update_systems() -> Vec<SystemErased> {
    todo!()
}
