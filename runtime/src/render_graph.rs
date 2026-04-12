use nalgebra::Vector2;
use rogue_engine::common::color::Color;
use rogue_engine::debug::debug_renderer::DebugRenderer;
use rogue_engine::egui::egui_gpu::EguiGpu;
use rogue_engine::graphics::backend::{GfxBlitInfo, GfxFilterMode};
use rogue_engine::graphics::device::DeviceResource;
use rogue_engine::graphics::frame_graph::FrameGraphImageInfo;
use rogue_engine::graphics::passes::post_process_pass::PostProcessPass;
use rogue_engine::graphics::{frame_graph::FrameGraphBuilder, renderer::Renderer};
use rogue_engine::resource::{Res, ResMut};
use rogue_engine::voxel::baker_gpu::VoxelBakerGpu;
use rogue_engine::world::renderable::rt_pass::WorldRTPass;

pub struct RuntimeRenderGraphConstants {
    pub backbuffer_name: &'static str,
    pub backbuffer_size_input: &'static str,
    pub backbuffer_depth_r16_name: &'static str,
    pub backbuffer_depth_name: &'static str,
    pub backbuffer_blit_offset_input: &'static str,

    pub intermediate_image_name: &'static str,
}

pub struct RuntimeRenderGraph {}

impl RuntimeRenderGraph {
    const GRAPH: RuntimeRenderGraphConstants = RuntimeRenderGraphConstants {
        backbuffer_name: "backbuffer",
        backbuffer_size_input: "backbuffer_size",
        backbuffer_depth_r16_name: "backbuffer_r16_depth",
        backbuffer_depth_name: "backbuffer_depth",
        backbuffer_blit_offset_input: "backbuffer_blit_offset",
        intermediate_image_name: "intermediate_image",
    };

    /// Supplies inputs such as backbuffer size or backbuffer blit offset, etc. to the
    /// render graph executor.
    pub fn write_general_inputs(mut renderer: ResMut<Renderer>) {
        let swapchain_size = renderer.swapchain_size();
        renderer
            .executor()
            .supply_input(Self::GRAPH.backbuffer_size_input, Box::new(swapchain_size));

        renderer.executor().supply_input(
            Self::GRAPH.backbuffer_blit_offset_input,
            Box::new(Vector2::new(0u32, 0u32)),
        );
    }

    pub fn init_render_graph(
        mut renderer: ResMut<Renderer>,
        mut world_rt_pass_gpu: ResMut<WorldRTPass>,
        mut voxel_baker_gpu: ResMut<VoxelBakerGpu>,
    ) {
        let mut fg = FrameGraphBuilder::new();

        let backbuffer_size_input =
            fg.create_input::<Vector2<u32>>(Self::GRAPH.backbuffer_size_input);
        let backbuffer = fg.create_frame_image_with_ctx(Self::GRAPH.backbuffer_name, move |ctx| {
            FrameGraphImageInfo::new_rgba32float(ctx.get_vec2(backbuffer_size_input))
        });
        let backbuffer_depth_r16 = fg
            .create_frame_image_with_ctx(Self::GRAPH.backbuffer_depth_r16_name, move |ctx| {
                FrameGraphImageInfo::new_r16float(ctx.get_vec2(backbuffer_size_input))
            });
        let backbuffer_depth = fg
            .create_frame_image_with_ctx(Self::GRAPH.backbuffer_depth_name, move |ctx| {
                FrameGraphImageInfo::new_depth(ctx.get_vec2(backbuffer_size_input))
            });

        // World model material baking pass
        let bake_pass = voxel_baker_gpu.set_graph_bake_pass(&mut fg);

        // World render pass, draws the terrain and entities.
        world_rt_pass_gpu.set_graph_rt_pass(&mut fg, backbuffer, backbuffer_depth_r16);

        fg.create_pass(
            "depth_copy_pass",
            &[&backbuffer_depth_r16],
            &[&backbuffer_depth],
            move |recorder, ctx| {
                let depth_src = ctx.get_image(&backbuffer_depth_r16);
                let depth_dst = ctx.get_image(&backbuffer_depth);
                let depth_src_info = recorder.get_image_info(&depth_src);
                let depth_dst_info = recorder.get_image_info(&depth_dst);
                assert_eq!(
                    depth_src_info.resolution_xy(),
                    depth_dst_info.resolution_xy()
                );
                recorder.blit(GfxBlitInfo {
                    src: depth_src,
                    src_offset: Vector2::new(0, 0),
                    src_length: depth_src_info.resolution_xy(),
                    dst: depth_dst,
                    dst_offset: Vector2::new(0, 0),
                    dst_length: depth_dst_info.resolution_xy(),
                    filter: GfxFilterMode::Nearest,
                });
            },
        );

        let swapchain_image = fg.create_input_image(Renderer::GRAPH.image_swapchain);
        let swapchain_image_size =
            fg.create_input::<Vector2<u32>>(Renderer::GRAPH.image_swapchain_size);

        let intermediate_image = fg
            .create_frame_image_with_ctx(Self::GRAPH.intermediate_image_name, move |ctx| {
                FrameGraphImageInfo::new_rgba8(ctx.get_vec2(swapchain_image_size))
            });
        // Clear the swapchaain image and blit the backbuffer to it while computing
        // post processing effects.
        let blit_offset_input =
            fg.create_input::<Vector2<u32>>(Self::GRAPH.backbuffer_blit_offset_input);
        PostProcessPass::set_graph_post_process_blit_pass(
            &mut fg,
            blit_offset_input,
            backbuffer,
            intermediate_image,
        );

        fg.create_pass(
            "blit_intermediate_to_swapchain_pass",
            &[&intermediate_image, &swapchain_image],
            &[&swapchain_image],
            move |recorder, ctx| {
                let intermediate = ctx.get_image(&intermediate_image);
                let swapchain = ctx.get_image(&swapchain_image);
                let intermediate_info = recorder.get_image_info(&intermediate);
                let swapchain_info = recorder.get_image_info(&swapchain);
                assert_eq!(
                    intermediate_info.resolution_xy(),
                    swapchain_info.resolution_xy()
                );
                recorder.blit(GfxBlitInfo {
                    src: intermediate,
                    src_offset: Vector2::new(0, 0),
                    src_length: intermediate_info.resolution_xy(),
                    dst: swapchain,
                    dst_offset: Vector2::new(0, 0),
                    dst_length: swapchain_info.resolution_xy(),
                    filter: GfxFilterMode::Nearest,
                });
            },
        );

        fg.present_image(swapchain_image);

        renderer.set_frame_graph(
            fg.bake().expect("Frame graph has an error oops"),
            Self::GRAPH.backbuffer_size_input,
        );
    }
}
