use std::borrow::Borrow;

use egui::Separator;
use gui::Egui;
use log::debug;
use rogue_macros::Resource;

use crate::{
    common::color::Color,
    consts,
    engine::asset::{asset::AssetPath, repr::settings::SettingsAsset},
    settings::Settings,
};

use super::{
    asset::asset::Assets,
    graphics::renderer::Renderer,
    resource::{Res, ResMut},
    voxel::voxel_world::{VoxelWorld, VoxelWorldGpu},
    window::{
        time::{Instant, Time},
        window::Window,
    },
    world::game_world::GameWorld,
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
}

pub struct DebugUIState {
    pub zoom_factor: f32,
    pub player_fov: f32,
    pub fps: u32,
    pub delta_time_ms: f32,
    pub samples: u32,
    pub polling_time_ms: u32,
    pub draw_grid: bool,

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
        }
    }

    pub fn update(
        mut egui: ResMut<Egui>,
        mut ui: ResMut<UI>,
        time: Res<Time>,
        renderer: Res<Renderer>,
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
        window: Res<Window>,
        mut egui: ResMut<Egui>,
        mut ui: ResMut<UI>,
        mut game_world: ResMut<GameWorld>,
        voxel_world: Res<VoxelWorldGpu>,
        mut assets: ResMut<Assets>,
        settings: Res<Settings>,
    ) {
        let debug_state = &mut ui.debug_state;
        egui.resolve_ui(&window, |ctx| {
            let mut total_allocation_str;
            let al = voxel_world
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

            egui::Window::new("Debug")
                .current_pos(egui::pos2(4.0, 4.0))
                .movable(false)
                .show(ctx, |ui| {
                    ui.set_width(150.0);

                    ui.label(egui::RichText::new("Performance:").size(16.0));
                    ui.label(format!("FPS: {}", debug_state.fps));
                    ui.label(format!("Frame time: {}ms", debug_state.delta_time_ms));
                    ui.label(format!("Voxel data allocation: {}", total_allocation_str));

                    if ui
                        .add(egui::Button::new("Save Settings").rounding(4.0))
                        .clicked()
                    {
                        log::info!("Saving current settings.");
                        assets.save_asset(
                            AssetPath::new_user_dir(consts::io::SETTINGS_FILE),
                            SettingsAsset::from(&settings as &Settings),
                        );
                    }

                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("World:").size(16.0));
                        if game_world.is_io_busy() {
                            ui.disable();
                        }

                        if ui.add(egui::Button::new("Save").rounding(4.0)).clicked() {
                            game_world.save();
                        }

                        if ui.add(egui::Button::new("Load").rounding(4.0)).clicked() {
                            game_world.load();
                        }
                    });
                });
        });
    }
}
