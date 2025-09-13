use std::time::Duration;

use egui_plot::PlotPoints;

use crate::{
    common::util::format_bytes,
    engine::{
        asset::asset::Assets,
        editor::editor::Editor,
        entity::ecs_world::ECSWorld,
        ui::EditorUIState,
        voxel::{voxel_world::VoxelWorld, voxel_world_gpu::VoxelWorldGpu},
        window::time::{Instant, Time},
    },
    session::Session,
};

pub fn stats_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_world: &mut VoxelWorld,
    voxel_world_gpu: &mut VoxelWorldGpu,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
    time: &Time,
) {
    let content = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new("Statistics").size(20.0));
        ui.add_space(16.0);
        let stats = &mut ui_state.stats;

        let time_between_samples =
            Duration::from_secs_f32(stats.time_length.as_secs_f32() / stats.samples as f32);
        stats.cpu_frame_time_samples_max = stats.cpu_frame_time_samples_max.max(time.delta_time());
        if stats.last_sample.elapsed() > time_between_samples {
            stats
                .cpu_frame_time_samples
                .push_back(stats.cpu_frame_time_samples_max);
            if stats.cpu_frame_time_samples.len() > stats.samples as usize {
                stats.cpu_frame_time_samples.pop_front();
            }
            stats.last_sample = Instant::now();
            stats.cpu_frame_time_samples_max = Duration::ZERO;
        }
        let cpu_frame_time_points = PlotPoints::Owned(
            stats
                .cpu_frame_time_samples
                .iter()
                .enumerate()
                .map(|(i, time)| egui_plot::PlotPoint {
                    x: (i as f64 / stats.samples as f64) * -stats.time_length.as_secs_f64(),
                    y: time.as_micros() as f64 / 1000.0,
                })
                .collect::<Vec<egui_plot::PlotPoint>>(),
        );
        let cpu_frame_time_line = egui_plot::Line::new("Frame time (ms)", cpu_frame_time_points);

        egui_plot::Plot::new(egui::Id::new("frame_time_plot"))
            .link_axis(egui::Id::new("timings_plot"), egui::Vec2b::new(true, true))
            .view_aspect(1.0)
            .include_x(-stats.time_length.as_secs_f64())
            .include_x(0.0)
            .include_y(3.0)
            .include_y(10.0)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .legend(egui_plot::Legend::default())
            .y_grid_spacer(|grid_input| {
                let mut v = Vec::new();
                let mut distance = grid_input.bounds.1 - grid_input.bounds.0;
                let marks = 10;
                let step_size = distance / marks as f64;
                for i in 0..=marks {
                    v.push(egui_plot::GridMark {
                        value: grid_input.bounds.0 + i as f64 * step_size,
                        step_size,
                    });
                }
                v
            })
            .set_margin_fraction(egui::vec2(0.0, 0.0))
            .cursor_color(egui::Color32::TRANSPARENT)
            .show(ui, |ui| {
                ui.line(cpu_frame_time_line);
            });

        let used_bytes = ui.label(format!(
            "Total allocated GPU voxel data: {}",
            format_bytes(voxel_world_gpu.voxel_allocator().total_allocation_size())
        ));
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}
