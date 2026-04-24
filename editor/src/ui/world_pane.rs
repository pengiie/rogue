use rogue_engine::{
    asset::asset::GameAssetPath,
    egui::egui_util,
    world::{sky::Sky, terrain::region_map::RegionMapCommandEvent},
};

use crate::ui::{EditorCommand, FilePickerType, pane::EditorUIPane};

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
                ui.horizontal(|ui| {
                    let save_path = ctx.region_map.disk.as_ref().map(|disk| disk.regions_dir());
                    ui.label("Terrain:");
                    let (res, new_path) =
                        ui.dnd_drop_zone::<GameAssetPath, _>(egui::Frame::new(), |ui| {
                            let asset_title = match save_path {
                                Some(path) => path.as_relative_dir_path_str(),
                                None => "None".to_string(),
                            };
                            ui.menu_button(asset_title, |ui| {
                                if ui
                                    .add_enabled(save_path.is_some(), egui::Button::new("Save"))
                                    .clicked()
                                {
                                    ctx.events.push(RegionMapCommandEvent::Save);
                                    ui.close_menu();
                                }
                            });
                        });
                    if let Some(new_path) = new_path
                        && save_path != Some(&*new_path)
                    {
                        ctx.events.push(RegionMapCommandEvent::SetRegionsDir {
                            region_dir: (*new_path).clone(),
                        });
                    }
                });
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
