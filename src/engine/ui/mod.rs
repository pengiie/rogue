use std::{
    borrow::Borrow,
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::{Receiver, Sender},
};

use egui::{Button, Separator};
use gui::Egui;
use log::debug;
use nalgebra::{Translation3, Vector2, Vector3, Vector4};
use rogue_macros::Resource;

use crate::{
    common::color::Color,
    consts,
    engine::asset::{asset::AssetPath, repr::settings::SettingsAsset},
    game::entity::GameEntity,
    session::Session,
    settings::Settings,
};

use super::{
    asset::asset::Assets,
    editor::{self, editor::Editor},
    entity::{
        ecs_world::{ECSWorld, Entity},
        RenderableVoxelEntity,
    },
    graphics::renderer::Renderer,
    input::Input,
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::{
        attachment::{Attachment, PTMaterial},
        chunk_generator::{self, ChunkGenerator},
        flat::VoxelModelFlat,
        voxel::{VoxelModel, VoxelModelType},
        voxel_world::{self, VoxelWorld, VoxelWorldGpu},
    },
    window::{
        time::{Instant, Time, Timer},
        window::Window,
    },
};

pub mod gui;

pub fn initialize_debug_ui_resource(app: &mut crate::app::App) {
    let egui = Egui::new(&app.get_resource::<Window>());
    app.insert_resource(egui);
    app.insert_resource(UI::new());
}

#[derive(Resource)]
pub struct UI {
    pub debug_state: DebugUIState,
    pub editor_state: EditorUIState,
    /// Represented as [top, bottom, left, right].
    pub content_padding: Vector4<f32>,

    pub chunk_generator: ChunkGenerator,
}

pub struct EditorUIState {
    pub new_project_dialog: Option<EditorNewProjectDialog>,
    pub new_model_dialog: Option<EditorNewVoxelModelDialog>,
    pub existing_model_dialog: EditorExistingModelDialog,
    pub terrain_dialog: EditorTerrainDialog,

    pub asset_browser: EditorAssetBrowserState,
    pub texture_map: HashMap<String, egui::TextureHandle>,
    pub initialized_icons: bool,
    pub message: String,

    pub right_pane_state: EditorTab,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EditorTab {
    EntityProperties,
    WorldProperties,
    Editing,
    Game,
}

pub struct EditorTerrainDialog {
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
}

pub struct EditorExistingModelDialog {
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
    pub associated_entity: Entity,
}

pub struct EditorNewProjectDialog {
    pub open: bool,
    pub file_name: String,
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
    // Tracked so we don't read the dir every frame, only on updates.
    pub last_file_name: (String, /*valid_dir=*/ bool, /*error=*/ String),
}

pub struct EditorNewVoxelModelDialog {
    pub open: bool,
    pub associated_entity: Entity,
    pub file_path: String,
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
    // Tracked so we don't read the dir every frame, only on updates.
    pub last_file_path: (String, /*valid_path=*/ bool, /*error=*/ String),
    pub dimensions: Vector3<u32>,
    pub model_type: VoxelModelType,
}

pub struct EditorAssetBrowserState {
    pub sub_path: PathBuf,
    pub needs_reload: bool,
    pub contents: Vec<EditorAssetBrowserAsset>,
}

impl EditorAssetBrowserState {
    pub fn reload(&mut self, project_assets_dir: &Path) {
        let reload_dir = project_assets_dir.join(&self.sub_path);
        let Ok(iter) = std::fs::read_dir(&reload_dir) else {
            log::error!("Failed to read: {}", reload_dir.to_string_lossy());
            return;
        };

        self.contents.clear();
        for item in iter {
            let Ok(item) = item else {
                continue;
            };

            log::info!(
                "is dir {}, for path {:?}",
                item.path().is_dir(),
                item.path()
            );
            self.contents.push(EditorAssetBrowserAsset {
                file_sub_path: item
                    .path()
                    .strip_prefix(&project_assets_dir)
                    .unwrap()
                    .to_owned(),
                is_dir: item.path().is_dir(),
            });
        }
        self.contents.sort_by(|a, b| {
            if a.is_dir && !b.is_dir {
                return std::cmp::Ordering::Less;
            }

            if !a.is_dir && b.is_dir {
                return std::cmp::Ordering::Greater;
            }

            a.file_sub_path.cmp(&b.file_sub_path)
        });
    }
}

pub struct EditorAssetBrowserAsset {
    pub file_sub_path: PathBuf,
    pub is_dir: bool,
}

impl EditorUIState {
    pub fn new() -> Self {
        let terrain_dialog_channel = std::sync::mpsc::channel();
        let existing_model_dialog_channel = std::sync::mpsc::channel();
        Self {
            message: String::new(),
            new_project_dialog: None,
            new_model_dialog: None,
            terrain_dialog: EditorTerrainDialog {
                tx_file_name: terrain_dialog_channel.0,
                rx_file_name: terrain_dialog_channel.1,
            },
            existing_model_dialog: EditorExistingModelDialog {
                tx_file_name: existing_model_dialog_channel.0,
                rx_file_name: existing_model_dialog_channel.1,
                associated_entity: Entity::DANGLING,
            },

            asset_browser: EditorAssetBrowserState {
                sub_path: PathBuf::new(),
                needs_reload: true,
                contents: Vec::new(),
            },
            texture_map: HashMap::new(),
            initialized_icons: false,
            right_pane_state: EditorTab::EntityProperties,
        }
    }

    pub fn get_image(&self, name: &str, size: egui::Vec2) -> egui::ImageSource {
        let handle = self.texture_map.get(name).unwrap();
        egui::ImageSource::Texture(egui::load::SizedTexture {
            id: handle.id(),
            size,
        })
    }
}

pub struct DebugUIState {
    pub zoom_factor: f32,
    pub player_fov: f32,
    pub fps: u32,
    pub delta_time_ms: f32,
    pub samples: u32,
    pub polling_time_ms: u32,
    pub draw_grid: bool,

    pub generate_radius: u32,

    pub brush_size: u32,
    pub brush_color: Color,

    pub last_ui_update: Instant,
}

impl Default for DebugUIState {
    fn default() -> Self {
        Self {
            zoom_factor: 1.0,
            player_fov: 90.0,
            fps: 0,
            samples: 0,
            delta_time_ms: 0.0,
            polling_time_ms: 250,
            draw_grid: true,

            generate_radius: 0,

            brush_size: 1,
            brush_color: Color::new_srgb(1.0, 0.2, 1.0),

            last_ui_update: Instant::now(),
        }
    }
}

impl UI {
    pub fn new() -> Self {
        UI {
            debug_state: DebugUIState::default(),
            editor_state: EditorUIState::new(),
            content_padding: Vector4::zeros(),
            chunk_generator: ChunkGenerator::new(0),
        }
    }

    pub fn content_offset(&self) -> Vector2<f32> {
        self.content_padding.zx()
    }

    pub fn content_size(&self, window_size: Vector2<f32>) -> Vector2<f32> {
        return window_size
            - Vector2::new(
                self.content_padding.z + self.content_padding.w,
                self.content_padding.x + self.content_padding.y,
            );
    }

    pub fn update(
        mut egui: ResMut<Egui>,
        mut ui: ResMut<UI>,
        time: Res<Time>,
        renderer: Res<Renderer>,
        input: Res<Input>,
    ) {
        // Determine if we should poll for the current fps, ensures the fps doesn't change
        // rapidly where it is unreadable.
        if ui.debug_state.last_ui_update.elapsed().as_millis()
            >= ui.debug_state.polling_time_ms.into()
        {
            ui.debug_state.last_ui_update = Instant::now();

            ui.debug_state.fps = time.fps();
            ui.debug_state.delta_time_ms = time.delta_time().as_micros() as f32 / 1000.0;
        }
    }

    pub fn draw(
        mut window: ResMut<Window>,
        mut egui: ResMut<Egui>,
        mut ui: ResMut<UI>,
        mut voxel_world: ResMut<VoxelWorld>,
        voxel_world_gpu: Res<VoxelWorldGpu>,
        mut assets: ResMut<Assets>,
        settings: Res<Settings>,
        mut ecs_world: ResMut<ECSWorld>,
        mut editor: ResMut<Editor>,
        mut session: ResMut<Session>,
    ) {
        let voxel_world: &mut VoxelWorld = &mut voxel_world;
        let ui: &mut UI = &mut ui;
        let debug_state = &mut ui.debug_state;
        let editor_ui_state = &mut ui.editor_state;
        let chunk_generator = &mut ui.chunk_generator;

        let pixels_per_point = egui.pixels_per_point();
        egui.resolve_ui(&mut window, |ctx, window| {
            let mut total_allocation_str;
            let al = voxel_world_gpu
                .voxel_allocator()
                .map_or(0, |alloc| alloc.total_allocated_size());
            if al >= 2u64.pow(30) {
                total_allocation_str = format!("{:.3}GiB", al as f32 / 2f32.powf(30.0));
            } else if al >= 2u64.pow(20) {
                total_allocation_str = format!("{:.3}MiB", al as f32 / 2f32.powf(20.0));
            } else if al >= 2u64.pow(10) {
                total_allocation_str = format!("{:.3}KiB", al as f32 / 2f32.powf(10.0));
            } else {
                total_allocation_str = format!("{:.3}B", al);
            }

            ui.content_padding = Vector4::zeros();
            if editor.is_active {
                ui.content_padding = editor::ui::egui_editor_ui(
                    ctx,
                    &mut ecs_world,
                    voxel_world,
                    &mut editor,
                    editor_ui_state,
                    &mut session,
                    &mut assets,
                    window,
                );
            } else {
                egui::Window::new("Debug")
                    .current_pos(egui::pos2(4.0, 4.0))
                    .movable(false)
                    .show(ctx, |ui| {
                        ui.set_width(150.0);

                        ui.label(egui::RichText::new("Performance:").size(16.0));
                        ui.label(format!("FPS: {}", debug_state.fps));
                        ui.label(format!("Frame time: {}ms", debug_state.delta_time_ms));
                        ui.label(format!("Voxel data allocation: {}", total_allocation_str));
                    });
            }
        });
    }
}
