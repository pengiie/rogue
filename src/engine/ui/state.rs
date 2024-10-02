use std::time::{Duration, Instant};

use rogue_macros::Resource;

#[derive(Resource)]
pub struct UIState {
    pub zoom_factor: f32,
    pub player_fov: f32,
    pub fps: u32,
    pub delta_time_ms: f32,
    pub polling_time_ms: u32,

    pub last_ui_update: Instant,
}

impl Default for UIState {
    fn default() -> Self {
        Self {
            zoom_factor: 1.0,
            player_fov: 90.0,
            fps: 0,
            delta_time_ms: 0.0,
            polling_time_ms: 250,
            last_ui_update: Instant::now(),
        }
    }
}

impl UIState {}
