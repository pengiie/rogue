use crate::engine::editor::ui::asset_browser::EditorAssetBrowserState;
use crate::engine::editor::ui::dialog::new_voxel_model_dialog::EditorNewVoxelModelDialog;
use crate::engine::entity::ecs_world::Entity;
use crate::engine::physics::collider_registry::ColliderId;
use crate::engine::voxel::voxel_registry::VoxelModelId;
use crate::engine::window::time::Instant;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

pub struct EditorUIState {
    pub new_project_dialog: Option<EditorNewProjectDialog>,
    pub new_model_dialog: Option<EditorNewVoxelModelDialog>,
    pub open_model_dialog: EditorOpenModelDialog,
    pub save_model_dialog: EditorSaveModelDialog,
    pub terrain_dialog: EditorTerrainDialog,
    pub add_script_dialog: EditorAddScriptDialog,

    pub asset_browser: EditorAssetBrowserState,
    // Constant textures used for editor stuff
    pub texture_map: HashMap<String, egui::TextureHandle>,
    pub material_map: HashMap</*material name*/ String, egui::TextureHandle>,
    pub initialized_icons: bool,
    pub message: String,
    pub selecting_new_parent: Option<Entity>,

    pub stats: EditorUIStatistics,

    pub right_pane_state: EditorTab,
    pub selected_collider: Option<ColliderId>,
    /// The materials which have their UI opened up in the materials section.
    pub open_materials: HashSet<MaterialId>,
    pub material_texture_dialog: EditorPickMaterialTextureDialog,
}

pub struct EditorUIStatistics {
    pub time_length: Duration,
    pub samples: u32,
    pub last_sample: Instant,
    pub cpu_frame_time_samples_max: Duration,
    pub cpu_frame_time_samples: VecDeque<Duration>,
    pub physics_world_energy: VecDeque</*KE*/ f32>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum EditorTab {
    /// Properties and components for the currently selected entity.
    EntityProperties,
    /// Properties of the underlying voxel world, may be better
    /// called terrain properties.
    WorldProperties,
    /// Voxel editing brushes and colors.
    Editing,
    /// Game/project specific settings.
    Game,
    /// Statistics for CPU/GPU and current game state.
    Stats,
    /// Settings which would be visible to the end user of the game.
    User,
}

pub struct EditorTerrainDialog {
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
}

pub struct EditorPickMaterialTextureDialog {
    pub tx_file_name: Sender<String>,
    pub rx_file_name: Receiver<String>,
    pub associated_material_id: MaterialId,
    pub associated_texture_type: MaterialTextureType,
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
        let new_project_dialog_channel = std::sync::mpsc::channel();
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
                physics_world_energy: VecDeque::new(),
            },
            selected_collider: None,
            material_map: HashMap::new(),
            open_materials: HashSet::new(),
            material_texture_dialog: EditorPickMaterialTextureDialog {
                tx_file_name: new_project_dialog_channel.0,
                rx_file_name: new_project_dialog_channel.1,
                associated_material_id: MaterialId::null(),
                associated_texture_type: MaterialTextureType::Color,
            },
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
