use std::collections::HashMap;

use log::{debug, warn};
use nalgebra::{Matrix3, Matrix4, Vector2, Vector3};
use rogue_macros::Resource;
use serde::{Deserialize, Serialize};

use crate::{
    common::color::Color,
    engine::{
        entity::{self, ecs_world::ECSWorld},
        resource::{Res, ResMut},
        ui::UI,
        voxel::voxel_world::{self, VoxelWorldGpu},
        window::{time::Time, window::Window},
    },
    settings::{GraphicsSettings, Settings},
};

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
    pass::{self, ui::UIPass},
    shader::ShaderCompiler,
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
    acquired_swapchain: bool,
    swapchain_size: Vector2<u32>,
}

pub struct GraphConstantsEditorUI {
    pub buffer_vertex_buffer: &'static str,
    pub buffer_index_buffer: &'static str,

    pub pipeline_raster_ui: &'static str,
    pub pipeline_raster_ui_info: FrameGraphRasterInfo<'static>,
    pub pass_ui: &'static str,
}

pub struct GraphConstantsDebug3D {
    pub buffer_lines: &'static str,

    pub pipeline_compute_shapes: &'static str,
    pub pipeline_compute_shapes_info: FrameGraphComputeInfo<'static>,
    pub pass_debug: &'static str,
}

pub struct GraphConstantsNormalCalc {
    pub pipeline_compute_terrain: &'static str,
    pub pipeline_compute_terrain_info: FrameGraphComputeInfo<'static>,
    pub pipeline_compute_standalone: &'static str,
    pub pipeline_compute_standalone_info: FrameGraphComputeInfo<'static>,
    pub pass_normal_calc: &'static str,
}

pub struct GraphConstantsVoxel {
    pub buffer_terrain_acceleration_data: &'static str,
    pub buffer_model_info_data: &'static str,
    pub buffer_model_voxel_data: &'static str,
}

pub struct GraphConstantsRT {
    pub image_size: &'static str,
    pub image_albedo: &'static str,
    pub image_depth: &'static str,

    pub pipeline_compute_prepass: &'static str,
    pub pipeline_compute_prepass_info: FrameGraphComputeInfo<'static>,
    pub pass_prepass: &'static str,

    pub pipeline_compute_gi_sample: &'static str,
    pub pipeline_compute_gi_sample_info: FrameGraphComputeInfo<'static>,
    pub pass_gi_sample: &'static str,
}

pub struct GraphConstantsPostProcess {
    pub pipeline_compute_post_process: &'static str,
    pub pipeline_compute_post_process_info: FrameGraphComputeInfo<'static>,
    pub pass_post_process: &'static str,
}

pub struct GraphConstants {
    pub voxel: GraphConstantsVoxel,
    pub normal_calc: GraphConstantsNormalCalc,
    pub rt: GraphConstantsRT,
    pub editor_ui: GraphConstantsEditorUI,
    pub debug_3d: GraphConstantsDebug3D,
    pub post_process: GraphConstantsPostProcess,

    pub image_backbuffer: &'static str,
    pub image_backbuffer_size: &'static str,
    pub image_preswapchain_composite: &'static str,
    pub image_swapchain: &'static str,
    pub image_swapchain_size: &'static str,

    pub pass_blit_backbuffer_to_swapchain: &'static str,
}

impl Renderer {
    pub const GRAPH: GraphConstants = GraphConstants {
        voxel: GraphConstantsVoxel {
            buffer_terrain_acceleration_data: "rt_buffer_terrain_acceleration_data",
            buffer_model_info_data: "rt_buffer_model_info_data",
            buffer_model_voxel_data: "rt_buffer_model_voxel_data",
        },
        normal_calc: GraphConstantsNormalCalc {
            pipeline_compute_terrain: "normal_calc_terrain_compute",
            pipeline_compute_terrain_info: FrameGraphComputeInfo {
                shader_path: "normal_calc_terrain",
                entry_point_fn: "main",
            },
            pipeline_compute_standalone: "normal_calc_standalone_compute",
            pipeline_compute_standalone_info: FrameGraphComputeInfo {
                shader_path: "normal_calc_standalone",
                entry_point_fn: "main",
            },
            pass_normal_calc: "normal_calc_pass",
        },
        rt: GraphConstantsRT {
            image_size: "rt_image_size",
            image_albedo: "rt_image_albedo",
            image_depth: "rt_image_depth",

            pipeline_compute_prepass: "rt_compute_prepass",
            pipeline_compute_prepass_info: FrameGraphComputeInfo {
                shader_path: "rt_prepass",
                entry_point_fn: "main",
            },
            pass_prepass: "rt_pass_prepass",

            pipeline_compute_gi_sample: "rt_compute_gi_sample",
            pipeline_compute_gi_sample_info: FrameGraphComputeInfo {
                shader_path: "rt_sample",
                entry_point_fn: "main",
            },
            pass_gi_sample: "rt_pass_gi_sample",
        },
        debug_3d: GraphConstantsDebug3D {
            buffer_lines: "debug_3d_buffer_lines",

            pipeline_compute_shapes: "debug_3d_compute_shapes",
            pipeline_compute_shapes_info: FrameGraphComputeInfo {
                shader_path: "debug_shapes",
                entry_point_fn: "main",
            },
            pass_debug: "debug_3d_pass_debug",
        },
        post_process: GraphConstantsPostProcess {
            pipeline_compute_post_process: "post_process_compute_post_process",
            pipeline_compute_post_process_info: FrameGraphComputeInfo {
                shader_path: "post_process",
                entry_point_fn: "main",
            },
            pass_post_process: "post_process_pass_post_process",
        },
        editor_ui: GraphConstantsEditorUI {
            buffer_vertex_buffer: "editor_ui_buffer_vertex_buffer",
            buffer_index_buffer: "editor_ui_buffer_index_buffer",

            pipeline_raster_ui: "editor_ui_pipeline_raster_ui",
            pipeline_raster_ui_info: FrameGraphRasterInfo {
                vertex_shader_path: "egui",
                vertex_entry_point_fn: "main_vs",
                fragment_shader_path: "egui",
                fragment_entry_point_fn: "main_fs",
                vertex_format: FrameGraphVertexFormat {
                    attributes: &[
                        GfxVertexAttribute {
                            location: 0,
                            format: GfxVertexAttributeFormat::Float2,
                        },
                        GfxVertexAttribute {
                            location: 1,
                            format: GfxVertexAttributeFormat::Float2,
                        },
                        GfxVertexAttribute {
                            location: 2,
                            format: GfxVertexAttributeFormat::Uint,
                        },
                    ],
                },
                blend_state: FrameGraphRasterBlendInfo {
                    attachments: &[GfxRasterPipelineBlendStateAttachmentInfo {
                        enable_blend: true,
                        // Egui uses premultiplied alpha colors which is why we use `One`.
                        src_color_blend_factor: GfxBlendFactor::One,
                        dst_color_blend_factor: GfxBlendFactor::OneMinusSrcAlpha,
                        color_blend_op: GfxBlendOp::Add,
                        src_alpha_blend_factor: GfxBlendFactor::One,
                        dst_alpha_blend_factor: GfxBlendFactor::Zero,
                        alpha_blend_op: GfxBlendOp::Add,
                    }],
                },
                cull_mode: GfxCullMode::None,
                front_face: GfxFrontFace::Clockwise,
            },
            pass_ui: "editor_ui_pass_ui",
        },

        // The render image before any post processing, ui, or overlays.
        image_backbuffer: "image_backbuffer",
        image_backbuffer_size: "image_backbuffer_size",
        image_swapchain: "image_swapchain",
        image_swapchain_size: "image_swapchain_size",
        image_preswapchain_composite: "image_preswapchain_composite",

        pass_blit_backbuffer_to_swapchain: "pass_blit_backbuffer_to_swapchain",
    };

    pub const SET_CACHE_SLOT_FRAME: u32 = 0;

    pub fn new(device: &mut DeviceResource) -> Self {
        let frame_graph_executor = device.create_frame_graph_executor();

        Self {
            frame_graph_executor,
            frame_graph: None,
            acquired_swapchain: false,
            swapchain_size: Vector2::zeros(),
        }
    }

    fn construct_frame_graph(gfx_settings: &GraphicsSettings) -> FrameGraph {
        let mut builder = FrameGraph::builder();

        // General frame resources.

        let swapchain_image = builder.create_input_image(Self::GRAPH.image_swapchain);
        let swapchain_size_input = builder.create_input(Self::GRAPH.image_swapchain_size);

        // Normal calc
        {
            builder.create_compute_pipeline(
                Self::GRAPH.normal_calc.pipeline_compute_terrain,
                Self::GRAPH.normal_calc.pipeline_compute_terrain_info,
            );
            builder.create_compute_pipeline(
                Self::GRAPH.normal_calc.pipeline_compute_standalone,
                Self::GRAPH.normal_calc.pipeline_compute_standalone_info,
            );
            builder.create_input_pass(Self::GRAPH.normal_calc.pass_normal_calc, &[], &[]);
        }

        let rt_size_input = builder.create_input(Self::GRAPH.rt.image_size);
        let rt_albedo_image = builder
            .create_frame_image_with_ctx(Self::GRAPH.rt.image_albedo, move |ctx| {
                FrameGraphImageInfo::new_rgba32float(ctx.get_vec2(rt_size_input))
            });

        {
            let rt_depth_image = builder
                .create_frame_image_with_ctx(Self::GRAPH.rt.image_depth, move |ctx| {
                    FrameGraphImageInfo::new_r16float(ctx.get_vec2(rt_size_input))
                });

            builder.create_compute_pipeline(
                Self::GRAPH.rt.pipeline_compute_prepass,
                Self::GRAPH.rt.pipeline_compute_prepass_info,
            );
            builder.create_input_pass(
                Self::GRAPH.rt.pass_prepass,
                &[],
                &[&Self::GRAPH.rt.image_albedo, &Self::GRAPH.rt.image_depth],
            );
        }

        let backbuffer_size_input = builder.create_input(Self::GRAPH.image_backbuffer_size);
        let backbuffer_image = builder.create_frame_image_with_ctx(
            Self::GRAPH.image_backbuffer,
            move |ctx: &FrameGraphContext| {
                FrameGraphImageInfo::new_rgba8(ctx.get_vec2(backbuffer_size_input))
            },
        );

        // Post process, blit to content backbuffer.
        {
            let post_process_compute_pipline = builder.create_compute_pipeline(
                Self::GRAPH.post_process.pipeline_compute_post_process,
                Self::GRAPH.post_process.pipeline_compute_post_process_info,
            );

            builder.create_pass(
                Self::GRAPH.post_process.pass_post_process,
                &[&Self::GRAPH.rt.image_albedo],
                &[&Self::GRAPH.image_backbuffer],
                move |recorder, ctx| {
                    let rt_image = ctx.get_image(Self::GRAPH.rt.image_albedo);
                    let backbuffer_image = ctx.get_image(Self::GRAPH.image_backbuffer);
                    let backbuffer_image_size =
                        recorder.get_image_info(&backbuffer_image).resolution_xy();

                    {
                        let compute_pipeline =
                            ctx.get_compute_pipeline(post_process_compute_pipline);

                        let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);
                        compute_pass.bind_uniforms(&mut |writer| {
                            writer.use_set_cache("u_frame", Self::SET_CACHE_SLOT_FRAME);
                            writer.write_binding("u_shader.rt_final", rt_image);
                            writer.write_binding("u_shader.backbuffer", backbuffer_image);
                        });

                        let wg_size = compute_pass.workgroup_size();
                        compute_pass.dispatch(
                            (backbuffer_image_size.x as f32 / wg_size.x as f32).ceil() as u32,
                            (backbuffer_image_size.y as f32 / wg_size.y as f32).ceil() as u32,
                            1,
                        );
                    }
                },
            );
        }

        // Draw debug gizmos and shapes on game content backbuffer.
        {
            builder.create_frame_buffer(Self::GRAPH.debug_3d.buffer_lines);

            builder.create_compute_pipeline(
                &Self::GRAPH.debug_3d.pipeline_compute_shapes,
                Self::GRAPH.debug_3d.pipeline_compute_shapes_info,
            );
            builder.create_input_pass(
                Self::GRAPH.debug_3d.pass_debug,
                &[
                    &Self::GRAPH.image_backbuffer,
                    &Self::GRAPH.rt.image_depth,
                    &Self::GRAPH.debug_3d.buffer_lines,
                ],
                &[&Self::GRAPH.image_backbuffer],
            );
        }

        let preswapchain_image = builder.create_frame_image_with_ctx(
            Self::GRAPH.image_preswapchain_composite,
            move |ctx: &FrameGraphContext| {
                FrameGraphImageInfo::new_rgba8(ctx.get_vec2(swapchain_size_input))
            },
        );

        // Overlay editor ui and composite game content onto pre-swapchain buffer.
        {
            builder.create_frame_buffer(Self::GRAPH.editor_ui.buffer_vertex_buffer);
            builder.create_frame_buffer(Self::GRAPH.editor_ui.buffer_index_buffer);

            builder.create_raster_pipeline(
                Self::GRAPH.editor_ui.pipeline_raster_ui,
                Self::GRAPH.editor_ui.pipeline_raster_ui_info,
                &[&Self::GRAPH.image_backbuffer],
                None,
            );

            builder.create_input_pass(
                Self::GRAPH.editor_ui.pass_ui,
                &[
                    &Self::GRAPH.image_backbuffer,
                    &Self::GRAPH.editor_ui.pipeline_raster_ui,
                    &Self::GRAPH.editor_ui.buffer_vertex_buffer,
                    &Self::GRAPH.editor_ui.buffer_index_buffer,
                ],
                &[&Self::GRAPH.image_preswapchain_composite],
            );
        }

        // Preswapchain composite to swapchain blit.
        builder.create_pass(
            Self::GRAPH.pass_blit_backbuffer_to_swapchain,
            &[&Self::GRAPH.image_preswapchain_composite],
            &[&Self::GRAPH.image_swapchain],
            |recorder, ctx| {
                let preswapchain_image = ctx.get_image(Self::GRAPH.image_preswapchain_composite);
                let swapchain_image = ctx.get_image(Self::GRAPH.image_swapchain);
                recorder.blit_full(preswapchain_image, swapchain_image, GfxFilterMode::Nearest);
            },
        );

        // Present.
        builder.present_image(swapchain_image);

        builder.bake().unwrap()
    }

    pub fn begin_frame(
        mut renderer: ResMut<Renderer>,
        device: ResMut<DeviceResource>,
        settings: Res<Settings>,
    ) {
        let renderer: &mut Renderer = &mut renderer;

        let frame_graph = renderer
            .frame_graph
            .take()
            .unwrap_or_else(|| Self::construct_frame_graph(&settings.graphics));
        renderer.frame_graph_executor.begin_frame(frame_graph);
        renderer.acquired_swapchain = false;
    }

    pub fn write_common_render_data(
        mut renderer: ResMut<Renderer>,
        time: Res<Time>,
        voxel_world_gpu: Res<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        main_camera: Res<MainCamera>,
        ui: Res<UI>,
    ) {
        assert!(renderer.acquired_swapchain);
        let content_size = ui
            .content_size(Vector2::new(
                renderer.swapchain_size.x as f32,
                renderer.swapchain_size.y as f32,
            ))
            .map(|x| x.max(1.0));
        let backbuffer_aspect_ratio = content_size.x / content_size.y;
        let content_size = content_size.map(|x| x as u32);
        renderer
            .frame_graph_executor
            .supply_input(Self::GRAPH.image_backbuffer_size, Box::new(content_size));
        renderer
            .frame_graph_executor
            .supply_input(Self::GRAPH.rt.image_size, Box::new(content_size));

        renderer
            .frame_graph_executor
            .write_uniforms(&mut |writer, ctx| {
                writer.write_set_cache("u_frame", Self::SET_CACHE_SLOT_FRAME);
                writer.write_uniform(
                    "u_frame.frame_info.time_ms",
                    time.start_time().elapsed().as_millis() as u32,
                );

                if let Some(mut camera_query) = ecs_world.try_get_main_camera(&main_camera) {
                    let (camera_transform, camera) = camera_query.get().unwrap();

                    let proj_view_matrix = camera.projection_matrix(backbuffer_aspect_ratio)
                        * camera_transform.to_view_matrix();
                    writer.write_uniform_mat4(
                        "u_frame.world_info.camera.proj_view",
                        &proj_view_matrix,
                    );

                    let transformation_matrix = camera_transform.to_transformation_matrix();
                    writer.write_uniform_mat4(
                        "u_frame.world_info.camera.transform",
                        &transformation_matrix,
                    );

                    let rot_matrix_3x3 = transformation_matrix.fixed_resize::<3, 3>(0.0);
                    writer
                        .write_uniform_mat3("u_frame.world_info.camera.rotation", &rot_matrix_3x3);
                    writer.write_uniform("u_frame.world_info.camera.fov", camera.fov());
                    writer
                        .write_uniform("u_frame.world_info.camera.near_plane", camera.near_plane());
                    writer.write_uniform("u_frame.world_info.camera.far_plane", camera.far_plane());
                } else {
                    log::error!("Main camera doesn't exist.");
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
                    writer.write_uniform("u_frame.world_info.camera.fov", 0.0);
                    writer.write_uniform("u_frame.world_info.camera.near_plane", 0.0);
                    writer.write_uniform("u_frame.world_info.camera.far_plane", 0.0);
                }

                writer.write_binding(
                    "u_frame.voxel.entity_data.accel_buf",
                    *voxel_world_gpu.world_entity_acceleration_buffer(),
                );
                writer.write_uniform(
                    "u_frame.voxel.entity_data.entity_count",
                    voxel_world_gpu.rendered_voxel_model_entity_count(),
                );

                writer.write_binding(
                    "u_frame.voxel.model_info_data",
                    *voxel_world_gpu.world_voxel_model_info_buffer(),
                );
                writer.write_binding(
                    "u_frame.voxel.model_voxel_data",
                    *voxel_world_gpu.world_data_buffer().unwrap(),
                );
                writer.write_binding(
                    "u_frame.voxel.rw_model_voxel_data",
                    *voxel_world_gpu.world_data_buffer().unwrap(),
                );

                writer.write_binding(
                    "u_frame.voxel.terrain.data",
                    *voxel_world_gpu.world_terrain_acceleration_buffer(),
                );
                writer.write_uniform(
                    "u_frame.voxel.terrain.side_length",
                    voxel_world_gpu.terrain_side_length(),
                );
                writer.write_uniform(
                    "u_frame.voxel.terrain.volume",
                    voxel_world_gpu.terrain_side_length().pow(3),
                );
                writer.write_uniform(
                    "u_frame.voxel.terrain.anchor",
                    voxel_world_gpu.renderable_chunks.terrain_anchor,
                );
                writer.write_uniform(
                    "u_frame.voxel.terrain.window_offset",
                    voxel_world_gpu.renderable_chunks.terrain_window_offset,
                );
            });
    }

    pub fn acquire_swapchain_image(
        mut renderer: ResMut<Renderer>,
        mut device: ResMut<DeviceResource>,
        window: Res<Window>,
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

        // Write swapchain related inputs.
        renderer.acquired_swapchain = true;
        renderer.swapchain_size = swapchain_image_info.resolution_xy();
        renderer
            .frame_graph_executor
            .supply_image_ref(Self::GRAPH.image_swapchain, &swapchain_image);
        renderer.frame_graph_executor.supply_input(
            Self::GRAPH.image_swapchain_size,
            Box::new(swapchain_image_info.resolution_xy()),
        );
    }

    pub fn finish_frame(
        mut renderer: ResMut<Renderer>,
        mut device: ResMut<DeviceResource>,
        mut ui: ResMut<UIPass>,
        time: Res<Time>,
    ) {
        // -------- RT Pass ---------------

        // Supply pass logic.
        renderer.frame_graph_executor.supply_pass_ref(
            Self::GRAPH.rt.pass_prepass,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext| {
                let rt_image = ctx.get_image(Self::GRAPH.rt.image_albedo);
                let rt_image_depth = ctx.get_image(Self::GRAPH.rt.image_depth);
                let rt_image_size = recorder.get_image_info(&rt_image).resolution_xy();
                let rt_image_depth_size = recorder.get_image_info(&rt_image_depth).resolution_xy();
                assert_eq!(
                    rt_image_size, rt_image_depth_size,
                    "RT image sizes should be matching."
                );

                let compute_pipeline =
                    ctx.get_compute_pipeline(Self::GRAPH.rt.pipeline_compute_prepass);

                {
                    let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);
                    compute_pass.bind_uniforms(&mut |writer| {
                        writer.use_set_cache("u_frame", Self::SET_CACHE_SLOT_FRAME);
                        writer.write_binding("u_shader.backbuffer", rt_image);
                        writer.write_binding("u_shader.backbuffer_depth", rt_image_depth);
                    });
                    let wg_size = compute_pass.workgroup_size();
                    compute_pass.dispatch(
                        (rt_image_size.x as f32 / wg_size.x as f32).ceil() as u32,
                        (rt_image_size.y as f32 / wg_size.y as f32).ceil() as u32,
                        1,
                    );
                }
            },
        );

        renderer.frame_graph = Some(renderer.frame_graph_executor.end_frame());
    }

    pub fn did_acquire_swapchain(&self) -> bool {
        self.acquired_swapchain
    }
}
