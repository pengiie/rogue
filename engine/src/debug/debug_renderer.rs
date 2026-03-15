use std::collections::HashMap;

use nalgebra::{Point3, Translation3, UnitQuaternion, Vector3};
use rogue_macros::Resource;

use crate::{
    asset::{
        asset::{AssetPath, Assets},
        gltf::GltfAsset,
    },
    common::{
        color::{Color, ColorSpaceSrgb},
        geometry::obb::OBB,
    },
    graphics::{
        backend::{
            Buffer, GfxBlendFactor, GfxBlendOp, GfxBufferCreateInfo, GfxCullMode, GfxFrontFace,
            GfxRasterPipelineBlendStateAttachmentInfo, GfxRenderPassAttachment,
            GraphicsBackendRecorder, Image, ResourceId, ShaderWriter,
        },
        device::DeviceResource,
        frame_graph::{
            FrameGraphBuilder, FrameGraphContext, FrameGraphRasterBlendInfo, FrameGraphRasterInfo,
            FrameGraphResource, FrameGraphVertexFormat, IntoFrameGraphResource,
            IntoFrameGraphResourceUntyped, Pass,
        },
        renderer::Renderer,
    },
    resource::ResMut,
};

pub struct DebugRendererGraphConstants {
    pub pass_name: &'static str,
    pub raster_pipeline_name: &'static str,
    pub raster_pipeline_info: FrameGraphRasterInfo<'static>,
}

pub struct DebugMesh {
    vertices: Vec<f32>,
    indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DebugShapeType {
    Arrow,
    Sphere,
    Cube,
}

pub struct DebugShape {
    transform: nalgebra::Matrix4<f32>,
    color: Color<ColorSpaceSrgb>,
}

#[derive(Resource)]
pub struct DebugRenderer {
    meshes: HashMap<DebugShapeType, DebugMesh>,

    // Buffer with the first 4 bytes containing the draw count, and then from byte 16 (due to
    // alignment) onwards the
    // draw commands.
    count_draw_buffer: Option<ResourceId<Buffer>>,
    vertices_buffer: Option<ResourceId<Buffer>>,
    indices_buffer: Option<ResourceId<Buffer>>,
    mesh_info_buffer: Option<ResourceId<Buffer>>,
    instances_buffer: Option<ResourceId<Buffer>>,
    shape_mesh_offests: HashMap<DebugShapeType, /*byte_offset*/ u32>,
    shape_mesh_vertex_offset: HashMap<DebugShapeType, /*byte_offset*/ u32>,
    shape_mesh_index_offset: HashMap<DebugShapeType, /*byte_offset*/ u32>,

    shapes: HashMap<DebugShapeType, Vec<DebugShape>>,
    graph_framebuffer: Option<FrameGraphResource<Image>>,
    graph_framebuffer_depth: Option<FrameGraphResource<Image>>,
}

impl DebugRenderer {
    const GRAPH: DebugRendererGraphConstants = DebugRendererGraphConstants {
        pass_name: "debug_renderer_pass",
        raster_pipeline_name: "debug_renderer_raster_pipeline",
        raster_pipeline_info: FrameGraphRasterInfo {
            vertex_shader_path: "debug_raster",
            vertex_entry_point_fn: "main_vs",
            fragment_shader_path: "debug_raster",
            fragment_entry_point_fn: "main_fs",
            vertex_format: FrameGraphVertexFormat {
                // Mesh data will be in a storage buffer.
                attributes: &[],
            },
            blend_state: FrameGraphRasterBlendInfo {
                attachments: &[GfxRasterPipelineBlendStateAttachmentInfo {
                    enable_blend: true,
                    src_color_blend_factor: GfxBlendFactor::SrcAlpha,
                    dst_color_blend_factor: GfxBlendFactor::OneMinusSrcAlpha,
                    color_blend_op: GfxBlendOp::Add,
                    src_alpha_blend_factor: GfxBlendFactor::One,
                    dst_alpha_blend_factor: GfxBlendFactor::Zero,
                    alpha_blend_op: GfxBlendOp::Add,
                }],
            },
            cull_mode: GfxCullMode::Back,
            front_face: GfxFrontFace::CounterClockwise,
        },
    };
    const MAX_DRAW_COUNT: u32 = 1000;

    pub fn new() -> Self {
        let mut s = Self {
            meshes: HashMap::new(),
            count_draw_buffer: None,
            shapes: HashMap::new(),
            graph_framebuffer: None,
            graph_framebuffer_depth: None,

            vertices_buffer: None,
            indices_buffer: None,
            shape_mesh_offests: HashMap::new(),
            mesh_info_buffer: None,
            instances_buffer: None,
            shape_mesh_vertex_offset: HashMap::new(),
            shape_mesh_index_offset: HashMap::new(),
        };
        s.register_mesh(DebugShapeType::Arrow, "models::arrow::glb");
        s.register_mesh(DebugShapeType::Sphere, "models::sphere::glb");
        s.register_mesh(DebugShapeType::Cube, "models::cube::glb");
        s
    }

    pub fn register_mesh(&mut self, shape_type: DebugShapeType, asset_path: impl AsRef<str>) {
        let Ok(gltf) = Assets::load_asset_sync::<GltfAsset>(AssetPath::new_binary_dir(
            &asset_path.as_ref().clone(),
        )) else {
            log::error!(
                "Failed to load glTF asset for debug shape {:?} from path {:?}.",
                shape_type,
                asset_path.as_ref()
            );
            return;
        };

        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        let mesh = gltf
            .document
            .meshes()
            .next()
            .expect("Debug mesh gltf should have at least one mesh.");
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|buffer| {
                gltf.buffers
                    .get(buffer.index())
                    .map(|buffer_data| buffer_data.0.as_slice())
            });
            positions.extend(
                reader
                    .read_positions()
                    .unwrap()
                    .map(|pos| Vector3::new(pos[0], pos[1], pos[2])),
            );
            normals.extend(
                reader
                    .read_normals()
                    .unwrap()
                    .map(|n| Vector3::new(n[0], n[1], n[2])),
            );
            indices.extend(reader.read_indices().unwrap().into_u32());
        }

        let vertices = positions
            .iter()
            .zip(normals.iter())
            .flat_map(|(pos, normal)| [pos.x, pos.y, pos.z, normal.x, normal.y, normal.z])
            .collect::<Vec<f32>>();
        self.meshes
            .insert(shape_type, DebugMesh { vertices, indices });
    }

    pub fn draw_line(
        &mut self,
        start: Vector3<f32>,
        end: Vector3<f32>,
        radius: f32,
        color: Color<ColorSpaceSrgb>,
    ) {
        let diff = end - start;
        let rot = if diff != Vector3::y() {
            UnitQuaternion::face_towards(&diff, &Vector3::y())
        } else {
            UnitQuaternion::from_scaled_axis(Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0))
        };
        let midpoint = (start + end) * 0.5;
        let isometry = nalgebra::Isometry3::from_parts(Translation3::from(midpoint), rot);
        let scale = Vector3::new(radius, radius, diff.norm() * 0.5 + radius);
        self.draw_cube(isometry, scale, color);
    }

    pub fn draw_cube(
        &mut self,
        isometry: nalgebra::Isometry3<f32>,
        scale: Vector3<f32>,
        color: Color<ColorSpaceSrgb>,
    ) {
        let transform =
            isometry.to_homogeneous() * nalgebra::Matrix4::new_nonuniform_scaling(&scale);
        self.shapes
            .entry(DebugShapeType::Cube)
            .or_default()
            .push(DebugShape { transform, color });
    }

    pub fn draw_arrow(
        &mut self,
        start: Vector3<f32>,
        end: Vector3<f32>,
        scale: f32,
        color: Color<ColorSpaceSrgb>,
    ) {
        let diff = end - start;
        let isometry = if diff != Vector3::y() {
            nalgebra::Isometry3::<f32>::face_towards(
                &Point3::from(start),
                &Point3::from(end),
                &Vector3::y(),
            )
        } else {
            nalgebra::Isometry3::<f32>::new(
                start,
                Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0),
            )
        };
        let scale = nalgebra::Scale3::new(scale, scale, diff.norm());
        let transform = isometry.to_homogeneous() * scale.to_homogeneous();
        self.shapes
            .entry(DebugShapeType::Arrow)
            .or_default()
            .push(DebugShape { transform, color });
    }

    pub fn draw_obb(&mut self, obb: &OBB, line_radius: f32, color: Color<ColorSpaceSrgb>) {
        let (min, _) = obb.rotated_min_max();
        let side_length = obb.aabb.side_length();
        // Bottom
        self.draw_line(min, min + obb.right() * side_length.x, line_radius, color);
        self.draw_line(min, min + obb.forward() * side_length.z, line_radius, color);
        self.draw_line(
            min + obb.right() * side_length.x,
            min + obb.right() * side_length.x + obb.forward() * side_length.z,
            line_radius,
            color,
        );
        self.draw_line(
            min + obb.forward() * side_length.z,
            min + obb.right() * side_length.x + obb.forward() * side_length.z,
            line_radius,
            color,
        );

        // Top
        let top_offset = obb.up() * side_length.y;
        self.draw_line(
            min + top_offset,
            min + obb.right() * side_length.x + top_offset,
            line_radius,
            color,
        );
        self.draw_line(
            min + top_offset,
            min + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
        );
        self.draw_line(
            min + obb.right() * side_length.x + top_offset,
            min + obb.right() * side_length.x + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
        );
        self.draw_line(
            min + obb.forward() * side_length.z + top_offset,
            min + obb.right() * side_length.x + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
        );

        // Lines between top and bottom.
        self.draw_line(min, min + top_offset, line_radius, color);
        self.draw_line(
            min + obb.right() * side_length.x,
            min + obb.right() * side_length.x + top_offset,
            line_radius,
            color,
        );
        self.draw_line(
            min + obb.forward() * side_length.z,
            min + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
        );
        self.draw_line(
            min + obb.right() * side_length.x + obb.forward() * side_length.z,
            min + obb.right() * side_length.x + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
        );
    }

    pub fn draw_sphere(&mut self, center: Vector3<f32>, radius: f32, color: Color<ColorSpaceSrgb>) {
        let transform = nalgebra::Matrix4::new_scaling(radius).append_translation(&center);
        self.shapes
            .entry(DebugShapeType::Sphere)
            .or_default()
            .push(DebugShape { transform, color });
    }

    fn init_mesh_buffers(&mut self, device_resource: &mut DeviceResource) {
        let total_vertex_count = self
            .meshes
            .values()
            .map(|mesh| mesh.vertices.len())
            .sum::<usize>();
        device_resource.create_or_reallocate_buffer(
            &mut self.vertices_buffer,
            GfxBufferCreateInfo {
                name: "debug_renderer_vertices_buffer".to_owned(),
                size: (total_vertex_count * 6 * std::mem::size_of::<f32>()) as u64,
            },
        );

        let total_index_count = self
            .meshes
            .values()
            .map(|mesh| mesh.indices.len())
            .sum::<usize>();
        device_resource.create_or_reallocate_buffer(
            &mut self.indices_buffer,
            GfxBufferCreateInfo {
                name: "debug_renderer_indices_buffer".to_owned(),
                size: (total_index_count * std::mem::size_of::<u32>()) as u64,
            },
        );

        #[repr(C)]
        #[derive(bytemuck::Pod, bytemuck::Zeroable, Copy, Clone)]
        struct DebugMeshInfo {
            vertices_offset: u32,
            indices_offset: u32,
            vertex_count: u32,
            index_count: u32,
        }
        let mesh_info_size = self.meshes.len() * std::mem::size_of::<DebugMeshInfo>();
        device_resource.create_or_reallocate_buffer(
            &mut self.mesh_info_buffer,
            GfxBufferCreateInfo {
                name: "debug_renderer_mesh_info_buffer".to_owned(),
                size: mesh_info_size as u64,
            },
        );

        let mut mesh_info_data = Vec::with_capacity(mesh_info_size);
        // Offset in f32s
        let mut vertex_offset = 0;
        // Offset in u32s
        let mut index_offset = 0;
        for (mesh_type, mesh) in self.meshes.iter() {
            device_resource.write_buffer_slice(
                &self.vertices_buffer.unwrap(),
                vertex_offset as u64 * std::mem::size_of::<f32>() as u64,
                bytemuck::cast_slice(&mesh.vertices),
            );
            device_resource.write_buffer_slice(
                &self.indices_buffer.unwrap(),
                index_offset as u64 * std::mem::size_of::<u32>() as u64,
                bytemuck::cast_slice(&mesh.indices),
            );
            self.shape_mesh_offests.insert(
                *mesh_type,
                (mesh_info_data.len() / std::mem::size_of::<DebugMeshInfo>()) as u32,
            );
            mesh_info_data.extend_from_slice(bytemuck::bytes_of(&DebugMeshInfo {
                vertices_offset: vertex_offset,
                indices_offset: index_offset,

                vertex_count: mesh.vertices.len() as u32,
                index_count: mesh.indices.len() as u32,
            }));

            vertex_offset += (mesh.vertices.len() * 6) as u32;
            index_offset += (mesh.indices.len()) as u32;
        }

        device_resource.write_buffer_slice(
            &self.mesh_info_buffer.unwrap(),
            0,
            bytemuck::cast_slice(&mesh_info_data),
        );
    }

    pub fn write_render_data(
        mut debug_renderer: ResMut<Self>,
        mut device_resource: ResMut<DeviceResource>,
    ) {
        let req_bytes = 16
            + Self::MAX_DRAW_COUNT as usize * std::mem::size_of::<ash::vk::DrawIndirectCommand>();
        device_resource.create_or_reallocate_buffer(
            &mut debug_renderer.count_draw_buffer,
            GfxBufferCreateInfo {
                name: "debug_renderer_count_draw_buffer".to_owned(),
                size: req_bytes as u64,
            },
        );

        if debug_renderer.vertices_buffer.is_none() {
            assert!(
                debug_renderer.indices_buffer.is_none()
                    && debug_renderer.mesh_info_buffer.is_none()
            );
            debug_renderer.init_mesh_buffers(&mut device_resource);
        }

        #[repr(C)]
        #[derive(bytemuck::Pod, bytemuck::Zeroable, Copy, Clone)]
        struct MeshInstance {
            transform: nalgebra::Matrix4<f32>,
            color: Vector3<f32>,
            mesh_ptr: u32,
        }
        let instances_size = debug_renderer
            .shapes
            .values()
            .map(|shapes| shapes.len())
            .sum::<usize>()
            * std::mem::size_of::<MeshInstance>();
        device_resource.create_or_reallocate_buffer(
            &mut debug_renderer.instances_buffer,
            GfxBufferCreateInfo {
                name: "debug_renderer_to_draw_meshes".to_owned(),
                size: (instances_size as u64).max(1), // Can't have 0 size idk make it lazy later ig
            },
        );

        let count_draw_buffer = debug_renderer.count_draw_buffer.as_ref().unwrap();
        let mut draw_data = vec![0u8; req_bytes];
        let mut instances_data = Vec::with_capacity(instances_size);
        let draw_count = debug_renderer.shapes.len() as u32;
        draw_data[0..4].copy_from_slice(&draw_count.to_le_bytes());

        let mut draw_offset = 0;
        let mut instances_offset = 0;
        for (shape_type, shapes) in debug_renderer.shapes.iter() {
            let mesh = debug_renderer
                .meshes
                .get(shape_type)
                .expect("Should have mesh loaded for debug shape.");
            // Same as ash::vk::DrawIndirectCommand but separate cause bytemuck.
            #[repr(C)]
            #[derive(bytemuck::Pod, bytemuck::Zeroable, Copy, Clone)]
            struct DrawIndirectCommandData {
                pub vertex_count: u32,
                pub instance_count: u32,
                pub first_vertex: u32,
                pub first_instance: u32,
            }
            let draw_command = DrawIndirectCommandData {
                vertex_count: mesh.indices.len() as u32,
                instance_count: shapes.len() as u32,
                first_vertex: 0,
                first_instance: instances_offset,
            };
            let offset = 16 + draw_offset * std::mem::size_of::<ash::vk::DrawIndirectCommand>();
            let (start, end) = (
                offset,
                offset + std::mem::size_of::<DrawIndirectCommandData>(),
            );
            draw_data[start..end].copy_from_slice(bytemuck::bytes_of(&draw_command));
            draw_offset += 1;

            for shape in shapes {
                instances_data.extend_from_slice(bytemuck::bytes_of(&MeshInstance {
                    // Transpose since slang is row major.
                    transform: shape.transform.transpose(),
                    color: shape.color.xyz,
                    mesh_ptr: *debug_renderer.shape_mesh_offests.get(shape_type).unwrap(),
                }));
            }
            instances_offset += shapes.len() as u32;
        }

        if !draw_data.is_empty() {
            device_resource.write_buffer_slice(count_draw_buffer, 0, &draw_data);
        }
        if !instances_data.is_empty() {
            device_resource.write_buffer_slice(
                debug_renderer.instances_buffer.as_ref().unwrap(),
                0,
                &instances_data,
            );
        }

        debug_renderer.shapes.clear();
    }

    pub fn set_graph_debug_pass(
        &mut self,
        fg: &mut FrameGraphBuilder,
        framebuffer: impl IntoFrameGraphResource<Image>,
        framebuffer_depth: impl IntoFrameGraphResource<Image>,
        dependencies: &[&FrameGraphResource<Pass>],
    ) -> FrameGraphResource<Pass> {
        let framebuffer = framebuffer.handle(fg);
        let framebuffer_depth = framebuffer_depth.handle(fg);
        let mut inputs = dependencies
            .into_iter()
            .map(|x| *x as &dyn IntoFrameGraphResourceUntyped)
            .collect::<Vec<_>>();
        inputs.push(&framebuffer);
        inputs.push(&framebuffer_depth);

        let raster_pipeline = fg.create_raster_pipeline(
            Self::GRAPH.raster_pipeline_name,
            Self::GRAPH.raster_pipeline_info,
            &[&framebuffer],
            Some(&framebuffer_depth),
        );
        inputs.push(&raster_pipeline);

        let outputs = [&framebuffer] as [&dyn IntoFrameGraphResourceUntyped; _];
        self.graph_framebuffer = Some(framebuffer);
        self.graph_framebuffer_depth = Some(framebuffer_depth);
        fg.create_input_pass(Self::GRAPH.pass_name, &inputs, &outputs)
    }

    pub fn write_graph_pass(mut debug_renderer: ResMut<Self>, mut renderer: ResMut<Renderer>) {
        let framebuffer_image_handle = debug_renderer.graph_framebuffer.as_ref().expect(
            "Should not be writing debug renderer pass without setting it up in the render graph first.",
        );
        let framebuffer_depth_image_handle = debug_renderer.graph_framebuffer_depth.as_ref().expect(
            "Should not be writing debug renderer pass without setting it up in the render graph first.",
        );

        renderer.executor().supply_pass_ref(
            Self::GRAPH.pass_name,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                let framebuffer_image = ctx.get_image(framebuffer_image_handle);
                let framebuffer_image_info = recorder.get_image_info(&framebuffer_image);
                let framebuffer_depth = ctx.get_image(framebuffer_depth_image_handle);

                let raster_pipeline = ctx.get_raster_pipeline(Self::GRAPH.raster_pipeline_name);
                let mut render_pass = recorder.begin_render_pass(
                    raster_pipeline,
                    &[GfxRenderPassAttachment::new_load(framebuffer_image)],
                    Some(GfxRenderPassAttachment::new_load(framebuffer_depth)),
                );

                render_pass.set_scissor(
                    0,
                    0,
                    framebuffer_image_info.resolution.x,
                    framebuffer_image_info.resolution.y,
                );

                render_pass.bind_uniforms(&mut |writer| {
                    writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                });

                render_pass.draw_indirect_count(
                    debug_renderer.count_draw_buffer.unwrap(),
                    16,
                    debug_renderer.count_draw_buffer.unwrap(),
                    0,
                    Self::MAX_DRAW_COUNT,
                );
            },
        );
    }

    pub fn write_global_uniforms(&self, writer: &mut ShaderWriter) {
        writer.write_binding(
            "u_frame.debug.vertices_buffer",
            self.vertices_buffer.unwrap(),
        );
        writer.write_binding("u_frame.debug.indices_buffer", self.indices_buffer.unwrap());
        writer.write_binding(
            "u_frame.debug.mesh_info_buffer",
            self.mesh_info_buffer.unwrap(),
        );
        writer.write_binding(
            "u_frame.debug.instance_info_buffer",
            self.instances_buffer.unwrap(),
        );
    }
}
