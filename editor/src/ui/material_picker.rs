use rogue_engine::material::material_bank::{MaterialAssetId, MaterialBank};

pub fn material_picker(
    ui: &mut egui::Ui,
    material_bank: &mut MaterialBank,
    material_id: &mut MaterialAssetId,
) {
    //egui::ComboBox::from_label("Material")
    //    .selected_text(
    //        material_bank
    //            .get_material(*material_id)
    //            .map(|mat| mat.name.as_str())
    //            .unwrap_or("None"),
    //    )
    //    .show_ui(ui, |ui| {
    //        for (id, material) in material_bank.materials.iter_with_handle() {
    //            ui.selectable_value(material_id, id, &material.name);
    //        }
    //    });
}
