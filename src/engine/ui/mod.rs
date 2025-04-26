use std::borrow::Borrow;

use egui::Separator;
use gui::Egui;
use log::debug;
use nalgebra::{Translation3, Vector3};
use rogue_macros::Resource;

use crate::{
    common::color::Color,
    consts,
    engine::asset::{asset::AssetPath, repr::settings::SettingsAsset},
    game::entity::GameEntity,
    settings::Settings,
};

use super::{
    asset::asset::Assets,
    entity::{ecs_world::ECSWorld, RenderableVoxelEntity},
    graphics::renderer::Renderer,
    input::Input,
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::{
        attachment::{Attachment, PTMaterial},
        chunk_generator::{self, ChunkGenerator},
        flat::VoxelModelFlat,
        voxel::VoxelModel,
        voxel_world::{self, VoxelWorld, VoxelWorldGpu},
    },
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

    pub chunk_generator: ChunkGenerator,
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
            chunk_generator: ChunkGenerator::new(0),
        }
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
        window: Res<Window>,
        mut egui: ResMut<Egui>,
        mut ui: ResMut<UI>,
        mut game_world: ResMut<GameWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
        voxel_world_pu: Res<VoxelWorldGpu>,
        mut assets: ResMut<Assets>,
        settings: Res<Settings>,
        mut ecs_world: ResMut<ECSWorld>,
    ) {
        let voxel_world: &mut VoxelWorld = &mut voxel_world;
        let ui: &mut UI = &mut ui;
        let debug_state = &mut ui.debug_state;
        let chunk_generator = &mut ui.chunk_generator;
        egui.resolve_ui(&window, |ctx| {
            let mut total_allocation_str;
            let al = voxel_world_pu
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

                        let can_save = !voxel_world.chunks.is_saving();
                        if ui
                            .add_enabled(can_save, egui::Button::new("Save").rounding(4.0))
                            .clicked()
                        {
                            voxel_world
                                .chunks
                                .save_terrain(&mut assets, &voxel_world.registry);
                        }

                        if ui.add(egui::Button::new("Load").rounding(4.0)).clicked() {
                            game_world.load();
                        }
                    });

                    if let Some(player_chunk_position) = voxel_world.chunks.player_chunk_position {
                        ui.label(egui::RichText::new("Current Chunk:").size(14.0));
                        ui.label(format!(
                            "Position: x: {}, y: {}, z: {}",
                            player_chunk_position.x,
                            player_chunk_position.y,
                            player_chunk_position.z
                        ));

                        ui.label(egui::RichText::new("Current terrain anchor:").size(14.0));
                        ui.label(format!(
                            "Position: x: {}, y: {}, z: {}",
                            voxel_world.chunks.renderable_chunks.chunk_anchor.x,
                            voxel_world.chunks.renderable_chunks.chunk_anchor.y,
                            voxel_world.chunks.renderable_chunks.chunk_anchor.z,
                        ));

                        ui.horizontal(|ui| {
                            ui.label("Radius:");
                            ui.add(egui::Slider::new(&mut debug_state.generate_radius, 0..=8));
                        });
                        if ui
                            .add(egui::Button::new("Regenerate chunks").rounding(4.0))
                            .clicked()
                        {
                            chunk_generator.generate_chunk(voxel_world, player_chunk_position);
                        }

                        if ui
                            .add(egui::Button::new("Spawn entity").rounding(4.0))
                            .clicked()
                        {
                            let mut player_query = ecs_world.player_query::<&Transform>();
                            let player_pos = player_query.player().1.position();
                            let player_dir = player_query
                                .player()
                                .1
                                .rotation()
                                .transform_vector(&Vector3::z());
                            let mut flat_model =
                                VoxelModelFlat::new_empty(Vector3::new(32, 32, 32));
                            for (local_pos, mut voxel) in flat_model.xyz_iter_mut() {
                                voxel.set_attachment(
                                    Attachment::PTMATERIAL,
                                    Some(
                                        PTMaterial::diffuse(Color::from(
                                            local_pos.map(|x| x as f32 / 31.0),
                                        ))
                                        .encode(),
                                    ),
                                );
                            }
                            let model_id = voxel_world.registry.register_renderable_voxel_model(
                                "entity",
                                VoxelModel::new(flat_model),
                            );
                            drop(player_query);
                            ecs_world.spawn((
                                GameEntity::new("new_entity"),
                                Transform::with_translation(Translation3::from(
                                    player_pos + player_dir * 2.0,
                                )),
                                RenderableVoxelEntity {
                                    voxel_model_id: model_id,
                                },
                            ));
                        }
                    }
                });
        });
    }
}
