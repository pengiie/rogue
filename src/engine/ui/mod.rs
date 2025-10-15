use std::{
    borrow::Borrow,
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use egui::{Button, Separator};
use gui::Egui;
use log::debug;
use nalgebra::{Translation3, Vector2, Vector3, Vector4};
use rogue_macros::Resource;

use crate::{
    common::color::Color,
    consts,
    engine::{
        asset::{asset::AssetPath, repr::settings::SettingsAsset},
        editor::ui::{
            asset_browser::EditorAssetBrowserState,
            dialog::new_voxel_model_dialog::EditorNewVoxelModelDialog,
        },
        event::Events,
        physics::{collider_registry::ColliderId, physics_world::PhysicsWorld},
        voxel::{voxel_registry::VoxelModelId, voxel_world_gpu::VoxelWorldGpu},
    },
    session::Session,
    settings::Settings,
};

use super::{
    asset::asset::Assets,
    editor::{self, editor::Editor},
    entity::{
        ecs_world::{ECSWorld, Entity},
        scripting::Scripts,
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
        voxel_world::{self, VoxelWorld},
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
    pub open_model_dialog: EditorOpenModelDialog,
    pub save_model_dialog: EditorSaveModelDialog,
    pub terrain_dialog: EditorTerrainDialog,
    pub add_script_dialog: EditorAddScriptDialog,

    pub asset_browser: EditorAssetBrowserState,
    pub texture_map: HashMap<String, egui::TextureHandle>,
    pub initialized_icons: bool,
    pub message: String,
    pub selecting_new_parent: Option<Entity>,

    pub stats: EditorUIStatistics,

    pub right_pane_state: EditorTab,
    pub selected_collider: Option<ColliderId>,
}

pub struct EditorUIStatistics {
    pub time_length: Duration,
    pub samples: u32,
    pub last_sample: Instant,
    pub cpu_frame_time_samples_max: Duration,
    pub cpu_frame_time_samples: VecDeque<Duration>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EditorTab {
    EntityProperties,
    WorldProperties,
    Editing,
    // Game/project specific settings.
    Game,
    Stats,
    User,
}

pub struct EditorTerrainDialog {
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
}

pub struct EditorOpenModelDialog {
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
    pub associated_entity: Entity,
}

pub struct EditorSaveModelDialog {
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
    pub model_id: VoxelModelId,
}

pub struct EditorAddScriptDialog {
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

impl EditorUIState {
    pub fn new() -> Self {
        let terrain_dialog_channel = std::sync::mpsc::channel();
        let existing_model_dialog_channel = std::sync::mpsc::channel();
        let add_script_dialog_channel = std::sync::mpsc::channel();
        let save_model_script_dialog_channel = std::sync::mpsc::channel();
        Self {
            message: String::new(),
            new_project_dialog: None,
            new_model_dialog: None,
            selecting_new_parent: None,
            terrain_dialog: EditorTerrainDialog {
                tx_file_name: terrain_dialog_channel.0,
                rx_file_name: terrain_dialog_channel.1,
            },
            open_model_dialog: EditorOpenModelDialog {
                tx_file_name: existing_model_dialog_channel.0,
                rx_file_name: existing_model_dialog_channel.1,
                associated_entity: Entity::DANGLING,
            },
            add_script_dialog: EditorAddScriptDialog {
                tx_file_name: add_script_dialog_channel.0,
                rx_file_name: add_script_dialog_channel.1,
                associated_entity: Entity::DANGLING,
            },
            save_model_dialog: EditorSaveModelDialog {
                tx_file_name: save_model_script_dialog_channel.0,
                rx_file_name: save_model_script_dialog_channel.1,
                model_id: VoxelModelId::null(),
            },

            asset_browser: EditorAssetBrowserState::new(),
            texture_map: HashMap::new(),
            initialized_icons: false,
            right_pane_state: EditorTab::EntityProperties,
            stats: EditorUIStatistics {
                time_length: Duration::from_secs(5),
                samples: 1000,
                last_sample: Instant::now(),
                cpu_frame_time_samples_max: Duration::ZERO,
                cpu_frame_time_samples: VecDeque::new(),
            },
            selected_collider: None,
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
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        mut assets: ResMut<Assets>,
        mut physics_world: ResMut<PhysicsWorld>,
        mut settings: ResMut<Settings>,
        mut ecs_world: ResMut<ECSWorld>,
        mut editor: ResMut<Editor>,
        mut session: ResMut<Session>,
        time: Res<Time>,
        mut scripts: ResMut<Scripts>,
        mut events: ResMut<Events>,
    ) {
        let voxel_world: &mut VoxelWorld = &mut voxel_world;
        let ui: &mut UI = &mut ui;
        let debug_state = &mut ui.debug_state;
        let editor_ui_state = &mut ui.editor_state;
        let chunk_generator = &mut ui.chunk_generator;

        let pixels_per_point = egui.pixels_per_point();
        egui.resolve_ui(&mut window, |ctx, window| {
            ui.content_padding = Vector4::zeros();
            if editor.is_active {
                ui.content_padding = editor::ui::egui_editor_ui(
                    ctx,
                    &mut ecs_world,
                    voxel_world,
                    &mut voxel_world_gpu,
                    &mut physics_world,
                    &mut editor,
                    editor_ui_state,
                    &mut session,
                    &mut assets,
                    window,
                    &time,
                    &mut scripts,
                    &mut settings,
                    &mut events,
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
                    });
            }
        });
    }
}
