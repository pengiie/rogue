use std::{collections::HashMap, f32};

use log::{debug, warn};
use nalgebra::{Matrix3, Matrix4, UnitQuaternion, Vector2, Vector3};
use rogue_macros::Resource;
use serde::{Deserialize, Serialize};

use super::{
    backend::{
        Binding, GfxBlendFactor, GfxBlendOp, GfxCullMode, GfxFilterMode, GfxFrontFace,
        GfxImageFormat, GfxRasterPipelineBlendStateAttachmentInfo,
        GfxRasterPipelineBlendStateCreateInfo, GfxVertexAttribute, GfxVertexAttributeFormat,
        GfxVertexFormat, GraphicsBackendFrameGraphExecutor, GraphicsBackendRecorder, Image,
        ShaderWriter,
    },
    camera::MainCamera,
    device::DeviceResource,
    frame_graph::{
        FrameGraph, FrameGraphComputeInfo, FrameGraphContext, FrameGraphContextImpl,
        FrameGraphImageInfo, FrameGraphRasterBlendInfo, FrameGraphRasterInfo, FrameGraphResource,
        FrameGraphVertexFormat,
    },
    shader::ShaderCompiler,
};
use crate::{
    common::color::Color,
    graphics::backend::ResourceId,
    settings::{GraphicsSettings, Settings},
    world::sky::Sky,
};
use crate::{
    entity::{self, ecs_world::ECSWorld},
    graphics::camera::Camera,
    physics::transform::Transform,
};
use crate::{
    graphics::frame_graph::IntoFrameGraphResource,
    window::{time::Time, window::Window},
};
use crate::{material::material_gpu::MaterialBankGpu, world::world_renderable::WorldRenderable};
use crate::{
    resource::{Res, ResMut},
    voxel::voxel_registry_gpu::VoxelModelRegistryGpu,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum Antialiasing {
    None,
    TAA,
}

#[derive(Resource)]
pub struct Renderer {
    pub frame_graph_executor: Box<dyn GraphicsBackendFrameGraphExecutor>,
    frame_graph: Option<FrameGraph>,
    backbuffer_size_input: Option<FrameGraphResource<Vector2<u32>>>,
    swapchain_size: Vector2<u32>,
    swapchain_image: Option<ResourceId<Image>>,
}

pub struct GraphConstants {
    pub image_swapchain: &'static str,
    pub image_swapchain_size: &'static str,
}

impl Renderer {
    pub const GRAPH: GraphConstants = GraphConstants {
        image_swapchain: "rogue_swapchain_image",
        image_swapchain_size: "rogue_swapchain_image_size",
    };

    pub const SET_CACHE_SLOT_FRAME: u32 = 0;

    pub fn new(device: &mut DeviceResource) -> Self {
        let frame_graph_executor = device.create_frame_graph_executor();

        Self {
            frame_graph_executor,
            frame_graph: None,
            backbuffer_size_input: None,
            swapchain_size: Vector2::zeros(),
            swapchain_image: None,
        }
    }

    /// Backbuffer is used for setting up the world cameras projection.
    pub fn set_frame_graph(
        &mut self,
        frame_graph: FrameGraph,
        backbuffer_size_input: impl IntoFrameGraphResource<Vector2<u32>>,
    ) {
        let backbuffer_size_input = backbuffer_size_input.handle(&frame_graph);
        self.frame_graph = Some(frame_graph);
        self.backbuffer_size_input = Some(backbuffer_size_input);
    }

    pub fn executor(&mut self) -> &mut dyn GraphicsBackendFrameGraphExecutor {
        &mut *self.frame_graph_executor
    }

    pub fn begin_frame(
        mut renderer: ResMut<Renderer>,
        device: ResMut<DeviceResource>,
        settings: Res<Settings>,
    ) {
        let renderer: &mut Renderer = &mut renderer;

        let Some(frame_graph) = renderer.frame_graph.take() else {
            log::error!("No frame graph has been submitted to the renderer.");
            return;
        };
        renderer.frame_graph_executor.begin_frame(frame_graph);

        // Write swapchain constants immediately.
        let swapchain_image = renderer
            .swapchain_image
            .take()
            .expect("Should not call begin_frame if swapchain image was not acquired.");
        let swapchain_image_info = device.get_image_info(&swapchain_image);

        // Write swapchain related inputs.
        renderer.swapchain_size = swapchain_image_info.resolution_xy();
        renderer
            .frame_graph_executor
            .supply_image_ref(Self::GRAPH.image_swapchain, &swapchain_image);
        renderer.frame_graph_executor.supply_input(
            Self::GRAPH.image_swapchain_size,
            Box::new(swapchain_image_info.resolution_xy()),
        );
    }

    pub fn write_frame_uniforms(
        mut renderer: ResMut<Renderer>,
        time: Res<Time>,
        ecs_world: Res<ECSWorld>,
        main_camera: Res<MainCamera>,
        material_bank_gpu: Res<MaterialBankGpu>,
        world_gpu: ResMut<WorldRenderable>,
        voxel_registry_gpu: Res<VoxelModelRegistryGpu>,
        sky: Res<Sky>,
    ) {
        let renderer = &mut *renderer;
        renderer
            .frame_graph_executor
            .write_uniforms(&mut |writer, ctx| {
                writer.write_set_cache("u_frame", Self::SET_CACHE_SLOT_FRAME);

                // FrameInfo struct
                writer.write_uniform(
                    "u_frame.frame_info.time_ms",
                    time.start_time().elapsed().as_millis() as u32,
                );

                // FrameWorldInfo struct
                if let Some(main_camera) = main_camera.camera() {
                    let (camera_transform, camera) = ecs_world
                        .query_one::<(&Transform, &Camera)>(main_camera)
                        .get()
                        .expect("Main camera should have a transform and camera component.");
                    let camera_world_transform =
                        ecs_world.get_world_transform(main_camera, camera_transform);

                    let backbuffer_size =
                        ctx.get_vec2(renderer.backbuffer_size_input.expect("Should exist."));
                    let aspect_ratio = backbuffer_size.x as f32 / backbuffer_size.y as f32;
                    let proj_view_matrix = camera.projection_matrix(aspect_ratio)
                        * camera_world_transform.to_view_matrix();
                    writer.write_uniform_mat4(
                        "u_frame.world_info.camera.proj_view",
                        &proj_view_matrix,
                    );

                    let transformation_matrix = camera_world_transform.to_transformation_matrix();
                    writer.write_uniform_mat4(
                        "u_frame.world_info.camera.transform",
                        &transformation_matrix,
                    );

                    let rot_matrix_3x3 = transformation_matrix.fixed_resize::<3, 3>(0.0);
                    writer
                        .write_uniform_mat3("u_frame.world_info.camera.rotation", &rot_matrix_3x3);
                    writer.write_uniform::<f32>("u_frame.world_info.camera.fov", camera.fov());
                    writer.write_uniform::<f32>(
                        "u_frame.world_info.camera.near_plane",
                        camera.near_plane(),
                    );
                    writer.write_uniform::<f32>(
                        "u_frame.world_info.camera.far_plane",
                        camera.far_plane(),
                    );
                } else {
                    writer.write_uniform_mat4(
                        "u_frame.world_info.camera.proj_view",
                        &Matrix4::zeros(),
                    );
                    writer.write_uniform_mat4(
                        "u_frame.world_info.camera.transform",
                        &Matrix4::zeros(),
                    );
                    writer.write_uniform_mat3(
                        "u_frame.world_info.camera.rotation",
                        &Matrix3::zeros(),
                    );
                    writer.write_uniform::<f32>("u_frame.world_info.camera.fov", 0.0);
                    writer.write_uniform::<f32>("u_frame.world_info.camera.near_plane", 0.0);
                    writer.write_uniform::<f32>("u_frame.world_info.camera.far_plane", 0.0);
                }
                writer
                    .write_uniform::<Vector3<f32>>("u_frame.world_info.sky.sun_dir", sky.sun_dir());

                // Material bank bindings
                writer.write_binding_array(
                    "u_frame.material_bank.textures_float4",
                    &material_bank_gpu.get_textures(),
                );
                //log::info!(
                //    "Writing textures count: {:?}",
                //    material_bank_gpu.get_textures()
                //);
                writer.write_binding_array(
                    "u_frame.material_bank.samplers",
                    &material_bank_gpu.get_samplers(),
                );
                writer.write_binding(
                    "u_frame.material_bank.materials",
                    material_bank_gpu.get_descriptor_buffer(),
                );

                // // Voxel entity bindings
                writer.write_binding(
                    "u_frame.voxel.entity_data.accel_buf",
                    *world_gpu.entities_accel_buf(),
                );
                writer.write_uniform(
                    "u_frame.voxel.entity_data.entity_count",
                    world_gpu.entities_accel_buf_count(),
                );

                // Voxel model bindings
                writer.write_binding(
                    "u_frame.voxel.model_info_data",
                    *voxel_registry_gpu.voxel_model_info_buffer(),
                );
                let world_data_buffers = voxel_registry_gpu
                    .voxel_data_allocator()
                    .buffers()
                    .into_iter()
                    .map(|b| Some(b))
                    .collect::<Vec<_>>();
                writer.write_binding_array(
                    "u_frame.voxel.model_voxel_data.model_voxel_data",
                    &world_data_buffers,
                );
                writer.write_binding_array(
                    "u_frame.voxel.model_voxel_data.model_voxel_data_rw",
                    &world_data_buffers,
                );

                // // Voxel terrain data
                writer.write_binding(
                    "u_frame.voxel.terrain.region_data",
                    *world_gpu.region_data_buffer(),
                );
                writer.write_binding(
                    "u_frame.voxel.terrain.region_ptrs_window",
                    *world_gpu.region_window_buffer(),
                );
                writer.write_uniform(
                    "u_frame.voxel.terrain.side_length",
                    world_gpu.region_window_side_length(),
                );
                writer.write_uniform(
                    "u_frame.voxel.terrain.region_anchor",
                    Vector3::<i32>::from(world_gpu.region_window_anchor()),
                );
                writer.write_uniform(
                    "u_frame.voxel.terrain.region_offset",
                    world_gpu.region_window_offset(),
                );
            });
    }

    /// Size of the last acquired swapchain image.
    pub fn swapchain_size(&self) -> Vector2<u32> {
        self.swapchain_size
    }

    pub fn acquire_swapchain_image(
        mut renderer: ResMut<Renderer>,
        mut device: ResMut<DeviceResource>,
        window: Res<Window>,
    ) {
        renderer.swapchain_image = match device.acquire_swapchain_image() {
            Ok(image) => Some(image),
            Err(err) => {
                let inner_size = window.inner_size();
                warn!("Tried to acquire swapchain error but got an error `{}`, trying to resize swapchain to {}x{}.", err, inner_size.width, inner_size.height);
                device.resize_swapchain(inner_size, true);
                None
            }
        };
    }

    pub fn end_frame(
        mut renderer: ResMut<Renderer>,
        mut device: ResMut<DeviceResource>,
        time: Res<Time>,
    ) {
        renderer.swapchain_image = None;
        renderer.frame_graph = Some(renderer.frame_graph_executor.end_frame());
    }

    pub fn did_acquire_swapchain(&self) -> bool {
        self.swapchain_image.is_some()
    }
}
