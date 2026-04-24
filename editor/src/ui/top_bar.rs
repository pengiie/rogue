use rogue_engine::{
    entity::RenderableVoxelEntity,
    voxel::voxel::VoxelModelEdit,
    world::renderable::rt_pass::{ShadingMode, WorldRTPass},
};
use strum::VariantArray;

use crate::{
    editing::voxel_editing::EditorVoxelEditingTarget,
    game_session::EditorGameSessionEvent,
    session::EditorCommandEvent,
    ui::{
        EditorCommand, EditorUIContext, animation_pane::AnimationPane, asset_pane::AssetsPane,
        editing_pane::EditingPane, entity_hierarchy::EntityHierarchyUI,
        entity_properties::EntityPropertiesPane, materials_pane::MaterialsPane, pane::EditorUIPane,
        world_pane::WorldPane,
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
                    ctx.events.push(EditorCommandEvent::SaveProject);
                    ctx.events.push(EditorCommandEvent::SaveEditorSettings);
                    ui.close_menu();
                }
                if ui.button("Open").clicked() {}
            });
            ui.menu_button("View", |ui| {});
            ui.menu_button("Open", |ui| {
                if ui.button("Animation").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(AnimationPane::ID));
                    ui.close_menu();
                }
                if ui.button("Assets").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(AssetsPane::ID));
                    ui.close_menu();
                }
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
                if ui.button("Voxel Editing").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(EditingPane::ID));
                    ui.close_menu();
                }
                if ui.button("World").clicked() {
                    ctx.commands.push(EditorCommand::open_ui(WorldPane::ID));
                    ui.close_menu();
                }
            });
            ui.add_enabled_ui(ctx.voxel_editing.is_enabled(), |ui| {
                ui.menu_button("Editing", |ui| {
                    if ui.button("Select terrain").clicked() {
                        ctx.voxel_editing.edit_target = Some(EditorVoxelEditingTarget::Terrain);
                        ui.close_menu();
                    }
                });
            });

            if let Some(project_dir) = &ctx.assets.project_dir() {
                ui.label(format!("{}", project_dir.to_string_lossy()));
            } else {
                ui.label("Please perform File -> New to start a project.");
            }
        });

        // Under the menu bar, quick actions.
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing.x = 4.0;
            if ui
                .add_enabled(
                    ctx.game_session.can_start_game(),
                    egui::Button::new("\u{25B6}"),
                )
                .clicked()
            {
                ctx.events.push(EditorGameSessionEvent::StartGame);
            }
            if ui
                .add_enabled(
                    ctx.game_session.can_pause_game(),
                    egui::Button::new("\u{23F8}"),
                )
                .clicked()
            {
                ctx.events.push(EditorGameSessionEvent::PauseGame);
            }
            if ui
                .add_enabled(
                    ctx.game_session.can_stop_game(),
                    egui::Button::new("\u{25A0}"),
                )
                .clicked()
            {
                ctx.events.push(EditorGameSessionEvent::StopGame);
            }

            let is_editor_camera = ctx.main_camera.camera() == Some(ctx.session.editor_camera());
            if ui
                .add_enabled(!is_editor_camera, egui::Button::new("Scene"))
                .clicked()
            {
                ctx.main_camera
                    .set_camera(ctx.session.editor_camera(), "editor_camera");
            }
            let game_camera = &ctx.game_session.game_camera;
            if ui
                .add_enabled(
                    is_editor_camera && game_camera.is_some(),
                    egui::Button::new("Game"),
                )
                .clicked()
            {
                ctx.main_camera
                    .set_camera(game_camera.unwrap(), "editor_camera");
            }

            for shading_mode in ShadingMode::VARIANTS {
                if ui
                    .add_enabled(
                        &ctx.world_rt_pass.shading_mode != shading_mode,
                        egui::Button::new(shading_mode.to_string()),
                    )
                    .clicked()
                {
                    ctx.world_rt_pass.shading_mode = *shading_mode;
                }
            }
            ui.horizontal(|ui| {
                ui.label("Show colliders:");
                ui.checkbox(&mut ctx.session.render_colliders, "");
            });
        });
    }
}
