use parking_lot::Mutex;
use voxei_macros::Resource;

use crate::engine::window::window::Window;

/// Rendering found in graphics/egui.rs
#[derive(Resource)]
pub struct Egui {
    ctx: egui::Context,
    primary_state: Mutex<egui_winit::State>,
    viewport_info: egui::ViewportInfo,
}

impl Egui {
    pub fn new(window: &Window) -> Self {
        let ctx = egui::Context::default();

        ctx.set_embed_viewports(true);

        Self {
            ctx: ctx.clone(),
            primary_state: Mutex::new(egui_winit::State::new(
                ctx,
                egui::ViewportId::ROOT,
                window,
                None,
                None,
            )),
            viewport_info: egui::ViewportInfo::default(),
        }
    }

    /// Returns if the event was consumed.
    pub fn handle_window_event(
        &mut self,
        window: &Window,
        window_event: &winit::event::WindowEvent,
    ) -> bool {
        let response = self
            .primary_state
            .get_mut()
            .on_window_event(window.handle(), window_event);

        response.consumed
    }

    pub fn draw_ui(&mut self, window: &Window, func: impl FnOnce(&egui::Context) -> ()) {
        egui_winit::update_viewport_info(&mut self.viewport_info, &self.ctx, window.handle());

        let mut raw_input = self
            .primary_state
            .get_mut()
            .take_egui_input(window.handle());

        raw_input.viewport_id = egui::ViewportId::ROOT;
        raw_input
            .viewports
            .insert(egui::ViewportId::ROOT, self.viewport_info.clone());

        let full_output = self
            .primary_state
            .get_mut()
            .egui_ctx()
            .run(raw_input, |ui| {
                func(ui);
            });

        let triangles = self
            .ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
    }
}
