use std::time::Instant;

use gui::Egui;
use state::UIState;

use super::{
    resource::{Res, ResMut},
    window::{time::Time, window::Window},
};

pub mod gui;
pub mod state;

pub struct UI {}

impl UI {
    pub fn update(mut egui: ResMut<Egui>, mut state: ResMut<UIState>, time: Res<Time>) {
        if egui.context().zoom_factor() != state.zoom_factor {
            egui.context_mut().set_zoom_factor(state.zoom_factor);
        }

        if state.last_ui_update.elapsed().as_millis() >= state.polling_time_ms.into() {
            state.last_ui_update = Instant::now();

            state.fps = time.fps();
            state.delta_time_ms = time.delta_time().as_micros() as f32 / 1000.0;
        }
    }

    pub fn draw(window: Res<Window>, mut egui: ResMut<Egui>, mut state: ResMut<UIState>) {
        egui.resolve_ui(&window, |ctx| {
            egui::Window::new("diagnostics")
                .current_pos(egui::pos2(4.0, 4.0))
                .movable(false)
                .show(ctx, |ui| {
                    ui.label(format!("FPS: {}", state.fps));
                    ui.label(format!("Frame time: {}ms", state.delta_time_ms));
                    ui.add(
                        egui::Slider::new(&mut state.player_fov, 10.0..=170.0)
                            .text("fov")
                            .drag_value_speed(0.1),
                    );
                });
        });
    }
}
