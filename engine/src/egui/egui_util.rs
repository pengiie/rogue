use crate::asset::asset::GameAssetPath;

pub fn game_asset_path_button(
    ui: &mut egui::Ui,
    game_asset_path: &mut Option<GameAssetPath>,
    title: String,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.label(title);
        let (res, new_animation) = ui.dnd_drop_zone::<GameAssetPath, _>(egui::Frame::new(), |ui| {
            let asset_title = match game_asset_path {
                Some(path) => path.as_relative_path_str(),
                None => "None".to_string(),
            };
            ui.menu_button(asset_title, |ui| {
                if let Some(existing_path) = game_asset_path {
                    if ui.button("Remove").clicked() {
                        *game_asset_path = None;
                        ui.close_menu();
                    }
                }
                add_contents(ui);
            });
        });
        if let Some(new_animation) = new_animation {
            *game_asset_path = Some((*new_animation).clone());
        }
    });
}
