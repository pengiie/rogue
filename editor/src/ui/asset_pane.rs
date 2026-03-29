use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use rogue_engine::asset::asset::GameAssetPath;

use crate::ui::{EditorCommand, asset_properties_pane::AssetPropertiesPane, pane::EditorUIPane};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default = "AssetsPane::new")]
pub struct AssetsPane {
    root_folder: AssetFolder,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default = "AssetFolder::default")]
struct AssetFolder {
    folder_absolute_path: PathBuf,
    open: bool,

    #[serde(skip)]
    needs_reload: bool,
    #[serde(skip)]
    files: Vec<AssetItem>,
    folders: Vec<AssetFolder>,
}

impl AssetFolder {
    pub fn default() -> Self {
        Self {
            folder_absolute_path: PathBuf::new(),
            needs_reload: true,
            files: Vec::new(),
            folders: Vec::new(),
            open: false,
        }
    }
}

impl AssetFolder {
    pub fn new(folder_path: PathBuf, open: bool) -> Self {
        Self {
            folder_absolute_path: folder_path,
            needs_reload: true,
            files: Vec::new(),
            folders: Vec::new(),
            open,
        }
    }

    pub fn try_reload(&mut self, asset_dir_path: &Path) {
        if !self.open {
            return;
        }

        if self.needs_reload {
            self.needs_reload = false;
            self.files.clear();
            assert!(self.folder_absolute_path.is_dir());

            let present_folders = self
                .folders
                .iter()
                .map(|f| f.folder_absolute_path.clone())
                .collect::<HashSet<_>>();
            let mut new_folders = HashSet::new();
            for entry in self.folder_absolute_path.read_dir().unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let relative_path = path.strip_prefix(asset_dir_path).unwrap();
                if path.is_file() {
                    self.files.push(AssetItem {
                        file_path: relative_path.to_path_buf(),
                    });
                } else if path.is_dir() {
                    if !present_folders.contains(&path) {
                        self.folders.push(AssetFolder::new(path.clone(), false));
                    }
                    new_folders.insert(path);
                }
            }

            self.folders
                .retain(|f| new_folders.contains(&f.folder_absolute_path));
        }

        for folder in &mut self.folders {
            folder.try_reload(asset_dir_path);
        }
    }

    pub fn show_contents(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        ui.vertical(|ui| {
            for folder in &mut self.folders {
                let response = ui.horizontal(|ui| {
                    let (id, rect) = ui.allocate_space(egui::vec2(6.0, 6.0));
                    let rect_response = ui.interact(rect, id, egui::Sense::click());
                    crate::ui::util::paint_chevron_icon(ui, !folder.open, &rect_response);
                    let folder_str = format!(
                        "/{}",
                        folder
                            .folder_absolute_path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                    );
                    let label_response = ui.label(folder_str);
                    if label_response.clicked() || rect_response.clicked() {
                        folder.open = !folder.open;
                    }
                });

                if folder.open {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        folder.show_contents(ui, ctx);
                    });
                }
            }

            for file in &self.files {
                let file_path_str = file.file_path.to_string_lossy().to_string();
                let label_id = egui::Id::new(&file_path_str);
                let label_dnd_source_id = egui::Id::new(format!("{}_dnd_source", file_path_str));
                let is_hovering = ui.data(|w| w.get_temp(label_id).unwrap_or(false));

                let file_str = file.file_path.file_name().unwrap().to_string_lossy();
                let mut rich_text = egui::RichText::new(file_str.clone());
                if is_hovering {
                    rich_text = rich_text.background_color(egui::Color32::from_white_alpha(2));
                }

                let game_asset_path = GameAssetPath::from_relative_path(&file.file_path);
                let mut label = ui
                    .dnd_drag_source::<GameAssetPath, _>(
                        label_dnd_source_id,
                        game_asset_path.clone(),
                        |ui| ui.add(egui::Label::new(rich_text)),
                    )
                    .response;
                if label.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
                }
                ui.data_mut(|w| w.insert_temp(label_id, label.hovered()));

                if label.clicked() {
                    ctx.commands
                        .push(EditorCommand::open_ui(AssetPropertiesPane::ID));
                    ctx.ui_state.selected_asset = Some(game_asset_path);
                }
            }
        });
    }
}

struct AssetItem {
    // Relative path from the root of the asset directory.
    file_path: PathBuf,
}

impl AssetsPane {
    pub fn new() -> Self {
        Self {
            root_folder: AssetFolder::new(PathBuf::new(), false),
        }
    }

    pub fn update_root_folder(&mut self, folder_path: PathBuf) {
        if self.root_folder.folder_absolute_path != folder_path {
            self.root_folder = AssetFolder::new(folder_path.clone(), true);
        }
        self.root_folder.open = true;
        self.root_folder.try_reload(&folder_path);
    }

    pub fn show_header(ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Assets").size(20.0));
            ui.menu_button("Create", |ui| {
                if ui.button("Voxel model").clicked() {
                    use crate::ui::create_voxel_model_dialog::{
                        CreateVoxelModelDialogCreateInfo, create_voxel_model_dialog,
                    };
                    ctx.commands.push(create_voxel_model_dialog(
                        CreateVoxelModelDialogCreateInfo {
                            target_entity: None,
                        },
                    ));
                    ui.close_menu();
                }
            });
        });
    }
}

impl EditorUIPane for AssetsPane {
    const ID: &'static str = "assets";
    const NAME: &'static str = "Assets";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        let Some(project_assets_dir) = ctx.assets.project_assets_dir() else {
            ui.label("No project loaded.");
            return;
        };
        self.update_root_folder(project_assets_dir);

        Self::show_header(ui, ctx);
        self.root_folder.show_contents(ui, ctx);
    }
}
