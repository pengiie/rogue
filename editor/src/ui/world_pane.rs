use rogue_engine::world::sky::Sky;

use crate::ui::pane::EditorUIPane;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct WorldPane;

impl WorldPane {
    pub fn new() -> Self {
        Self
    }

    pub fn show_header(ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("World").size(20.0));
        });
    }

    pub fn show_generator_section(ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        egui::CollapsingHeader::new("Terrain")
            .default_open(true)
            .show_unindented(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Generation Enabled");
                    let mut enabled = !ctx.world_generator.paused;
                    ui.checkbox(&mut enabled, "");
                    ctx.world_generator.paused = !enabled;
                });
                ui.horizontal(|ui| ui.label("Terrain: "));
            });
        egui::CollapsingHeader::new("Sky")
            .default_open(true)
            .show_unindented(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Time of day");
                    ui.add(
                        egui::Slider::new(&mut ctx.sky.time_of_day_secs, 0.0..=Sky::SECS_PER_DAY)
                            .show_value(true),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Do day/night cycle");
                    ui.checkbox(&mut ctx.sky.do_day_night_cycle, "");
                });
            });
    }
}

impl EditorUIPane for WorldPane {
    const ID: &'static str = "world_info";
    const NAME: &'static str = "World";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        Self::show_header(ui, ctx);
        Self::show_generator_section(ui, ctx);
    }
}
