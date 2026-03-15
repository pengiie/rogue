use crate::{
    session::EditorEvent,
    ui::{
        EditorCommand, EditorUIContext, asset_pane::AssetsPane, editing_pane::EditingPane,
        entity_hierarchy::EntityHierarchyUI, entity_properties::EntityPropertiesPane,
        materials_pane::MaterialsPane, pane::EditorUIPane, world_pane::WorldPane,
    },
};

pub struct TopBarPane;

impl TopBarPane {
    pub fn show(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    todo!();
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        ctx.assets.project_dir().is_some(),
                        egui::Button::new("Save"),
                    )
                    .clicked()
                {
                    ctx.events.push(EditorEvent::SaveProject);
                    ctx.events.push(EditorEvent::SaveEditorSettings);
                    ui.close_menu();
                }
                if ui.button("Open").clicked() {}
            });
            ui.menu_button("View", |ui| {});
            ui.menu_button("Open", |ui| {
                if ui.button("Entity Hiearchy").clicked() {
                    ctx.commands
                        .push(EditorCommand::open_ui(EntityHierarchyUI::ID));
                    ui.close_menu();
                }
                if ui.button("Entity Properties").clicked() {
                    ctx.commands
                        .push(EditorCommand::open_ui(EntityPropertiesPane::ID));
                    ui.close_menu();
                }
                if ui.button("Materials").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(MaterialsPane::ID));
                    ui.close_menu();
                }
                if ui.button("World").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(WorldPane::ID));
                    ui.close_menu();
                }
                if ui.button("Assets").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(AssetsPane::ID));
                    ui.close_menu();
                }
                if ui.button("Voxel Editing").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(EditingPane::ID));
                    ui.close_menu();
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label("Good luck :)");
            });

            if let Some(project_dir) = &ctx.assets.project_dir() {
                ui.label(format!("{}", project_dir.to_string_lossy()));
            } else {
                ui.label("Please perform File -> New to start a project.");
            }
        });
    }
}
