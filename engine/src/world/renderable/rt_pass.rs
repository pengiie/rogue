use rogue_macros::Resource;

use crate::graphics::{
    backend::Image,
    frame_graph::{
        FrameGraphBuilder, FrameGraphComputeInfo, FrameGraphResource, IntoFrameGraphResource, Pass,
    },
    renderer::Renderer,
};

struct WorldRTPassGraphConstants {
    rt_pass_name: &'static str,
    rt_compute_pipeline_name: &'static str,
    rt_compute_pipeline_info: FrameGraphComputeInfo<'static>,
}

#[derive(Resource)]
pub struct WorldRTPass {}

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
        Self {}
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
        let pass = fg.create_pass(
            Self::GRAPH.rt_pass_name,
            &[
                &framebuffer_handle,
                &framebuffer_depth_handle,
                &compute_pipeline,
            ],
            &[&framebuffer_handle, &framebuffer_depth_handle],
            move |recorder, ctx| {
                let framebuffer = ctx.get_image(framebuffer_handle);
                let framebuffer_depth = ctx.get_image(framebuffer_depth_handle);
                let framebuffer_size = recorder.get_image_info(&framebuffer).resolution_xy();

                let pipeline = ctx.get_compute_pipeline(&compute_pipeline);
                let mut compute_pass = recorder.begin_compute_pass(pipeline);
                let wg_size = compute_pass.workgroup_size();

                compute_pass.bind_uniforms(&mut |writer| {
                    writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                    writer.write_binding("u_shader.backbuffer", framebuffer);
                    writer.write_binding("u_shader.backbuffer_depth", framebuffer_depth);
                });

                compute_pass.dispatch(
                    (framebuffer_size.x as f32 / wg_size.x as f32).ceil() as u32,
                    (framebuffer_size.y as f32 / wg_size.y as f32).ceil() as u32,
                    1,
                );
            },
        );

        pass
    }
}
