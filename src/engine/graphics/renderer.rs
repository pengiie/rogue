use std::collections::HashMap;

use log::{debug, warn};
use nalgebra::{Vector2, Vector3};
use rogue_macros::Resource;
use serde::{Deserialize, Serialize};

use crate::{
    common::color::Color,
    engine::{
        ecs::{self, ecs_world::ECSWorld},
        resource::{Res, ResMut},
        voxel::voxel_world::{self, VoxelWorldGpu},
        window::{time::Time, window::Window},
    },
    settings::{GraphicsSettings, Settings},
};

use super::{
    backend::{
        Binding, GfxFilterMode, GfxImageFormat, GraphicsBackendFrameGraphExecutor,
        GraphicsBackendRecorder, Image, UniformData,
    },
    camera::MainCamera,
    device::DeviceResource,
    frame_graph::{
        FrameGraph, FrameGraphComputeInfo, FrameGraphContext, FrameGraphContextImpl,
        FrameGraphImageInfo, FrameGraphResource,
    },
    pass,
    shader::ShaderCompiler,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum Antialiasing {
    None,
    TAA,
}

#[derive(Resource)]
pub struct Renderer {
    frame_graph_executor: Box<dyn GraphicsBackendFrameGraphExecutor>,
    frame_graph: Option<FrameGraph>,
}

struct GraphConstantsRT {
    image_albedo: &'static str,
    image_depth: &'static str,

    buffer_terrain_acceleration_data: &'static str,
    buffer_model_info_data: &'static str,
    buffer_model_voxel_data: &'static str,

    compute_prepass: &'static str,
    compute_prepass_info: FrameGraphComputeInfo<'static>,
    pass_prepass: &'static str,
}

struct GraphConstantsPostProcess {
    image_backbuffer: &'static str,

    compute_post_process: &'static str,
    compute_post_process_info: FrameGraphComputeInfo<'static>,
    pass_post_process: &'static str,
}

struct GraphConstants {
    rt: GraphConstantsRT,
    post_process: GraphConstantsPostProcess,

    frame_info_buffer: &'static str,
    frame_world_info_buffer: &'static str,

    swapchain: &'static str,
    swapchain_size: &'static str,
}

impl Renderer {
    pub const GRAPH: GraphConstants = GraphConstants {
        rt: GraphConstantsRT {
            image_albedo: "rt_image_albedo",
            image_depth: "rt_image_depth",

            buffer_terrain_acceleration_data: "rt_buffer_terrain_acceleration_data",
            buffer_model_info_data: "rt_buffer_model_info_data",
            buffer_model_voxel_data: "rt_buffer_model_voxel_data",

            compute_prepass: "rt_compute_prepass",
            compute_prepass_info: FrameGraphComputeInfo {
                shader_path: "terrain_prepass",
                entry_point_fn: "main",
            },
            pass_prepass: "rt_pass_prepass",
        },
        post_process: GraphConstantsPostProcess {
            image_backbuffer: "post_process_image_backbuffer",
            compute_post_process: "post_process_compute_post_process",
            compute_post_process_info: FrameGraphComputeInfo {
                shader_path: "post_process",
                entry_point_fn: "main",
            },
            pass_post_process: "post_process_pass_post_process",
        },

        frame_info_buffer: "frame_info_buffer",
        frame_world_info_buffer: "frame_world_info_buffer",

        swapchain: "swapchain",
        swapchain_size: "swapchain_size",
    };

    pub fn new(device: &mut DeviceResource) -> Self {
        let frame_graph_executor = device.create_frame_graph_executor();

        Self {
            frame_graph_executor,
            frame_graph: None,
        }
    }

    fn construct_frame_graph(gfx_settings: &GraphicsSettings) -> FrameGraph {
        let mut builder = FrameGraph::builder();

        // General frame resources.

        let swapchain_image = builder.create_input_image(Self::GRAPH.swapchain);
        let swapchain_size_input = builder.create_input(Self::GRAPH.swapchain_size);

        let frame_info_buffer = builder.create_frame_buffer(Self::GRAPH.frame_info_buffer);
        let frame_world_info_buffer =
            builder.create_frame_buffer(Self::GRAPH.frame_world_info_buffer);

        // RT passes
        let rt_albedo_image = builder.create_frame_image(
            Self::GRAPH.rt.image_albedo,
            FrameGraphImageInfo::new_rgba32float(gfx_settings.rt_size),
        );

        {
            let rt_depth_image = builder.create_frame_image(
                Self::GRAPH.rt.image_depth,
                FrameGraphImageInfo::new_depth(gfx_settings.rt_size),
            );

            let rt_buffer_terrain_acceleration_data =
                builder.create_input_buffer(Self::GRAPH.rt.buffer_terrain_acceleration_data);
            let rt_buffer_model_info_data =
                builder.create_input_buffer(Self::GRAPH.rt.buffer_model_info_data);
            let rt_buffer_model_voxel_data =
                builder.create_input_buffer(Self::GRAPH.rt.buffer_model_voxel_data);

            builder.create_compute_pipeline(
                Self::GRAPH.rt.compute_prepass,
                Self::GRAPH.rt.compute_prepass_info,
            );
            builder.create_input_pass(
                Self::GRAPH.rt.pass_prepass,
                &[
                    &Self::GRAPH.frame_info_buffer,
                    &Self::GRAPH.frame_world_info_buffer,
                    &rt_buffer_terrain_acceleration_data,
                    &rt_buffer_model_info_data,
                    &rt_buffer_model_voxel_data,
                ],
                &[&Self::GRAPH.rt.image_albedo],
            );
        }

        // Post process, blit to swapchain.
        {
            let backbuffer_image = builder.create_frame_image_with_ctx(
                Self::GRAPH.post_process.image_backbuffer,
                move |ctx: &FrameGraphContext| {
                    FrameGraphImageInfo::new_rgba8(ctx.get_vec2(swapchain_size_input))
                },
            );

            let post_process_compute_pipline = builder.create_compute_pipeline(
                Self::GRAPH.post_process.compute_post_process,
                Self::GRAPH.post_process.compute_post_process_info,
            );

            builder.create_pass(
                Self::GRAPH.post_process.pass_post_process,
                &[
                    &Self::GRAPH.rt.image_albedo,
                    &Self::GRAPH.post_process.image_backbuffer,
                ],
                &[&swapchain_image],
                move |recorder, ctx| {
                    let rt_image = ctx.get_image(Self::GRAPH.rt.image_albedo);
                    let backbuffer_image = ctx.get_image(Self::GRAPH.post_process.image_backbuffer);
                    let backbuffer_image_size =
                        recorder.get_image_info(&backbuffer_image).resolution_xy();
                    assert_eq!(
                        backbuffer_image_size,
                        ctx.get_vec2(Self::GRAPH.swapchain_size),
                        "Swapchain and post-process backbuffer image should be the same size"
                    );

                    {
                        let compute_pipeline =
                            ctx.get_compute_pipeline(post_process_compute_pipline);

                        let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);
                        compute_pass.bind_uniforms({
                            let mut data = UniformData::new();
                            data.load("u_shader.rt_final", rt_image.as_sampled_binding());
                            data.load("u_shader.backbuffer", backbuffer_image.as_storage_binding());
                            data
                        });

                        let wg_size = compute_pass.workgroup_size();
                        compute_pass.dispatch(
                            (backbuffer_image_size.x as f32 / wg_size.x as f32).ceil() as u32,
                            (backbuffer_image_size.y as f32 / wg_size.y as f32).ceil() as u32,
                            1,
                        );
                    }

                    let swapchain_image = ctx.get_image(Self::GRAPH.swapchain);
                    recorder.blit(backbuffer_image, swapchain_image, GfxFilterMode::Nearest);
                },
            );
        }

        // Present.
        builder.present_image(swapchain_image);

        builder.bake().unwrap()
    }

    pub fn begin_frame(
        mut renderer: ResMut<Renderer>,
        device: ResMut<DeviceResource>,
        mut shader_compiler: ResMut<ShaderCompiler>,
        settings: Res<Settings>,
    ) {
        let renderer: &mut Renderer = &mut renderer;

        let frame_graph = renderer
            .frame_graph
            .take()
            .unwrap_or_else(|| Self::construct_frame_graph(&settings.graphics));
        renderer
            .frame_graph_executor
            .begin_frame(&mut shader_compiler, frame_graph);
    }

    pub fn write_common_render_data(
        mut renderer: ResMut<Renderer>,
        time: Res<Time>,
        voxel_world_gpu: Res<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        main_camera: Res<MainCamera>,
    ) {
        // Write buffers.
        let frame_info: structs::FrameInfo = structs::FrameInfo {
            time_ms: time.start_time().elapsed().as_millis() as u32,
        };
        renderer.frame_graph_executor.write_buffer_slice(
            Self::GRAPH.frame_info_buffer,
            bytemuck::bytes_of(&frame_info),
        );

        let frame_world_info: structs::FrameWorldInfo = structs::FrameWorldInfo {
            camera: {
                let mut camera_query = ecs_world.get_main_camera(&main_camera);
                let (camera_transform, camera) = camera_query.get().unwrap();

                let transform = camera_transform.to_view_matrix().transpose();
                let transform_arr: [f32; 16] = transform.as_slice().try_into().unwrap();
                let rotation_arr = transform_arr[0..15].try_into().unwrap();
                structs::Camera {
                    transform: transform_arr,
                    rotation: rotation_arr,
                    fov: camera.fov(),
                }
            },
        };
        renderer.frame_graph_executor.write_buffer_slice(
            Self::GRAPH.frame_world_info_buffer,
            bytemuck::bytes_of(&frame_world_info),
        );

        // TODO: Move this out to a separate pass struct for rt.
        renderer.frame_graph_executor.supply_buffer_ref(
            &Self::GRAPH.rt.buffer_terrain_acceleration_data,
            voxel_world_gpu.world_terrain_acceleration_buffer(),
        );
        renderer.frame_graph_executor.supply_buffer_ref(
            &Self::GRAPH.rt.buffer_model_info_data,
            voxel_world_gpu.world_voxel_model_info_buffer(),
        );
        renderer.frame_graph_executor.supply_buffer_ref(
            &Self::GRAPH.rt.buffer_model_voxel_data,
            voxel_world_gpu.world_data_buffer().unwrap(),
        );
    }

    pub fn finish_frame(
        mut renderer: ResMut<Renderer>,
        mut device: ResMut<DeviceResource>,
        window: Res<Window>,
        time: Res<Time>,
    ) {
        let swapchain_image = match device.acquire_swapchain_image() {
            Ok(image) => image,
            Err(err) => {
                let inner_size = window.inner_size();
                warn!("Tried to acquire swapchain error but got an error `{}`, trying to resize swapchain to {}x{}.", err, inner_size.width, inner_size.height);
                device.resize_swapchain(inner_size, true);
                return;
            }
        };
        let swapchain_image_info = device.get_image_info(&swapchain_image);

        // -------- Common inputs ---------

        renderer
            .frame_graph_executor
            .supply_image_ref(Self::GRAPH.swapchain, &swapchain_image);
        renderer.frame_graph_executor.supply_input(
            Self::GRAPH.swapchain_size,
            Box::new(swapchain_image_info.resolution_xy()),
        );

        // -------- RT Pass ---------------

        // Supply pass logic.
        renderer.frame_graph_executor.supply_pass_ref(
            Self::GRAPH.rt.pass_prepass,
            Box::new(
                move |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext| {
                    let rt_image = ctx.get_image(Self::GRAPH.rt.image_albedo);
                    let rt_image_size = recorder.get_image_info(&rt_image).resolution_xy();

                    let compute_pipeline = ctx.get_compute_pipeline(Self::GRAPH.rt.compute_prepass);

                    {
                        let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);
                        compute_pass.bind_uniforms({
                            let mut uniforms = UniformData::new();

                            uniforms.load(
                                "u_frame.frame_info",
                                ctx.get_buffer(Self::GRAPH.frame_info_buffer)
                                    .as_uniform_binding(),
                            );
                            uniforms.load(
                                "u_frame.world_info",
                                ctx.get_buffer(Self::GRAPH.frame_world_info_buffer)
                                    .as_uniform_binding(),
                            );

                            uniforms.load("u_shader.backbuffer", rt_image.as_storage_binding());
                            uniforms.load(
                                "u_shader.terrain_acceleration_data",
                                ctx.get_buffer(Self::GRAPH.rt.buffer_terrain_acceleration_data)
                                    .as_storage_binding(),
                            );
                            uniforms.load(
                                "u_shader.model_info_data",
                                ctx.get_buffer(Self::GRAPH.rt.buffer_model_info_data)
                                    .as_storage_binding(),
                            );
                            uniforms.load(
                                "u_shader.model_voxel_data",
                                ctx.get_buffer(Self::GRAPH.rt.buffer_model_voxel_data)
                                    .as_storage_binding(),
                            );
                            uniforms
                        });
                        let wg_size = compute_pass.workgroup_size();
                        compute_pass.dispatch(
                            (rt_image_size.x as f32 / wg_size.x as f32).ceil() as u32,
                            (rt_image_size.y as f32 / wg_size.y as f32).ceil() as u32,
                            1,
                        );
                    }
                },
            ),
        );

        renderer.frame_graph = Some(renderer.frame_graph_executor.end_frame());
    }
}

// ------ Shader uniform structs, TODO: Fetch memory layout info from slang and write a
// BufferWriter API. ------------------------------------------------------------------

pub mod structs {

    macro_rules! shader_struct {
        ($name: item) => {
            #[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
            #[repr(C)]
            $name
        };
    }

    shader_struct!(
        #[repr(align(16))]
        pub struct Camera {
            pub transform: [f32; 16], // matrix4x4
            pub rotation: [f32; 15],  // matrix3x3
            pub fov: f32,
        }
    );

    shader_struct!(
        pub struct FrameWorldInfo {
            pub camera: Camera,
        }
    );

    shader_struct!(
        pub struct FrameInfo {
            pub time_ms: u32,
        }
    );
}
