use nalgebra::{UnitQuaternion, Vector2, Vector3};
use rogue_macros::Resource;

use super::{
    graphics::{
        backend::GraphicsBackendRecorder, frame_graph::FrameGraphContext, renderer::Renderer,
    },
    input::Input,
    resource::{Res, ResMut},
};
use crate::common::geometry::obb::OBB;
use crate::{
    common::color::{Color, ColorSpaceSrgb},
    consts,
};

pub struct DebugGizmo {}

// Immediate mode shapes renderer.
#[derive(Resource)]
pub struct DebugRenderer {
    debug_lines: Vec<DebugLine>,
    debug_rings: Vec<DebugRing>,
    debug_planes: Vec<DebugPlane>,
    show_debug: bool,
}

pub struct DebugLine {
    pub start: Vector3<f32>,
    pub end: Vector3<f32>,
    pub thickness: f32,
    pub color: Color<ColorSpaceSrgb>,
    pub alpha: f32,
    pub flags: DebugFlags,
}

pub struct DebugRing {
    pub center: Vector3<f32>,
    pub normal: Vector3<f32>,
    pub stretch: Vector2<f32>,
    pub thickness: f32,
    pub color: Color<ColorSpaceSrgb>,
    pub alpha: f32,
    pub flags: DebugFlags,
}

pub struct DebugCapsule {
    pub center: Vector3<f32>,
    /// Identity orientation is oriented with height along on the Y axis.
    pub orientation: UnitQuaternion<f32>,
    pub radius: f32,
    pub height: f32,
    pub color: Color<ColorSpaceSrgb>,
    pub alpha: f32,
    pub flags: DebugFlags,
}

pub struct DebugPlane {
    pub center: Vector3<f32>,
    pub normal: Vector3<f32>,
    pub size: Vector2<f32>,
    pub color: Color<ColorSpaceSrgb>,
    pub alpha: f32,
    pub flags: DebugFlags,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct DebugFlags: u32 {
        const NONE = 0;
        const XRAY = 1;
        const SHADING = 2;
    }
}

pub struct DebugOBB<'a> {
    pub obb: &'a OBB,
    pub thickness: f32,
    pub color: Color<ColorSpaceSrgb>,
    pub alpha: f32,
    pub flags: DebugFlags,
}

impl DebugRenderer {
    pub fn new() -> Self {
        Self {
            debug_lines: Vec::new(),
            debug_rings: Vec::new(),
            debug_planes: Vec::new(),
            show_debug: true,
        }
    }

    pub fn draw_line(&mut self, line: DebugLine) {
        if !self.show_debug {
            return;
        }
        self.debug_lines.push(line);
    }

    pub fn draw_ring(&mut self, ring: DebugRing) {
        if !self.show_debug {
            return;
        }
        self.debug_rings.push(ring);
    }

    pub fn draw_plane(&mut self, plane: DebugPlane) {
        if !self.show_debug {
            return;
        }
        self.debug_planes.push(plane);
    }

    /// Alias for draw line, but a different spec.
    pub fn draw_capsule(&mut self, ellipsoid: DebugCapsule) {
        let start =
            ellipsoid.center + ellipsoid.orientation * Vector3::y() * ellipsoid.height * 0.5;
        let end = ellipsoid.center + ellipsoid.orientation * Vector3::y() * -ellipsoid.height * 0.5;
        self.draw_line(DebugLine {
            start,
            end,
            thickness: ellipsoid.radius,
            color: ellipsoid.color,
            alpha: ellipsoid.alpha,
            flags: ellipsoid.flags,
        });
    }

    pub fn draw_obb(&mut self, debug_obb: DebugOBB) {
        if !self.show_debug {
            return;
        }
        let obb = debug_obb.obb;
        let rot = obb.rotation;
        let (min, max) = obb.rotated_min_max();
        let forward = rot.transform_vector(&(Vector3::z() * (obb.aabb.max.z - obb.aabb.min.z)));
        let right = rot.transform_vector(&(Vector3::x() * (obb.aabb.max.x - obb.aabb.min.x)));
        let up = rot.transform_vector(&(Vector3::y() * (obb.aabb.max.y - obb.aabb.min.y)));

        // Draws the edges of an OBB.
        let line = |start, end| DebugLine {
            start,
            end,
            thickness: debug_obb.thickness,
            color: debug_obb.color.clone(),
            alpha: debug_obb.alpha,
            flags: debug_obb.flags,
        };
        self.draw_line(line(min, min + forward));
        self.draw_line(line(min, min + right));
        self.draw_line(line(min + forward, min + right + forward));
        self.draw_line(line(min + right, min + right + forward));

        self.draw_line(line(min + up, min + forward + up));
        self.draw_line(line(min + up, min + right + up));
        self.draw_line(line(min + forward + up, max));
        self.draw_line(line(min + right + up, max));

        self.draw_line(line(min, min + up));
        self.draw_line(line(min + right, min + right + up));
        self.draw_line(line(min + forward, min + forward + up));
        self.draw_line(line(min + forward + right, max));
    }

    pub fn write_debug_shapes_pass(
        mut debug: ResMut<DebugRenderer>,
        mut renderer: ResMut<Renderer>,
        input: Res<Input>,
    ) {
        if input.is_key_pressed(consts::actions::keybind::EDITOR_TOGGLE_DEBUG) {
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
                lines_buffer_ref[(i + 12)..(i + 16)]
                    .copy_from_slice(&line.flags.bits().to_le_bytes());

                lines_buffer_ref[(i + 16)..(i + 28)].copy_from_slice(bytemuck::bytes_of(&line.end));
                lines_buffer_ref[(i + 28)..(i + 32)].copy_from_slice(&line.thickness.to_le_bytes());

                lines_buffer_ref[(i + 32)..(i + 44)]
                    .copy_from_slice(bytemuck::bytes_of(&line.color.rgb_vec()));
                lines_buffer_ref[(i + 44)..(i + 48)].copy_from_slice(&line.alpha.to_le_bytes());
            }
        } else {
            renderer
                .frame_graph_executor
                .write_buffer_slice(Renderer::GRAPH.debug_3d.buffer_lines, &[0u8; 16]);
        }

        let ring_count = debug.debug_rings.len();
        if ring_count > 0 {
            let rings_byte_size = ring_count * 16 * 4;
            let rings_buffer_ref = renderer.frame_graph_executor.write_buffer(
                Renderer::GRAPH.debug_3d.buffer_rings,
                rings_byte_size as u64,
            );
            for (i, ring) in debug.debug_rings.drain(..).enumerate() {
                let i = i * 16 * 4;

                rings_buffer_ref[i..(i + 12)].copy_from_slice(bytemuck::bytes_of(&ring.center));
                rings_buffer_ref[(i + 12)..(i + 16)]
                    .copy_from_slice(&ring.flags.bits().to_le_bytes());

                rings_buffer_ref[(i + 16)..(i + 24)]
                    .copy_from_slice(bytemuck::bytes_of(&ring.stretch));

                rings_buffer_ref[(i + 32)..(i + 44)]
                    .copy_from_slice(bytemuck::bytes_of(&ring.normal));
                rings_buffer_ref[(i + 44)..(i + 48)].copy_from_slice(&ring.thickness.to_le_bytes());

                rings_buffer_ref[(i + 48)..(i + 60)]
                    .copy_from_slice(bytemuck::bytes_of(&ring.color.rgb_vec()));
                rings_buffer_ref[(i + 60)..(i + 64)].copy_from_slice(&ring.alpha.to_le_bytes());
            }
        } else {
            // Dummy bytes so the descriptor is bound to a valid buffer.
            renderer
                .frame_graph_executor
                .write_buffer_slice(Renderer::GRAPH.debug_3d.buffer_rings, &[0u8; 16]);
        }

        let plane_count = debug.debug_planes.len();
        if plane_count > 0 {
            let planes_byte_size = plane_count * 16 * 4;
            let planes_buffer_ref = renderer.frame_graph_executor.write_buffer(
                Renderer::GRAPH.debug_3d.buffer_planes,
                planes_byte_size as u64,
            );
            for (i, plane) in debug.debug_planes.drain(..).enumerate() {
                let i = i * 16 * 4;

                planes_buffer_ref[i..(i + 12)].copy_from_slice(bytemuck::bytes_of(&plane.center));
                planes_buffer_ref[(i + 12)..(i + 16)]
                    .copy_from_slice(&plane.flags.bits().to_le_bytes());

                planes_buffer_ref[(i + 16)..(i + 28)]
                    .copy_from_slice(bytemuck::bytes_of(&plane.normal));

                planes_buffer_ref[(i + 32)..(i + 44)]
                    .copy_from_slice(bytemuck::bytes_of(&plane.color.rgb_vec()));
                planes_buffer_ref[(i + 44)..(i + 48)].copy_from_slice(&plane.alpha.to_le_bytes());
                planes_buffer_ref[(i + 48)..(i + 56)]
                    .copy_from_slice(bytemuck::bytes_of(&plane.size));
            }
        } else {
            // Dummy bytes so the descriptor is bound to a valid buffer.
            renderer
                .frame_graph_executor
                .write_buffer_slice(Renderer::GRAPH.debug_3d.buffer_planes, &[0u8; 16]);
        }

        renderer.frame_graph_executor.supply_pass_ref(
            Renderer::GRAPH.debug_3d.pass_debug,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                let backbuffer_image = ctx.get_image(Renderer::GRAPH.image_backbuffer);
                let backbuffer_image_size =
                    recorder.get_image_info(&backbuffer_image).resolution_xy();
                let rt_image_depth = ctx.get_image(Renderer::GRAPH.rt.image_depth);
                let lines_buffer = ctx.get_buffer(Renderer::GRAPH.debug_3d.buffer_lines);
                let rings_buffer = ctx.get_buffer(Renderer::GRAPH.debug_3d.buffer_rings);
                let planes_buffer = ctx.get_buffer(Renderer::GRAPH.debug_3d.buffer_planes);

                let mut shapes_pass = recorder.begin_compute_pass(
                    ctx.get_compute_pipeline(Renderer::GRAPH.debug_3d.pipeline_compute_shapes),
                );

                shapes_pass.bind_uniforms(&mut |writer| {
                    writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                    writer.write_binding("u_shader.backbuffer", backbuffer_image);
                    writer.write_binding("u_shader.backbuffer_depth", rt_image_depth);
                    writer.write_binding("u_shader.lines", lines_buffer);
                    writer.write_uniform("u_shader.line_count", line_count as u32);
                    writer.write_binding("u_shader.rings", rings_buffer);
                    writer.write_uniform("u_shader.ring_count", ring_count as u32);
                    writer.write_binding("u_shader.planes", planes_buffer);
                    writer.write_uniform("u_shader.plane_count", plane_count as u32);
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
