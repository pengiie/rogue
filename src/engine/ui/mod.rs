use gui::Egui;
use log::debug;
use state::DebugUIState;

use super::{
    graphics::renderer::Renderer,
    resource::{Res, ResMut},
    voxel::voxel_world::{VoxelWorld, VoxelWorldGpu},
    window::{
        time::{Instant, Time},
        window::Window,
    },
};

pub mod gui;
pub mod state;

pub fn initialize_debug_ui_resource(app: &mut crate::app::App) {
    let egui = Egui::new(&app.get_resource::<Window>());
    app.insert_resource(egui);
    app.insert_resource(DebugUIState::default());
}

pub struct UI {}

impl UI {
    pub fn update(
        mut egui: ResMut<Egui>,
        mut state: ResMut<DebugUIState>,
        time: Res<Time>,
        renderer: Res<Renderer>,
    ) {
        // Determine if we should poll for the current fps, ensures the fps doesn't change
        // rapidly where it is unreadable.
        if state.last_ui_update.elapsed().as_millis() >= state.polling_time_ms.into() {
            state.last_ui_update = Instant::now();

            state.fps = time.fps();
            state.delta_time_ms = time.delta_time().as_micros() as f32 / 1000.0;
            //state.samples = renderer.sample_count();
        }
    }

    pub fn draw(
        window: Res<Window>,
        mut egui: ResMut<Egui>,
        mut state: ResMut<DebugUIState>,
        voxel_world: Res<VoxelWorldGpu>,
    ) {
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

            egui::Window::new("diagnostics")
                .current_pos(egui::pos2(4.0, 4.0))
                .movable(false)
                .show(ctx, |ui| {
                    ui.label(format!("FPS: {}", state.fps));
                    ui.label(format!("Samples: {}", state.samples));
                    ui.label(format!("Frame time: {}ms", state.delta_time_ms));
                    ui.label(format!("Currently allocated: {}", total_allocation_str));
                    ui.add(
                        egui::Slider::new(&mut state.player_fov, 10.0..=170.0)
                            .text("fov")
                            .drag_value_speed(0.1),
                    );
                    ui.add(egui::Checkbox::new(&mut state.draw_grid, "Grid"));
                });
        });
    }
}
