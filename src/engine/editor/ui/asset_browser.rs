use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use egui::Sense;

use crate::{engine::ui::EditorUIState, session::Session};

pub enum EditorAssetBrowserNode {
    Asset(EditorAssetBrowserAsset),
    Folder {
        /// Path starting from `/project_dir/assets/`.
        asset_sub_path: PathBuf,
        /// None if contents haven't been loaded yet.
        contents: Option<HashMap</*relative_path=*/ PathBuf, EditorAssetBrowserNode>>,
        collapsed: bool,
    },
}

impl EditorAssetBrowserNode {
    pub fn reload(&mut self, project_assets_dir: &Path) -> anyhow::Result<()> {
        assert!(project_assets_dir.ends_with("assets"));

        let Self::Folder {
            asset_sub_path,
            contents: prev_contents,
            ..
        } = self
        else {
            panic!("Reload should only be called on folders.");
        };

        let full_path = project_assets_dir.join(asset_sub_path);
        let Ok(iter) = std::fs::read_dir(&full_path) else {
            anyhow::bail!("Failed to read: {}", full_path.to_string_lossy());
        };

        let mut new_contents = iter
            .filter_map(|e| {
                e.ok()
                    .map(|e| {
                        let entry_path = e.path();
                        let mut entry_sub_path = entry_path
                            .strip_prefix(&project_assets_dir)
                            .unwrap()
                            .to_owned();
                        let node = std::fs::metadata(&entry_path).map(|metadata| {
                            if metadata.is_dir() {
                                EditorAssetBrowserNode::Folder {
                                    asset_sub_path: entry_sub_path.clone(),
                                    contents: None,
                                    collapsed: true,
                                }
                            } else {
                                EditorAssetBrowserNode::Asset(EditorAssetBrowserAsset {
                                    file_sub_path: entry_sub_path.clone(),
                                })
                            }
                        });
                        node.ok().map(|node| (entry_sub_path, node))
                    })
                    .unwrap_or(None)
            })
            .collect::<HashMap<_, _>>();

        // Reload any non-collapsed children of this folder.
        for (folder_path, folder) in new_contents.iter_mut() {
            match folder {
                EditorAssetBrowserNode::Folder { collapsed, .. } => {
                    let Some(prev_contents) = prev_contents else {
                        continue;
                    };
                    let Some(prev_node) = prev_contents.get(folder_path) else {
                        continue;
                    };
                    let was_collapsed = match prev_node {
                        EditorAssetBrowserNode::Asset(editor_asset_browser_asset) => true,
                        EditorAssetBrowserNode::Folder { collapsed, .. } => *collapsed,
                    };
                    if !was_collapsed {
                        *collapsed = false;
                        folder.reload(project_assets_dir)?;
                    }
                }
                _ => {}
            }
        }
        *prev_contents = Some(new_contents);

        return Ok(());
    }

    pub fn sub_path(&self) -> &Path {
        match self {
            EditorAssetBrowserNode::Asset(editor_asset_browser_asset) => {
                &editor_asset_browser_asset.file_sub_path
            }
            EditorAssetBrowserNode::Folder {
                asset_sub_path,
                contents,
                collapsed,
            } => asset_sub_path,
        }
    }

    pub fn is_loaded(&self) -> bool {
        match self {
            EditorAssetBrowserNode::Asset(editor_asset_browser_asset) => panic!(""),
            EditorAssetBrowserNode::Folder {
                asset_sub_path,
                contents,
                collapsed,
            } => contents.is_some(),
        }
    }
}

pub struct EditorAssetBrowserState {
    pub root_asset_folder: EditorAssetBrowserNode,
    pub project_assets_dir: PathBuf,
    pub needs_reload: bool,
}

impl EditorAssetBrowserState {
    pub fn new() -> Self {
        Self {
            root_asset_folder: EditorAssetBrowserNode::Folder {
                asset_sub_path: PathBuf::new(),
                contents: None,
                collapsed: false,
            },
            project_assets_dir: PathBuf::new(),
            needs_reload: true,
        }
    }

    pub fn reload(&mut self, project_assets_dir: &Path) {
        assert!(project_assets_dir.ends_with("assets"));
        self.needs_reload = false;

        self.project_assets_dir = project_assets_dir.to_owned();
        self.root_asset_folder
            .reload(project_assets_dir)
            .expect("Failed to reload assets folder.");
        //let reload_dir = project_assets_dir.join(&self.sub_path);
        //let Ok(iter) = std::fs::read_dir(&reload_dir) else {
        //    log::error!("Failed to read: {}", reload_dir.to_string_lossy());
        //    return;
        //};

        //self.contents.clear();
        //for item in iter {
        //    let Ok(item) = item else {
        //        continue;
        //    };

        //    log::info!(
        //        "is dir {}, for path {:?}",
        //        item.path().is_dir(),
        //        item.path()
        //    );
        //    self.contents.push(EditorAssetBrowserAsset {
        //        file_sub_path: item
        //            .path()
        //            .strip_prefix(&project_assets_dir)
        //            .unwrap()
        //            .to_owned(),
        //        is_dir: item.path().is_dir(),
        //    });
        //}
        //self.contents.sort_by(|a, b| {
        //    if a.is_dir && !b.is_dir {
        //        return std::cmp::Ordering::Less;
        //    }

        //    if !a.is_dir && b.is_dir {
        //        return std::cmp::Ordering::Greater;
        //    }

        //    a.file_sub_path.cmp(&b.file_sub_path)
        //});
    }
}

pub struct EditorAssetBrowserAsset {
    pub file_sub_path: PathBuf,
}

pub fn asset_browser_ui(ui: &mut egui::Ui, session: &mut Session, ui_state: &mut EditorUIState) {
    let assets_dir = session
        .project_assets_dir()
        .expect("UI should be shown only when project is open.");
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(
            egui::RichText::new("Asset Browser").size(20.0),
        ));
        if ui.button("Reload").clicked() {
            ui_state.asset_browser.needs_reload = true;
        }
    });

    if ui_state.asset_browser.needs_reload {
        ui_state.asset_browser.reload(&assets_dir);
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical(|ui| {
                let EditorAssetBrowserNode::Folder { contents, .. } =
                    &mut ui_state.asset_browser.root_asset_folder
                else {
                    panic!("Root node should be a folder.");
                };
                render_asset_children(
                    ui,
                    contents
                        .as_mut()
                        .expect("Root node assets should be resolved"),
                    &assets_dir,
                );
            });
        });
    //ui.label(egui::RichText::new("Performance:").size(8.0));
    //ui.label(format!("FPS: {}", debug_state.fps));
    //ui.label(format!("Frame time: {}ms", debug_state.delta_time_ms));
    //ui.label(format!("Voxel data allocation: {}", total_allocation_str));
}

pub fn paint_node_icon(ui: &mut egui::Ui, collapsed: bool, response: &egui::Response) {
    let visuals = ui.style().interact(response);

    let rect = response.rect;

    // Draw a pointy triangle arrow:
    let rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), rect.height()));
    let rect = rect.expand(visuals.expansion);
    let mut points = vec![rect.left_top(), rect.right_top(), rect.center_bottom()];
    use std::f32::consts::TAU;
    // Rotate 90 degrees so point right when collapsed.
    let rotation = egui::emath::Rot2::from_angle(collapsed.then_some(-TAU / 4.0).unwrap_or(0.0));
    for p in &mut points {
        *p = rect.center() + rotation * (*p - rect.center());
    }

    ui.painter().add(egui::Shape::convex_polygon(
        points,
        visuals.fg_stroke.color,
        egui::Stroke::NONE,
    ));
}

fn render_asset_node(
    ui: &mut egui::Ui,
    node: &mut EditorAssetBrowserNode,
    project_assets_dir: &Path,
) {
    ui.horizontal(|ui| {
        let (id, rect) = ui.allocate_space(egui::vec2(6.0, 6.0));
        let rect_response = ui.interact(rect, id, Sense::click());

        let node_path: &Path = match node {
            EditorAssetBrowserNode::Asset(editor_asset_browser_asset) => {
                &editor_asset_browser_asset.file_sub_path
            }
            EditorAssetBrowserNode::Folder {
                asset_sub_path,
                contents,
                collapsed,
            } => {
                paint_node_icon(ui, *collapsed, &rect_response);
                asset_sub_path
            }
        };
        let node_name = node_path.file_name().unwrap().to_string_lossy().to_string();
        let mut text = egui::RichText::new(node_name);

        let label_id = egui::Id::new(format!(
            "left_panel_{}_asset_label",
            node_path.to_string_lossy()
        ));
        let is_hovering = ui.data(|w| w.get_temp(label_id).unwrap_or(false));
        if is_hovering {
            text = text.background_color(egui::Color32::from_white_alpha(2));
        }

        let mut label = ui.add(egui::Label::new(text).truncate());
        ui.data_mut(|w| w.insert_temp(label_id, label.hovered()));

        if label.clicked() || rect_response.clicked() {
            match node {
                EditorAssetBrowserNode::Asset(editor_asset_browser_asset) => {}
                EditorAssetBrowserNode::Folder {
                    asset_sub_path,
                    contents,
                    collapsed,
                } => {
                    *collapsed = !*collapsed;

                    let first_load = !*collapsed && contents.is_none();
                    // Lazily load assets when folder is first uncollapsed.
                    if first_load {
                        node.reload(project_assets_dir);
                    }
                }
            }
        }
    });

    match node {
        EditorAssetBrowserNode::Folder {
            asset_sub_path,
            contents,
            collapsed,
        } => {
            if !*collapsed {
                assert!(contents.is_some());
                let contents = contents.as_mut().unwrap();
                if !contents.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        render_asset_children(ui, contents, project_assets_dir);
                    });
                }
            }
        }
        _ => {}
    }
}

fn render_asset_children(
    ui: &mut egui::Ui,
    contents: &mut HashMap<PathBuf, EditorAssetBrowserNode>,
    project_assets_dir: &Path,
) {
    let mut indices = contents
        .values()
        .map(|node| node.sub_path().to_owned())
        .collect::<Vec<_>>();
    indices.sort();

    ui.vertical(|ui| {
        for path in indices {
            let node = contents.get_mut(&path).unwrap();
            render_asset_node(ui, node, project_assets_dir);
        }
    });
}
