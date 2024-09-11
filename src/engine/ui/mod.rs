use gui::Egui;
use state::UIState;

use super::{
    resource::{Res, ResMut},
    window::window::Window,
};

pub mod gui;
pub mod state;

pub struct UI;

impl UI {
    pub fn update(mut egui: ResMut<Egui>, mut state: ResMut<UIState>) {
        if egui.context().zoom_factor() != state.zoom_factor {
            egui.context_mut().set_zoom_factor(state.zoom_factor);
        }
    }

    pub fn draw(window: Res<Window>, mut egui: ResMut<Egui>, mut state: ResMut<UIState>) {
        egui.resolve_ui(&window, |ctx| {
            egui::Window::new("diagnostics")
                .default_open(false)
                .show(ctx, |ui| {
                    ui.add(
                        egui::Slider::new(&mut state.player_fov, 10.0..=170.0)
                            .text("fov")
                            .drag_value_speed(0.1),
                    );
                });
        });
    }
}
