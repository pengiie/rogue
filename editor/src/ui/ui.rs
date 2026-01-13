use nalgebra::Vector4;
use rogue_engine::{
    egui::Egui,
    entity::ecs_world::ECSWorld,
    resource::{Res, ResMut, Resource},
    window::window::Window,
};
use rogue_macros::Resource;

use crate::ui::entity_properties::EntityPropertiesPane;

/// Context that we pass down to every component so we don't have 10 argument functions.
pub struct EditorUIContext<'a> {
    pub ecs_world: &'a mut ECSWorld,
}

#[derive(Resource)]
pub struct EditorUI {
    /// Top, bottom, left, right
    content_padding: Vector4<u32>,
}

impl EditorUI {
    pub fn new() -> Self {
        Self {
            content_padding: Vector4::zeros(),
        }
    }

    pub fn content_padding(&self) -> &Vector4<u32> {
        &self.content_padding
    }

    pub fn resolve_egui_ui(
        mut editor_ui: ResMut<EditorUI>,
        mut egui: ResMut<Egui>,
        mut window: ResMut<Window>,
        mut ecs_world: ResMut<ECSWorld>,
    ) {
        egui.resolve_ui(&mut window, |ctx, window| {
            let frame = egui::Frame::new()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(6.0);

            let mut res_ctx = EditorUIContext {
                ecs_world: &mut ecs_world,
            };
            let mut padding = Vector4::zeros();
            padding.x =
                egui::TopBottomPanel::new(egui::panel::TopBottomSide::Top, "editor_top_panel")
                    .frame(frame.clone())
                    .show(ctx, |ui| {
                        editor_ui.top_pane_ui(ui);
                    })
                    .response
                    .rect
                    .height();
            padding.y = egui::TopBottomPanel::new(
                egui::panel::TopBottomSide::Bottom,
                "editor_bottom_panel",
            )
            .frame(frame.clone())
            .resizable(true)
            .show(ctx, |ui| {
                editor_ui.bottom_pane_ui(ui);
            })
            .response
            .rect
            .height();
            padding.z = egui::SidePanel::new(egui::panel::Side::Left, "editor_left_panel")
                .resizable(true)
                .frame(frame.clone())
                .show(ctx, |ui| {
                    editor_ui.left_pane_ui(ui);
                })
                .response
                .rect
                .width();
            padding.w = egui::SidePanel::new(egui::panel::Side::Right, "editor_right_panel")
                .resizable(true)
                .frame(frame.clone())
                .show(ctx, |ui| {
                    editor_ui.right_pane_ui(ui, &mut res_ctx);
                })
                .response
                .rect
                .width();

            editor_ui.content_padding = (padding * ctx.pixels_per_point()).map(|x| x as u32);
        });
    }

    fn left_pane_ui(&mut self, ui: &mut egui::Ui) {}

    fn right_pane_ui(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        EntityPropertiesPane::show(ui, ctx);
    }

    fn top_pane_ui(&mut self, ui: &mut egui::Ui) {}

    fn bottom_pane_ui(&mut self, ui: &mut egui::Ui) {}
}
