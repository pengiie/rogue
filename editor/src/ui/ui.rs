use std::{
    path::PathBuf,
    sync::mpsc::{channel, Receiver, Sender},
};

use nalgebra::{Vector2, Vector4};
use rogue_engine::{
    animation::animation_bank::AnimationBank,
    asset::asset::Assets,
    debug::debug_renderer::DebugRenderer,
    egui::Egui,
    entity::ecs_world::ECSWorld,
    event::Events,
    graphics::camera::MainCamera,
    material::MaterialBank,
    physics::physics_world::{self, PhysicsWorld},
    resource::{Res, ResMut, Resource},
    voxel::voxel_registry::VoxelModelRegistry,
    window::window::Window,
    world::{renderable::rt_pass::WorldRTPass, sky::Sky},
};
use rogue_macros::Resource;
use serde_with::serde_as;
use rogue_engine::world::terrain::region_map::RegionMap;
use crate::{
    animation_preview::EditorAnimationPreviewer,
    editing::voxel_editing::EditorVoxelEditing,
    game_session::EditorGameSession,
    session::EditorSession,
    ui::{
        animation_pane::AnimationPane,
        asset_pane::AssetsPane,
        asset_properties_pane::AssetPropertiesPane,
        editing_pane::EditingPane,
        entity_hierarchy::EntityHierarchyUI,
        entity_properties::EntityPropertiesPane,
        global_state::GlobalStateEditorUI,
        materials_pane::MaterialsPane,
        pane::{
            EditorUIContentPane, EditorUIPane, EditorUIPaneData, EditorUIPaneMethods,
            EditorUITabPane,
        },
        top_bar::TopBarPane,
        world_pane::WorldPane,
    },
    world::generator::WorldGenerator,
};

/// Context that we pass down to every component so we don't have 10 argument functions.
pub struct EditorUIContext<'a> {
    pub session: &'a mut EditorSession,
    pub game_session: &'a mut EditorGameSession,
    pub ecs_world: &'a mut ECSWorld,
    pub voxel_registry: &'a mut VoxelModelRegistry,
    pub physics_world: &'a mut PhysicsWorld,
    pub material_bank: &'a mut MaterialBank,
    pub main_camera: &'a mut MainCamera,
    pub region_map: &'a mut RegionMap,
    pub events: &'a mut Events,
    pub world_generator: &'a mut WorldGenerator,
    pub assets: &'a mut Assets,
    pub commands: &'a mut EditorCommands,
    pub sky: &'a mut Sky,
    pub ui_state: &'a mut GlobalStateEditorUI,
    pub voxel_editing: &'a mut EditorVoxelEditing,
    pub debug_renderer: &'a mut DebugRenderer,
    pub world_rt_pass: &'a mut WorldRTPass,
    pub animation_preview: &'a mut EditorAnimationPreviewer,
    pub animation_bank: &'a mut AnimationBank,
}

pub struct EditorCommands {
    commands: Vec<EditorCommand>,
}

impl EditorCommands {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, command: EditorCommand) {
        self.commands.push(command);
    }
}

pub type DialogShowFn = Box<dyn FnMut(&mut egui::Ui, &mut EditorUIContext<'_>) -> bool>;
pub type FilePickerFn = Box<dyn FnOnce(EditorUIContext<'_>, PathBuf)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilePickerType {
    OpenFile,
    CreateFile,
}

pub enum EditorCommand {
    OpenUI(/*ui_id*/ String),
    FilePicker {
        picker_type: FilePickerType,
        callback: FilePickerFn,
        extensions: Vec<String>,
        preset_file_path: Option<PathBuf>,
    },
    OpenDialog(EditorDialog),
    CloseDialog(/*id*/ String),
}

pub struct EditorDialog {
    pub id: String,
    pub title: String,
    pub show_fn: DialogShowFn,
}

pub struct EditorDialogContext {
    pub should_close: bool,
}

impl EditorCommand {
    pub fn open_ui(ui_id: &str) -> Self {
        Self::OpenUI(ui_id.to_string())
    }
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorSide {
    Left = 0,
    Right = 1,
    Bottom = 2,
    NumSides,
}

impl EditorSide {
    const COUNT: usize = Self::NumSides as usize;
}

struct EditorFilePicker {
    file_picker_send: Sender<Option<PathBuf>>,
    file_picker_recv: Receiver<Option<PathBuf>>,
    file_picker_callback:
        Option<Box<dyn FnOnce(EditorUIContext<'_>, /*relative asset path to assets dir*/ PathBuf)>>,
    file_picker_type: Option<FilePickerType>,
}

impl EditorFilePicker {
    pub fn new() -> Self {
        let (file_picker_send, file_picker_recv) = channel();
        Self {
            file_picker_send,
            file_picker_recv,
            file_picker_callback: None,
            file_picker_type: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.file_picker_callback.is_some()
    }

    pub fn update(&mut self, res_ctx: EditorUIContext<'_>) {
        match self.file_picker_recv.try_recv() {
            Ok(file) => {
                if let Some(callback) = self.file_picker_callback.take()
                    && let Some(file) = file
                {
                    if !file.exists() && self.file_picker_type == Some(FilePickerType::OpenFile) {
                        log::error!(
                            "File picker tried to open path {:?} which does not exist.",
                            file
                        );
                        return;
                    }

                    let Ok(asset_path) =
                        file.strip_prefix(res_ctx.assets.project_assets_dir().unwrap())
                    else {
                        log::error!(
                            "File picker returned path {:?} which is not in the project assets directory.",
                            file
                        );
                        return;
                    };
                    callback(res_ctx, asset_path.to_path_buf());
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(err) => {
                log::error!("Error receiving file picker result from thread: {:?}", err);
            }
        }
    }

    pub fn open(
        &mut self,
        picker_type: FilePickerType,
        callback: FilePickerFn,
        extensions: Vec<String>,
        preset_file_path: Option<PathBuf>,
        assets: &Assets,
    ) {
        assert!(
            !self.is_open(),
            "Tried to open file picker but it's already open."
        );
        self.file_picker_type = Some(picker_type);
        self.file_picker_callback = Some(Box::new(callback));

        let assets_dir = assets.project_assets_dir();
        let sender = self.file_picker_send.clone();
        std::thread::spawn(move || {
            pollster::block_on(async move {
                let mut file_picker = rfd::FileDialog::new()
                    .add_filter(
                        "Supported files",
                        &extensions.iter().map(|e| e.as_str()).collect::<Vec<_>>(),
                    )
                    .add_filter("All files", &["*"])
                    .set_can_create_directories(true)
                    .set_directory(assets_dir.unwrap_or_else(|| PathBuf::from("./")));
                if let Some(preset_file_path) = preset_file_path {
                    if preset_file_path.is_dir() {
                        file_picker = file_picker.set_directory(preset_file_path);
                    } else {
                        file_picker = file_picker
                            .set_directory(preset_file_path.parent().unwrap())
                            .set_file_name(preset_file_path.file_name().unwrap().to_string_lossy());
                    }
                }
                let file = match picker_type {
                    FilePickerType::OpenFile => file_picker.pick_file(),
                    FilePickerType::CreateFile => file_picker.save_file(),
                };
                let _ = sender.send(file);
            });
        });
    }
}

#[serde_as]
#[derive(Resource, serde::Serialize, serde::Deserialize)]
#[serde(default = "EditorUI::new")]
pub struct EditorUI {
    /// Top, bottom, left, right
    content_padding: Vector4<u32>,

    #[serde_as(deserialize_as = "serde_with::DefaultOnError")]
    side_panes: [Option<EditorUIPaneData>; EditorSide::COUNT],
    global_state: GlobalStateEditorUI,

    #[serde(skip)]
    file_picker: EditorFilePicker,
    #[serde(skip)]
    open_dialogs: Vec<EditorDialog>,
}

impl EditorUI {
    pub fn new() -> Self {
        Self {
            content_padding: Vector4::zeros(),
            side_panes: Self::default_panes(),
            global_state: GlobalStateEditorUI::new(),
            file_picker: EditorFilePicker::new(),
            open_dialogs: Vec::new(),
        }
    }

    pub fn default_panes() -> [Option<EditorUIPaneData>; EditorSide::COUNT] {
        let mut sides = [const { None }; EditorSide::COUNT];
        sides[EditorSide::Left as usize] =
            Some(EditorUIContentPane::new(EntityHierarchyUI).into_pane());
        sides[EditorSide::Right as usize] =
            Some(EditorUIContentPane::new(EntityPropertiesPane::new()).into_pane());
        sides
    }

    pub fn content_padding(&self) -> &Vector4<u32> {
        &self.content_padding
    }

    pub fn backbuffer_size(&self, window: &Window) -> Vector2<u32> {
        let width = window.width() - self.content_padding.z - self.content_padding.w;
        let height = window.height() - self.content_padding.x - self.content_padding.y;
        Vector2::new(width, height)
    }

    pub fn backbuffer_offset(&self) -> Vector2<u32> {
        Vector2::new(self.content_padding.z, self.content_padding.x)
    }

    pub fn resolve_egui_ui(
        mut editor_ui: ResMut<EditorUI>,
        mut egui: ResMut<Egui>,
        mut window: ResMut<Window>,
        mut ecs_world: ResMut<ECSWorld>,
        mut session: ResMut<EditorSession>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut physics_world: ResMut<PhysicsWorld>,
        mut events: ResMut<Events>,
        mut assets: ResMut<Assets>,
        mut material_bank: ResMut<MaterialBank>,
        mut main_camera: ResMut<MainCamera>,
        mut region_map: ResMut<RegionMap>,
        mut world_generator: ResMut<WorldGenerator>,
        mut sky: ResMut<Sky>,
        mut voxel_editing: ResMut<EditorVoxelEditing>,
        mut debug_renderer: ResMut<DebugRenderer>,
        (mut game_session, mut world_rt_pass, mut animation_preview, mut animation_bank): (
            ResMut<EditorGameSession>,
            ResMut<WorldRTPass>,
            ResMut<EditorAnimationPreviewer>,
            ResMut<AnimationBank>,
        ),
    ) {
        let editor_ui = &mut *editor_ui;
        let mut commands = EditorCommands::new();
        egui.resolve_ui(&mut window, |ctx, window| {
            let frame = egui::Frame::new().fill(ctx.style().visuals.window_fill);
            let max_ui_half_width = (window.width() as f32 / ctx.pixels_per_point()) * 0.5 - 50.0;

            let mut res_ctx = EditorUIContext {
                game_session: &mut game_session,
                ecs_world: &mut ecs_world,
                session: &mut session,
                voxel_registry: &mut voxel_registry,
                physics_world: &mut physics_world,
                material_bank: &mut material_bank,
                events: &mut events,
                assets: &mut assets,
                commands: &mut commands,
                main_camera: &mut main_camera,
                region_map: &mut region_map,
                world_generator: &mut world_generator,
                sky: &mut sky,
                ui_state: &mut editor_ui.global_state,
                voxel_editing: &mut voxel_editing,
                debug_renderer: &mut debug_renderer,
                world_rt_pass: &mut world_rt_pass,
                animation_preview: &mut animation_preview,
                animation_bank: &mut animation_bank,
            };
            let default_padding = editor_ui
                .content_padding
                .map(|x| (x as f32 / ctx.pixels_per_point()) as f32);
            let mut padding = Vector4::zeros();
            padding.x =
                egui::TopBottomPanel::new(egui::panel::TopBottomSide::Top, "editor_top_panel")
                    .frame(frame.clone().inner_margin(8.0))
                    .default_height(default_padding.x)
                    .show(ctx, |ui| {
                        TopBarPane::show(ui, &mut res_ctx);
                    })
                    .response
                    .rect
                    .height();
            padding.w = egui::SidePanel::new(egui::panel::Side::Right, "editor_right_panel")
                .resizable(true)
                .default_width(default_padding.w)
                .max_width(max_ui_half_width)
                .frame(frame.clone())
                .show(ctx, |ui| {
                    if let Some(pane) = &mut editor_ui.side_panes[EditorSide::Right as usize] {
                        pane.show(ui, &mut res_ctx);
                    }
                })
                .response
                .rect
                .width();
            // Add bottom before left panel so it extends into the left.
            padding.y = egui::TopBottomPanel::new(
                egui::panel::TopBottomSide::Bottom,
                "editor_bottom_panel",
            )
            .frame(frame.clone())
            .default_height(default_padding.y)
            .resizable(true)
            .show(ctx, |ui| {
                if let Some(pane) = &mut editor_ui.side_panes[EditorSide::Bottom as usize] {
                    pane.show(ui, &mut res_ctx);
                }
            })
            .response
            .rect
            .height();
            padding.z = egui::SidePanel::new(egui::panel::Side::Left, "editor_left_panel")
                .resizable(true)
                .default_width(default_padding.z)
                .max_width(max_ui_half_width)
                .frame(frame.clone())
                .show(ctx, |ui| {
                    if let Some(pane) = &mut editor_ui.side_panes[EditorSide::Left as usize] {
                        pane.show(ui, &mut res_ctx);
                    }
                })
                .response
                .rect
                .width();

            editor_ui.content_padding = (padding * ctx.pixels_per_point()).map(|x| x as u32);

            // Render any open dialogs
            let mut to_close_indices = Vec::new();
            for (i, EditorDialog { title, show_fn, id }) in
                editor_ui.open_dialogs.iter_mut().enumerate()
            {
                let mut should_close = false;
                let mut is_open = true;
                egui::Window::new(title.clone())
                    .collapsible(false)
                    .resizable(true)
                    .open(&mut is_open)
                    .show(ctx, |ui| {
                        should_close = show_fn(ui, &mut res_ctx);
                    });
                if should_close || !is_open {
                    to_close_indices.push(i);
                }
            }

            for i in to_close_indices.into_iter().rev() {
                editor_ui.open_dialogs.swap_remove(i);
            }
        });

        let mut res_ctx = EditorUIContext {
            ecs_world: &mut ecs_world,
            session: &mut session,
            game_session: &mut game_session,
            voxel_registry: &mut voxel_registry,
            physics_world: &mut physics_world,
            material_bank: &mut material_bank,
            events: &mut events,
            assets: &mut assets,
            commands: &mut commands,
            main_camera: &mut main_camera,
            region_map: &mut region_map,
            world_generator: &mut world_generator,
            sky: &mut sky,
            ui_state: &mut editor_ui.global_state,
            voxel_editing: &mut voxel_editing,
            debug_renderer: &mut debug_renderer,
            world_rt_pass: &mut world_rt_pass,
            animation_preview: &mut animation_preview,
            animation_bank: &mut animation_bank,
        };
        editor_ui.file_picker.update(res_ctx);

        // Process all commands at the end since they are flushed every frame.
        for command in commands.commands {
            match command {
                EditorCommand::OpenUI(ui_id) => {
                    editor_ui.open_pane(&ui_id);
                }
                EditorCommand::FilePicker {
                    picker_type,
                    callback,
                    extensions,
                    preset_file_path,
                } => {
                    if !editor_ui.file_picker.is_open() {
                        editor_ui.file_picker.open(
                            picker_type,
                            callback,
                            extensions,
                            preset_file_path,
                            &assets,
                        );
                    }
                }
                EditorCommand::OpenDialog(dialog) => {
                    editor_ui.open_dialogs.push(dialog);
                }
                EditorCommand::CloseDialog(id) => {
                    log::info!("Got close dialog thingy.");
                    if let Some(index) = editor_ui
                        .open_dialogs
                        .iter()
                        .position(|dialog| dialog.id == id)
                    {
                        log::info!("Closing dialog with id {id} and index {index}");
                        editor_ui.open_dialogs.swap_remove(index);
                    }
                }
            }
        }
    }

    pub fn open_pane(&mut self, pane_id: &str) {
        let mut opened_somewhere = false;
        for side_pane in &mut self.side_panes {
            if let Some(pane) = side_pane {
                opened_somewhere |= pane.open_pane(pane_id);
            }
        }

        if !opened_somewhere {
            match pane_id {
                EntityHierarchyUI::ID => self.spawn_pane(EntityHierarchyUI, EditorSide::Left),
                EntityPropertiesPane::ID => {
                    self.spawn_pane(EntityPropertiesPane::new(), EditorSide::Right)
                }
                MaterialsPane::ID => self.spawn_pane(MaterialsPane::new(), EditorSide::Right),
                WorldPane::ID => self.spawn_pane(WorldPane::new(), EditorSide::Right),
                AssetsPane::ID => self.spawn_pane(AssetsPane::new(), EditorSide::Left),
                AssetPropertiesPane::ID => {
                    self.spawn_pane(AssetPropertiesPane::new(), EditorSide::Right)
                }
                EditingPane::ID => self.spawn_pane(EditingPane::new(), EditorSide::Right),
                AnimationPane::ID => self.spawn_pane(AnimationPane::new(), EditorSide::Bottom),
                _ => {
                    log::warn!(
                        "Tried to open pane with id {pane_id} but no implementation exists to spawn that pane."
                    );
                }
            }
        }
    }

    pub fn spawn_pane(&mut self, pane: impl EditorUIPane, side: EditorSide) {
        if let Some(side_pane) = &mut self.side_panes[side as usize] {
            let spawned = side_pane.spawn_pane(pane.into_content_pane());
            if let Some(spawned) = spawned {
                let side_pane = self.side_panes[side as usize].take().unwrap();
                let new_pane = EditorUITabPane {
                    tabs: vec![side_pane, spawned],
                    selected_tab: 1,
                };
                self.side_panes[side as usize] = Some(new_pane.into_pane());
            }
            return;
        }
        self.side_panes[side as usize] = Some(EditorUIContentPane::new(pane).into_pane());
    }
}
