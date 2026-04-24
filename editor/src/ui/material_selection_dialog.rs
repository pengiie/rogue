use crate::ui::{EditorCommand, EditorDialog, EditorUIContext};

const DIALOG_ID: &str = "material_selection_dialog";

pub fn material_selection_dialog() -> EditorCommand {
    EditorCommand::OpenDialog(EditorDialog {
        id: DIALOG_ID.to_owned(),
        title: "Select a material".to_owned(),
        show_fn: Box::new(move |ui, ctx| material_selection_dialog_show_fn(ui, ctx)),
    })
}

pub fn material_selection_dialog_show_fn(ui: &mut egui::Ui, ctx: &mut EditorUIContext) -> bool {
    let should_close = egui::ScrollArea::vertical().show(ui, |ui| {
        for (material_id, material_name) in ctx.material_bank.id_to_name.clone().iter() {
            ui.push_id(format!("material_{}", material_id), |ui| {
                if ui.button(material_name).clicked() {
                    ctx.voxel_editing.material = Some(*material_id);
                    ctx.commands
                        .push(EditorCommand::CloseDialog(DIALOG_ID.to_owned()));
                }
            });
        }
    });
    false
}
