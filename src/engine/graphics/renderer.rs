use std::collections::HashMap;

use log::warn;
use nalgebra::{Vector2, Vector3};
use rogue_macros::Resource;
use serde::{Deserialize, Serialize};

use crate::{
    common::color::Color,
    engine::{
        resource::{Res, ResMut},
        window::window::Window,
    },
    settings::{GraphicsSettings, Settings},
};

use super::{
    backend::{GfxFilterMode, GraphicsBackendFrameGraphExecutor, Image, UniformData},
    device::DeviceResource,
    frame_graph::{FrameGraph, FrameGraphComputeInfo, FrameGraphImageInfo},
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

    compute_prepass: &'static str,
    compute_prepass_info: FrameGraphComputeInfo<'static>,
    pass_prepass: &'static str,
}

struct GraphConstants {
    rt: GraphConstantsRT,

    swapchain: &'static str,
}

impl Renderer {
    pub const GRAPH: GraphConstants = GraphConstants {
        rt: GraphConstantsRT {
            image_albedo: "rt_image_albedo",
            image_depth: "rt_image_depth",
            compute_prepass: "rt_compute_prepass",
            compute_prepass_info: FrameGraphComputeInfo {
                shader_path: "terrain_prepass",
                entry_point_fn: "main",
            },
            pass_prepass: "rt_pass_prepass",
        },
        swapchain: "swapchain",
    };

    pub fn new(device: &mut DeviceResource) -> Self {
        let frame_graph_executor = device.create_frame_graph_executor();

        Self {
            frame_graph_executor,
            frame_graph: None,
        }
    }

    // fn construct_frame_graph(&mut self) -> FrameGraph {
    //     let mut builder = FrameGraph::builder();

    //     let rt_resolution = builder.create_input::<Vector2<u32>>(Self::GRAPH_RT_RESOLUTION_NAME);

    //     let swapchain_image = builder.create_input_image(Self::GRAPH_SWAPCHAIN_NAME);

    //     let frame_info_buffer = builder.create_frame_buffer(Self::GRAPH_FRAME_INFO_NAME);

    //     let rt_albedo_image = builder.create_frame_image(Self::GRAPH_RT_TARGET_ALBEDO_NAME);
    //     let rt_depth_image = builder.create_frame_image(Self::GRAPH_RT_TARGET_DEPTH_NAME);

    //     // Terrain Prepass
    //     {
    //         let compute_pipeline_resource = builder.create_compute_pipeline(
    //             Self::GRAPH_TERRAIN_PREPASS_NAME,
    //             Self::GRAPH_TERRAIN_PREPASS_COMPUTE_INFO,
    //         );
    //         builder.create_pass(
    //             Self::GRAPH_TERRAIN_PREPASS_NAME,
    //             &[&rt_albedo_image, &frame_info_buffer],
    //             &[&rt_albedo_image],
    //             move |recorder, ctx| {
    //                 let rt_resolution = ctx.get_vec2::<u32>(Self::GRAPH_RT_RESOLUTION_NAME);
    //                 let rt_albedo = ctx.get_image(Self::GRAPH_RT_TARGET_ALBEDO_NAME);
    //                 let rt_depth = ctx.get_image(Self::GRAPH_RT_TARGET_DEPTH_NAME);
    //                 //assert_eq!(rt_resolution, rt_albedo.info(recorder).resolution_xy());

    //                 let compute_pipeline = ctx.get_compute_pipeline(&compute_pipeline_resource);
    //                 //let workgroup_size = compute_pipeline.info(recorderer).workgroup_size();
    //                 let workgroup_size = Vector3::new(8, 8, 0);

    //                 let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);
    //                 compute_pass.bind_uniforms({
    //                     let mut map = HashMap::new();
    //                     map.insert("u_rt_albedo", rt_albedo.as_binding());
    //                     map.insert("u_rt_depth", rt_depth.as_binding());
    //                     map
    //                 });

    //                 compute_pass.dispatch(
    //                     (rt_resolution.x as f32 / workgroup_size.x as f32).ceil() as u32,
    //                     (rt_resolution.y as f32 / workgroup_size.y as f32).ceil() as u32,
    //                     0,
    //                 );
    //             },
    //         );
    //     }

    //     builder.create_pass(
    //         "blit",
    //         &[&rt_albedo_image],
    //         &[&swapchain_image],
    //         |recorder, ctx| {
    //             let rt_image = ctx.get_image(Self::GRAPH_RT_TARGET_ALBEDO_NAME);
    //             let swapchain_image = ctx.get_image(Self::GRAPH_SWAPCHAIN_NAME);

    //             recorder.blit(rt_image, swapchain_image);
    //         },
    //     );

    //     builder.present_image(swapchain_image);
    //     builder.bake().unwrap()
    // }

    fn construct_fallback_frame_graph(gfx_settings: &GraphicsSettings) -> FrameGraph {
        let mut builder = FrameGraph::builder();
        let graph_swapchain_image = builder.create_input_image(Self::GRAPH.swapchain);

        let rt_albedo_image = builder.create_frame_image(
            Self::GRAPH.rt.image_albedo,
            FrameGraphImageInfo::new_rgba32float(gfx_settings.rt_size),
        );
        let rt_depth_image = builder.create_frame_image(
            Self::GRAPH.rt.image_depth,
            FrameGraphImageInfo::new_depth(gfx_settings.rt_size),
        );

        // Compute n' blit.
        let pass_compute_pipeline = builder.create_compute_pipeline(
            Self::GRAPH.rt.compute_prepass,
            Self::GRAPH.rt.compute_prepass_info,
        );
        let pass_rt_size = gfx_settings.rt_size;
        builder.create_pass(
            "blit_n_clear_color",
            &[&graph_swapchain_image, &rt_albedo_image],
            &[&graph_swapchain_image],
            move |recorder, ctx| {
                let rt_image = ctx.get_image(Self::GRAPH.rt.image_albedo);
                let compute_pipeline = ctx.get_compute_pipeline(pass_compute_pipeline);

                {
                    let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);
                    compute_pass.bind_uniforms({
                        let mut uniforms = UniformData::new();
                        uniforms.load("u_shader.backbuffer", rt_image.as_storage_binding());
                        uniforms
                    });
                    let wg_size = compute_pass.workgroup_size();
                    compute_pass.dispatch(
                        (pass_rt_size.x as f32 / wg_size.x as f32).ceil() as u32,
                        (pass_rt_size.x as f32 / wg_size.x as f32).ceil() as u32,
                        1,
                    );
                }

                let swapchain_image = ctx.get_image(Self::GRAPH.swapchain);
                recorder.blit(rt_image, swapchain_image, GfxFilterMode::Nearest);
            },
        );

        builder.create_input_pass(
            Self::GRAPH.rt.pass_prepass,
            &[],
            &[&Self::GRAPH.rt.image_albedo],
        );

        // Present.
        builder.present_image(graph_swapchain_image);

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
            .unwrap_or_else(|| Self::construct_fallback_frame_graph(&settings.graphics));
        renderer
            .frame_graph_executor
            .begin_frame(&mut shader_compiler, frame_graph);
    }

    pub fn finish_frame(
        mut renderer: ResMut<Renderer>,
        mut device: ResMut<DeviceResource>,
        window: Res<Window>,
    ) {
        let swapchain_image = match device.acquire_swapchain_image() {
            Ok(image) => image,
            Err(err) => {
                let inner_size = window.inner_size();
                warn!("Tried to acquire swapchain error but got an error, trying to resize swapchain to {}x{}.", inner_size.width, inner_size.height);
                device.resize_swapchain(inner_size);
                return;
            }
        };
        renderer
            .frame_graph_executor
            .supply_image_ref(Self::GRAPH.swapchain, &swapchain_image);

        renderer.frame_graph = Some(renderer.frame_graph_executor.end_frame());
    }
}
