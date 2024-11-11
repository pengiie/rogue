use egui::output;
use parking_lot::Mutex;
use rogue_macros::Resource;

use crate::engine::window::window::Window;

/// Rendering found in graphics/egui.rs
#[derive(Resource)]
pub struct Egui {
    ctx: egui::Context,
    primary_state: Mutex<egui_winit::State>,
    viewport_info: egui::ViewportInfo,

    textures_delta: Option<egui::TexturesDelta>,
    primitives: Vec<egui::ClippedPrimitive>,
}

impl Egui {
    pub fn new(window: &Window) -> Self {
        let ctx = egui::Context::default();

        ctx.set_embed_viewports(true);
        ctx.set_visuals(egui::Visuals::dark());
        ctx.set_zoom_factor(1.0);

        Self {
            ctx: ctx.clone(),
            primary_state: Mutex::new(egui_winit::State::new(
                ctx,
                egui::ViewportId::ROOT,
                window,
                None,
                None,
                None,
            )),
            viewport_info: egui::ViewportInfo::default(),

            textures_delta: None,
            primitives: Vec::new(),
        }
    }

    pub fn textures_delta(&self) -> Option<&egui::TexturesDelta> {
        self.textures_delta.as_ref()
    }

    pub fn primitives(&self) -> &[egui::ClippedPrimitive] {
        self.primitives.as_slice()
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

    pub fn pixels_per_point(&self) -> f32 {
        self.ctx.pixels_per_point()
    }

    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn context_mut(&mut self) -> &mut egui::Context {
        &mut self.ctx
    }

    pub fn resolve_ui(&mut self, window: &Window, mut func: impl FnMut(&egui::Context)) {
        egui_winit::update_viewport_info(
            &mut self.viewport_info,
            &self.ctx,
            window.handle(),
            window.is_first_frame(),
        );

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

        self.primitives = self
            .ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        self.textures_delta = Some(full_output.textures_delta);
    }
}
