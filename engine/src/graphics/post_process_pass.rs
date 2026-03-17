use nalgebra::Vector2;

use crate::{
    common::color::Color,
    graphics::{
        backend::Image,
        frame_graph::{
            FrameGraphBuilder, FrameGraphComputeInfo, FrameGraphResource, IntoFrameGraphResource,
            Pass,
        },
    },
};

struct PostProcessPassGraphConstants {
    pass_name: &'static str,
    compute_pipeline_name: &'static str,
    compute_pipeline_info: FrameGraphComputeInfo<'static>,
}

pub struct PostProcessPass {}

impl PostProcessPass {
    const GRAPH: PostProcessPassGraphConstants = PostProcessPassGraphConstants {
        pass_name: "post_process_pass",
        compute_pipeline_name: "post_process_compute_pipeline",
        compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "post_process",
            entry_point_fn: "main",
        },
    };

    /// Adds the post process pass which also handles blitting to the swapchain.
    pub fn set_graph_post_process_blit_pass(
        fg: &mut FrameGraphBuilder,
        blit_offset_input: impl IntoFrameGraphResource<Vector2<u32>>,
        framebuffer: impl IntoFrameGraphResource<Image>,
        swapchain: impl IntoFrameGraphResource<Image>,
    ) -> FrameGraphResource<Pass> {
        let compute_pipeline = fg.create_compute_pipeline(
            Self::GRAPH.compute_pipeline_name,
            Self::GRAPH.compute_pipeline_info,
        );

        let framebuffer_handle = framebuffer.handle(fg);
        let swapchain_handle = swapchain.handle(fg);
        let blit_offset_input_handle = blit_offset_input.handle(fg);
        let pass = fg.create_pass(
            Self::GRAPH.pass_name,
            &[&framebuffer_handle, &swapchain_handle, &compute_pipeline],
            &[&swapchain_handle],
            move |recorder, ctx| {
                let framebuffer = ctx.get_image(framebuffer_handle);
                let framebuffer_size = recorder.get_image_info(&framebuffer).resolution_xy();
                let swapchain = ctx.get_image(swapchain_handle);
                recorder.clear_color(swapchain, Color::new_srgb(0.0, 0.0, 0.0));

                let pipeline = ctx.get_compute_pipeline(&compute_pipeline);
                let mut compute_pass = recorder.begin_compute_pass(pipeline);

                let blit_offset = ctx.get_vec2(blit_offset_input_handle);
                let wg_size = compute_pass.workgroup_size();
                compute_pass.bind_uniforms(&mut |writer| {
                    writer.write_binding("u_shader.rt_final", framebuffer);
                    writer.write_binding("u_shader.backbuffer", swapchain);
                    writer.write_uniform::<Vector2<u32>>("u_shader.blit_offset", blit_offset);
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
