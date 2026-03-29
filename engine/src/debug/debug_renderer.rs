use std::collections::HashMap;

use nalgebra::{Matrix4, Point3, Translation3, UnitQuaternion, Vector3, Vector4};
use rogue_macros::Resource;

use crate::{
    asset::{
        asset::{AssetPath, Assets},
        gltf::GltfAsset,
    },
    common::{
        color::{Color, ColorSpaceSrgb, ColorSrgba},
        geometry::{obb::OBB, ray::Ray},
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
    physics::transform::Transform,
    resource::ResMut,
};

pub struct DebugRendererGraphConstants {
    pub pass_name: &'static str,
    pub raster_pipeline_name: &'static str,
    pub raster_pipeline_info: FrameGraphRasterInfo<'static>,
}

#[derive(bytemuck::Pod, Clone, Copy, bytemuck::Zeroable)]
#[repr(C)]
pub struct DebugMeshVertex {
    position: Vector3<f32>,
    normal: Vector3<f32>,
}
pub struct DebugMesh {
    vertices: Vec<DebugMeshVertex>,
    indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DebugShapeType {
    Arrow,
    Sphere,
    Cube,
    Capsule {
        // f32s quantized by 1000 for 0.0001 increments
        height: u32,
        radius: u32,
    },
}

impl DebugShapeType {
    fn encode_f32(x: f32) -> u32 {
        (x * 1000.0).floor() as u32
    }

    fn decode_f32(x: u32) -> f32 {
        (x as f32 / 1000.0)
    }
}

pub struct DebugShape {
    transform: nalgebra::Matrix4<f32>,
    color: ColorSrgba,
    flags: DebugShapeFlags,
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct DebugShapeFlags: u32 {
        const NONE = 0;
        const SHADING = 1;
        const DEPTH_TEST = 1 << 1;
    }
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
            .into_iter()
            .zip(normals.into_iter())
            .map(|(position, normal)| DebugMeshVertex { position, normal })
            .collect::<Vec<DebugMeshVertex>>();
        self.meshes
            .insert(shape_type, DebugMesh { vertices, indices });
    }

    pub fn draw_line_3d(
        &mut self,
        start: Vector3<f32>,
        end: Vector3<f32>,
        radius: f32,
        color: ColorSrgba,
        flags: DebugShapeFlags,
    ) {
        let diff = end - start;
        let rot = if (diff.normalize()).dot(&Vector3::y()) <= 0.9999 {
            UnitQuaternion::face_towards(&diff, &Vector3::y())
        } else {
            UnitQuaternion::from_scaled_axis(Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0))
        };
        let midpoint = (start + end) * 0.5;
        let isometry = nalgebra::Isometry3::from_parts(Translation3::from(midpoint), rot);
        let scale = Vector3::new(radius, radius, diff.norm() * 0.5 + radius);
        self.draw_cube(isometry, scale, color, flags);
    }

    pub fn draw_obb_filled(&mut self, obb: &OBB, color: ColorSrgba, flags: DebugShapeFlags) {
        let (min, _) = obb.rotated_min_max();
        let side_length = obb.aabb.side_length();
        let center = min + side_length * 0.5;
        let isometry = nalgebra::Isometry3::from_parts(
            Translation3::from(center),
            UnitQuaternion::face_towards(&obb.forward(), &obb.up()),
        );
        let scale = Vector3::new(side_length.x, side_length.y, side_length.z) * 0.5;
        self.draw_cube(isometry, scale, color, flags);
    }

    pub fn draw_capsule(
        &mut self,
        isometry: nalgebra::Isometry3<f32>,
        radius: f32,
        height: f32,
        color: ColorSrgba,
        flags: DebugShapeFlags,
    ) {
        assert!(
            height >= 0.0,
            "Height must be non-negative, if 0.0, use a sphere."
        );

        let transform = isometry.to_homogeneous();
        self.shapes
            .entry(DebugShapeType::Capsule {
                height: DebugShapeType::encode_f32(height),
                radius: DebugShapeType::encode_f32(radius),
            })
            .or_default()
            .push(DebugShape {
                transform,
                color,
                flags,
            });
    }

    pub fn draw_cube(
        &mut self,
        isometry: nalgebra::Isometry3<f32>,
        scale: Vector3<f32>,
        color: ColorSrgba,
        flags: DebugShapeFlags,
    ) {
        let transform =
            isometry.to_homogeneous() * nalgebra::Matrix4::new_nonuniform_scaling(&scale);
        self.shapes
            .entry(DebugShapeType::Cube)
            .or_default()
            .push(DebugShape {
                transform,
                color,
                flags,
            });
    }

    fn arrow_transform(start: Vector3<f32>, end: Vector3<f32>, scale: f32) -> Matrix4<f32> {
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
        isometry.to_homogeneous() * scale.to_homogeneous()
    }

    pub fn raycast_arrow(
        &self,
        ray: &Ray,
        start: Vector3<f32>,
        end: Vector3<f32>,
        scale: f32,
    ) -> Option</*ray_t*/ f32> {
        let transform = Self::arrow_transform(start, end, scale);
        let arrow_mesh = self.meshes.get(&DebugShapeType::Arrow).unwrap();
        let indices = &arrow_mesh.indices;
        let vertices = &arrow_mesh.vertices;
        let tri_count = indices.len() / 3;
        for i in 0..tri_count {
            let v0 = vertices[indices[i * 3] as usize].position;
            let v1 = vertices[indices[i * 3 + 1] as usize].position;
            let v2 = vertices[indices[i * 3 + 2] as usize].position;
            let v0 = transform * Vector4::new(v0.x, v0.y, v0.z, 1.0);
            let v1 = transform * Vector4::new(v1.x, v1.y, v1.z, 1.0);
            let v2 = transform * Vector4::new(v2.x, v2.y, v2.z, 1.0);
            // Shouldn't need to divide by w since its always 1.
            let v0 = Vector3::new(v0.x, v0.y, v0.z);
            let v1 = Vector3::new(v1.x, v1.y, v1.z);
            let v2 = Vector3::new(v2.x, v2.y, v2.z);
            if let Some(t) = ray.intersect_tri(v0, v1, v2) {
                return Some(t);
            }
        }
        return None;
    }

    pub fn draw_arrow(
        &mut self,
        start: Vector3<f32>,
        end: Vector3<f32>,
        scale: f32,
        color: ColorSrgba,
        flags: DebugShapeFlags,
    ) {
        let transform = Self::arrow_transform(start, end, scale);
        self.shapes
            .entry(DebugShapeType::Arrow)
            .or_default()
            .push(DebugShape {
                transform,
                color,
                flags,
            });
    }

    pub fn draw_obb_outline(
        &mut self,
        obb: &OBB,
        line_radius: f32,
        color: ColorSrgba,
        flags: DebugShapeFlags,
    ) {
        let (min, _) = obb.rotated_min_max();
        let side_length = obb.aabb.side_length();
        // Bottom
        self.draw_line_3d(
            min,
            min + obb.right() * side_length.x,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min,
            min + obb.forward() * side_length.z,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min + obb.right() * side_length.x,
            min + obb.right() * side_length.x + obb.forward() * side_length.z,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min + obb.forward() * side_length.z,
            min + obb.right() * side_length.x + obb.forward() * side_length.z,
            line_radius,
            color,
            flags,
        );

        // Top
        let top_offset = obb.up() * side_length.y;
        self.draw_line_3d(
            min + top_offset,
            min + obb.right() * side_length.x + top_offset,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min + top_offset,
            min + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min + obb.right() * side_length.x + top_offset,
            min + obb.right() * side_length.x + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min + obb.forward() * side_length.z + top_offset,
            min + obb.right() * side_length.x + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
            flags,
        );

        // Lines between top and bottom.
        self.draw_line_3d(min, min + top_offset, line_radius, color, flags);
        self.draw_line_3d(
            min + obb.right() * side_length.x,
            min + obb.right() * side_length.x + top_offset,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min + obb.forward() * side_length.z,
            min + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
            flags,
        );
        self.draw_line_3d(
            min + obb.right() * side_length.x + obb.forward() * side_length.z,
            min + obb.right() * side_length.x + obb.forward() * side_length.z + top_offset,
            line_radius,
            color,
            flags,
        );
    }

    pub fn draw_sphere(
        &mut self,
        center: Vector3<f32>,
        radius: f32,
        color: ColorSrgba,
        flags: DebugShapeFlags,
    ) {
        let mut transform = nalgebra::Matrix4::new_scaling(radius).append_translation(&center);
        transform.m44 = 1.0;
        self.shapes
            .entry(DebugShapeType::Sphere)
            .or_default()
            .push(DebugShape {
                transform,
                color,
                flags,
            });
    }

    fn create_capsule_mesh(height: f32, radius: f32) -> DebugMesh {
        const RADIUS_SUBDIVISION: u32 = 16;
        const CAP_SUBDIVISION: u32 = 8;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let half_height = height * 0.5;

        // Cyliner vertices and indices.
        for y_step in 0..=1 {
            let y = -half_height + (y_step as f32 / 1.0) * height;
            for i in 0..=RADIUS_SUBDIVISION {
                let theta = i as f32 / RADIUS_SUBDIVISION as f32 * std::f32::consts::PI * 2.0;
                let x = radius * theta.cos();
                let z = radius * theta.sin();
                vertices.push(DebugMeshVertex {
                    position: Vector3::new(x, y, z),
                    normal: Vector3::new(x, 0.0, z).normalize(),
                });
            }
        }
        for y_step in 0..1 {
            for i in 0..RADIUS_SUBDIVISION {
                // Start from bottom up so looking from outside in.:
                // C --- D
                // | \   |
                // |   \ |
                // A --- B
                let a = y_step * (RADIUS_SUBDIVISION + 1) + i;
                let b = a + 1;
                let c = (y_step + 1) * (RADIUS_SUBDIVISION + 1) + i;
                let d = c + 1;

                indices.extend_from_slice(&[a as u32, c as u32, b as u32]);
                indices.extend_from_slice(&[c as u32, d as u32, b as u32]);
            }
        }

        // Top hemisphere of capsule.
        let top_offset = vertices.len() as u32;
        for y_step in 0..=CAP_SUBDIVISION {
            let phi = y_step as f32 / CAP_SUBDIVISION as f32 * std::f32::consts::FRAC_PI_2;
            let local_y = radius * phi.sin();
            let y = half_height + local_y;
            let r = radius * phi.cos();
            for i in 0..=RADIUS_SUBDIVISION {
                let theta = i as f32 / RADIUS_SUBDIVISION as f32 * 2.0 * std::f32::consts::PI;
                let x = r * theta.cos();
                let z = r * theta.sin();
                vertices.push(DebugMeshVertex {
                    position: Vector3::new(x, y, z),
                    normal: Vector3::new(x, local_y, z).normalize(),
                });
            }
        }
        for y_step in 0..CAP_SUBDIVISION {
            for i in 0..RADIUS_SUBDIVISION {
                // Same quad as cyliner indices.
                let a = top_offset + y_step * (RADIUS_SUBDIVISION + 1) + i;
                let b = a + 1;
                let c = top_offset + (y_step + 1) * (RADIUS_SUBDIVISION + 1) + i;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, c as u32, b as u32]);
                indices.extend_from_slice(&[c as u32, d as u32, b as u32]);
            }
        }

        // Bottom hemisphere of capsule, reverse order of y_step so we stay bottom up to
        // keep the indices winding order the same.
        let bottom_offset = vertices.len() as u32;
        for y_step in 0..=CAP_SUBDIVISION {
            let phi = y_step as f32 / CAP_SUBDIVISION as f32 * std::f32::consts::FRAC_PI_2;
            let local_y = -radius * phi.sin();
            let y = -half_height + local_y;
            let r = radius * phi.cos();
            for i in 0..=RADIUS_SUBDIVISION {
                let theta = i as f32 / RADIUS_SUBDIVISION as f32 * 2.0 * std::f32::consts::PI;
                let x = r * theta.cos();
                let z = r * theta.sin();
                vertices.push(DebugMeshVertex {
                    position: Vector3::new(x, y, z),
                    normal: Vector3::new(x, local_y, z).normalize(),
                });
            }
        }
        for y_step in 0..CAP_SUBDIVISION {
            for i in 0..RADIUS_SUBDIVISION {
                // Same quad as cyliner indices.
                let a = bottom_offset + y_step * (RADIUS_SUBDIVISION + 1) + i;
                let b = a + 1;
                let c = bottom_offset + (y_step + 1) * (RADIUS_SUBDIVISION + 1) + i;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, b as u32, c as u32]);
                indices.extend_from_slice(&[c as u32, b as u32, d as u32]);
            }
        }

        DebugMesh { vertices, indices }
    }

    fn write_mesh_buffers(&mut self, device_resource: &mut DeviceResource) {
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
        let debug_renderer = &mut *debug_renderer;
        let req_bytes = 16
            + Self::MAX_DRAW_COUNT as usize * std::mem::size_of::<ash::vk::DrawIndirectCommand>();
        device_resource.create_or_reallocate_buffer(
            &mut debug_renderer.count_draw_buffer,
            GfxBufferCreateInfo {
                name: "debug_renderer_count_draw_buffer".to_owned(),
                size: req_bytes as u64,
            },
        );

        // Capsules require custom meshes so check which ones we need to generate.
        let mut needs_mesh_write = debug_renderer.vertices_buffer.is_none();
        for (capsule_type, _) in debug_renderer.shapes.iter() {
            let DebugShapeType::Capsule { height, radius } = capsule_type else {
                continue;
            };
            if debug_renderer.meshes.contains_key(capsule_type) {
                continue;
            }
            let mesh = Self::create_capsule_mesh(
                DebugShapeType::decode_f32(*height),
                DebugShapeType::decode_f32(*radius),
            );
            debug_renderer.meshes.insert(*capsule_type, mesh);
            needs_mesh_write |= true;
        }
        if needs_mesh_write {
            debug_renderer.write_mesh_buffers(&mut device_resource);
        }

        #[repr(C)]
        #[derive(bytemuck::Pod, bytemuck::Zeroable, Copy, Clone)]
        struct MeshInstance {
            transform: nalgebra::Matrix4<f32>,
            color: Vector4<f32>,
            mesh_ptr: u32,
            flags: u32,
            padding: [u32; 2], // Pad to 16 byte alignment.
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
                    color: shape.color.rgba_vec(),
                    mesh_ptr: *debug_renderer.shape_mesh_offests.get(shape_type).unwrap(),
                    flags: 0,
                    padding: [0; 2],
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
