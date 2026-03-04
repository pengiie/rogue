use nalgebra::Vector4;
use rogue_engine::{
    asset::asset::Assets,
    egui::Egui,
    entity::ecs_world::ECSWorld,
    event::Events,
    graphics::camera::MainCamera,
    material::MaterialBank,
    physics::physics_world::{self, PhysicsWorld},
    resource::{Res, ResMut, Resource},
    voxel::voxel_registry::VoxelModelRegistry,
    window::window::Window,
    world::{region_map::RegionMap, sky::Sky},
};
use rogue_macros::Resource;

use crate::{
    session::EditorSession,
    ui::{
        entity_hierarchy::EntityHierarchyUI,
        entity_properties::EntityPropertiesPane,
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

pub enum EditorCommand {
    OpenUI(/*ui_id*/ String),
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
    NumSides,
}

impl EditorSide {
    const COUNT: usize = Self::NumSides as usize;
}

#[derive(Resource, serde::Serialize, serde::Deserialize)]
#[serde(default = "EditorUI::new")]
pub struct EditorUI {
    /// Top, bottom, left, right
    content_padding: Vector4<u32>,
    side_panes: [Option<EditorUIPaneData>; EditorSide::COUNT],
}

impl EditorUI {
    pub fn new() -> Self {
        Self {
            content_padding: Vector4::zeros(),
            side_panes: Self::default_panes(),
        }
    }

    pub fn default_panes() -> [Option<EditorUIPaneData>; EditorSide::COUNT] {
        let mut sides = [const { None }; EditorSide::COUNT];
        sides[EditorSide::Left as usize] =
            Some(EditorUIContentPane::new(EntityHierarchyUI).into_pane());
        sides[EditorSide::Right as usize] =
            Some(EditorUIContentPane::new(EntityPropertiesPane).into_pane());
        sides
    }

    pub fn content_padding(&self) -> &Vector4<u32> {
        &self.content_padding
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
    ) {
        let mut commands = EditorCommands::new();
        egui.resolve_ui(&mut window, |ctx, window| {
            let frame = egui::Frame::new().fill(ctx.style().visuals.window_fill);

            let mut res_ctx = EditorUIContext {
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
            };
            let mut padding = Vector4::zeros();
            padding.x =
                egui::TopBottomPanel::new(egui::panel::TopBottomSide::Top, "editor_top_panel")
                    .frame(frame.clone().inner_margin(8.0))
                    .show(ctx, |ui| {
                        TopBarPane::show(ui, &mut res_ctx);
                    })
                    .response
                    .rect
                    .height();
            padding.y = 0.0;
            //padding.y = egui::TopBottomPanel::new(
            //    egui::panel::TopBottomSide::Bottom,
            //    "editor_bottom_panel",
            //)
            //.frame(frame.clone())
            //.resizable(true)
            //.show(ctx, |ui| {
            //    editor_ui.bottom_pane_ui(ui);
            //})
            //.response
            //.rect
            //.height();
            let max_ui_half_width = (window.width() as f32 / ctx.pixels_per_point()) * 0.5 - 50.0;
            padding.z = egui::SidePanel::new(egui::panel::Side::Left, "editor_left_panel")
                .resizable(true)
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
            padding.w = egui::SidePanel::new(egui::panel::Side::Right, "editor_right_panel")
                .resizable(true)
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

            editor_ui.content_padding = (padding * ctx.pixels_per_point()).map(|x| x as u32);
        });

        for command in commands.commands {
            match command {
                EditorCommand::OpenUI(ui_id) => {
                    editor_ui.open_pane(&ui_id);
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
                    self.spawn_pane(EntityPropertiesPane, EditorSide::Right)
                }
                MaterialsPane::ID => self.spawn_pane(MaterialsPane::new(), EditorSide::Right),
                WorldPane::ID => self.spawn_pane(WorldPane::new(), EditorSide::Right),
                _ => {
                    log::warn!("Tried to open pane with id {pane_id} but no implementation exists to spawn that pane.");
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
