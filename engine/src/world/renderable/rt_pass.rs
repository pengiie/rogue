use rogue_macros::Resource;

use crate::{
    graphics::{
        backend::{GraphicsBackendRecorder, Image},
        frame_graph::{
            FrameGraphBuilder, FrameGraphComputeInfo, FrameGraphContext, FrameGraphResource,
            IntoFrameGraphResource, Pass,
        },
        renderer::Renderer,
    },
    resource::{Res, ResMut},
};

struct WorldRTPassGraphConstants {
    rt_pass_name: &'static str,
    rt_compute_pipeline_name: &'static str,
    rt_compute_pipeline_info: FrameGraphComputeInfo<'static>,
}

#[derive(Clone, Copy, strum_macros::VariantArray, strum_macros::Display, PartialEq, Eq)]
#[repr(u32)]
pub enum ShadingMode {
    Ambient = 0,
    Face = 1,
    Lambert = 2,
}

#[derive(Resource)]
pub struct WorldRTPass {
    pub shading_mode: ShadingMode,
    graph_framebuffer: Option<FrameGraphResource<Image>>,
    graph_framebuffer_depth: Option<FrameGraphResource<Image>>,
}

impl WorldRTPass {
    const GRAPH: WorldRTPassGraphConstants = WorldRTPassGraphConstants {
        rt_pass_name: "world_render_pass",
        rt_compute_pipeline_name: "world_render_compute_pipeline",
        rt_compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "rt_prepass",
            entry_point_fn: "main",
        },
    };

    pub fn new() -> Self {
        Self {
            shading_mode: ShadingMode::Lambert,
            graph_framebuffer: None,
            graph_framebuffer_depth: None,
        }
    }

    /// Adds the rt pass for rendering the world (terrain and entities).
    pub fn set_graph_rt_pass(
        &mut self,
        fg: &mut FrameGraphBuilder,
        framebuffer: impl IntoFrameGraphResource<Image>,
        framebuffer_depth: impl IntoFrameGraphResource<Image>,
    ) -> FrameGraphResource<Pass> {
        let compute_pipeline = fg.create_compute_pipeline(
            Self::GRAPH.rt_compute_pipeline_name,
            Self::GRAPH.rt_compute_pipeline_info,
        );

        let framebuffer_handle = framebuffer.handle(fg);
        let framebuffer_depth_handle = framebuffer_depth.handle(fg);
        let pass = fg.create_input_pass(
            Self::GRAPH.rt_pass_name,
            &[
                &framebuffer_handle,
                &framebuffer_depth_handle,
                &compute_pipeline,
            ],
            &[&framebuffer_handle, &framebuffer_depth_handle],
        );

        self.graph_framebuffer = Some(framebuffer_handle);
        self.graph_framebuffer_depth = Some(framebuffer_depth_handle);

        pass
    }

    pub fn write_graph_rt_pass(rt_pass: Res<WorldRTPass>, mut renderer: ResMut<Renderer>) {
        let framebuffer_image_handle = rt_pass.graph_framebuffer.as_ref().expect(
            "Should not be writing egui pass without setting it up in the render graph first.",
        );
        let framebuffer_depth_handle = rt_pass.graph_framebuffer_depth.as_ref().expect(
            "Should not be writing egui pass without setting it up in the render graph first.",
        );
        let shading_mode = rt_pass.shading_mode;
        renderer.frame_graph_executor.supply_pass_ref(
            Self::GRAPH.rt_pass_name,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                let framebuffer_image = ctx.get_image(framebuffer_image_handle);
                let framebuffer_image_info = recorder.get_image_info(&framebuffer_image);
                let framebuffer_size = framebuffer_image_info.resolution_xy();

                let framebuffer_depth = ctx.get_image(framebuffer_depth_handle);

                let pipeline = ctx.get_compute_pipeline(Self::GRAPH.rt_compute_pipeline_name);
                let mut compute_pass = recorder.begin_compute_pass(pipeline);
                let wg_size = compute_pass.workgroup_size();

                compute_pass.bind_uniforms(&mut |writer| {
                    writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                    writer.write_binding("u_shader.backbuffer", framebuffer_image);
                    writer.write_binding("u_shader.backbuffer_depth", framebuffer_depth);
                    writer.write_uniform::<u32>("u_shader.shading_mode", shading_mode as u32);
                });

                compute_pass.dispatch(
                    (framebuffer_size.x as f32 / wg_size.x as f32).ceil() as u32,
                    (framebuffer_size.y as f32 / wg_size.y as f32).ceil() as u32,
                    1,
                );
            },
        );
    }
}
