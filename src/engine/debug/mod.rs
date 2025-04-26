use nalgebra::{Vector2, Vector3};
use rogue_macros::Resource;

use crate::common::color::{Color, ColorSpaceSrgb};

use super::{
    graphics::{
        backend::GraphicsBackendRecorder, frame_graph::FrameGraphContext, renderer::Renderer,
    },
    input::{keyboard, Input},
    resource::{Res, ResMut},
};

pub struct DebugGizmo {}

// Immediate mode shapes renderer.
#[derive(Resource)]
pub struct DebugRenderer {
    debug_lines: Vec<DebugLine>,
    show_debug: bool,
}

pub struct DebugLine {
    pub start: Vector3<f32>,
    pub end: Vector3<f32>,
    pub thickness: f32,
    pub color: Color<ColorSpaceSrgb>,
    pub alpha: f32,
}

impl DebugRenderer {
    pub fn new() -> Self {
        Self {
            debug_lines: Vec::new(),
            show_debug: true,
        }
    }

    pub fn draw_line(&mut self, line: DebugLine) {
        if !self.show_debug {
            return;
        }
        self.debug_lines.push(line);
    }

    pub fn write_debug_shapes_pass(
        mut debug: ResMut<DebugRenderer>,
        mut renderer: ResMut<Renderer>,
        input: Res<Input>,
    ) {
        if input.is_key_pressed(keyboard::Key::C) {
            debug.show_debug = !debug.show_debug;
        }
        let line_count = debug.debug_lines.len();

        if line_count > 0 {
            let lines_byte_size = line_count * 48;
            let lines_buffer_ref = renderer.frame_graph_executor.write_buffer(
                Renderer::GRAPH.debug_3d.buffer_lines,
                lines_byte_size as u64,
            );
            for (i, line) in debug.debug_lines.drain(..).enumerate() {
                let i = i * 48;
                lines_buffer_ref[i..(i + 12)].copy_from_slice(bytemuck::bytes_of(&line.start));
                lines_buffer_ref[(i + 16)..(i + 28)].copy_from_slice(bytemuck::bytes_of(&line.end));
                lines_buffer_ref[(i + 28)..(i + 32)].copy_from_slice(&line.thickness.to_le_bytes());
                lines_buffer_ref[(i + 32)..(i + 44)]
                    .copy_from_slice(bytemuck::bytes_of(&line.color.rgb_vec()));
                lines_buffer_ref[(i + 44)..(i + 48)].copy_from_slice(&line.alpha.to_le_bytes());
            }
        }

        renderer.frame_graph_executor.supply_pass_ref(
            Renderer::GRAPH.debug_3d.pass_debug,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                if line_count == 0 {
                    return;
                }

                let backbuffer_image = ctx.get_image(Renderer::GRAPH.image_backbuffer);
                let backbuffer_image_size =
                    recorder.get_image_info(&backbuffer_image).resolution_xy();
                let rt_image_depth = ctx.get_image(Renderer::GRAPH.rt.image_depth);
                let lines_buffer = ctx.get_buffer(Renderer::GRAPH.debug_3d.buffer_lines);

                let mut shapes_pass = recorder.begin_compute_pass(
                    ctx.get_compute_pipeline(Renderer::GRAPH.debug_3d.pipeline_compute_shapes),
                );

                shapes_pass.bind_uniforms(&mut |writer| {
                    writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                    writer.write_binding("u_shader.backbuffer", backbuffer_image);
                    writer.write_binding("u_shader.backbuffer_depth", rt_image_depth);
                    writer.write_binding("u_shader.lines", lines_buffer);
                    writer.write_uniform("u_shader.line_count", line_count as u32);
                });

                let wg_size = shapes_pass.workgroup_size();
                shapes_pass.dispatch(
                    (backbuffer_image_size.x as f32 / wg_size.x as f32).ceil() as u32,
                    (backbuffer_image_size.y as f32 / wg_size.y as f32).ceil() as u32,
                    1,
                );
            },
        );
    }
}
