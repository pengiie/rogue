use nalgebra::Vector2;
use rogue_engine::common::color::Color;
use rogue_engine::egui::egui_gpu::EguiGpu;
use rogue_engine::graphics::backend::{GfxBlitInfo, GfxFilterMode};
use rogue_engine::graphics::device::DeviceResource;
use rogue_engine::graphics::frame_graph::FrameGraphImageInfo;
use rogue_engine::graphics::{frame_graph::FrameGraphBuilder, renderer::Renderer};
use rogue_engine::resource::{Res, ResMut};
use rogue_engine::world::world_renderable::WorldRenderable;

use crate::ui::EditorUI;

pub struct EditorRenderGraphConstants {
    pub backbuffer_name: &'static str,
    pub backbuffer_size_input: &'static str,
    pub backbuffer_depth_name: &'static str,
    pub backbuffer_blit_offset_input: &'static str,
}

pub struct EditorRenderGraph {}

impl EditorRenderGraph {
    const GRAPH: EditorRenderGraphConstants = EditorRenderGraphConstants {
        backbuffer_name: "editor_backbuffer",
        backbuffer_size_input: "editor_backbuffer_size",
        backbuffer_depth_name: "editor_backbuffer_depth",
        backbuffer_blit_offset_input: "editor_backbuffer_blit_offset",
    };

    /// Supplies inputs such as backbuffer size or backbuffer blit offset, etc. to the
    /// render graph executor.
    pub fn write_general_inputs(mut renderer: ResMut<Renderer>, mut editor_ui: Res<EditorUI>) {
        let swapchain_size = renderer.swapchain_size();
        let pad = editor_ui.content_padding();
        let backbuffer_size = Vector2::new(
            swapchain_size.x.saturating_sub(pad.z + pad.w),
            swapchain_size.y.saturating_sub(pad.x + pad.y),
        );
        renderer
            .executor()
            .supply_input(Self::GRAPH.backbuffer_size_input, Box::new(backbuffer_size));

        renderer
            .executor()
            .supply_input(Self::GRAPH.backbuffer_blit_offset_input, Box::new(pad.zy()));
    }

    pub fn init_render_graph(
        mut renderer: ResMut<Renderer>,
        mut egui_gpu: ResMut<EguiGpu>,
        mut world_gpu: ResMut<WorldRenderable>,
    ) {
        let mut fg = FrameGraphBuilder::new();

        let backbuffer_size_input =
            fg.create_input::<Vector2<u32>>(Self::GRAPH.backbuffer_size_input);
        let backbuffer = fg.create_frame_image_with_ctx(Self::GRAPH.backbuffer_name, move |ctx| {
            FrameGraphImageInfo::new_rgba32float(ctx.get_vec2(backbuffer_size_input))
        });
        let backbuffer_depth = fg
            .create_frame_image_with_ctx(Self::GRAPH.backbuffer_depth_name, move |ctx| {
                FrameGraphImageInfo::new_r16float(ctx.get_vec2(backbuffer_size_input))
            });

        // World render pass, draws the terrain and entities.
        world_gpu.set_graph_render_pass(&mut fg, backbuffer, backbuffer_depth);

        let swapchain_image = fg.create_input_image(Renderer::GRAPH.image_swapchain);
        let swapchain_image_size =
            fg.create_input::<Vector2<u32>>(Renderer::GRAPH.image_swapchain_size);

        // Clear the swapchaain image and blit the backbuffer to it.
        let blit_offset_input =
            fg.create_input::<Vector2<u32>>(Self::GRAPH.backbuffer_blit_offset_input);
        let swapchain_prepare_pass = fg.create_pass(
            "swapchain_prepare_pass",
            &[&swapchain_image],
            &[&swapchain_image],
            move |recorder, ctx| {
                let swapchain_image = ctx.get_image(&swapchain_image);
                recorder.clear_color(swapchain_image, Color::new_srgb(0.0, 0.0, 0.0));

                let backbuffer_image = ctx.get_image(&backbuffer);
                let backbuffer_size = recorder.get_image_info(&backbuffer_image).resolution_xy();
                let blit_offset = ctx.get_vec2(&blit_offset_input);
                recorder.blit(GfxBlitInfo {
                    src: backbuffer_image,
                    src_offset: Vector2::new(0, 0),
                    src_length: backbuffer_size,
                    dst: swapchain_image,
                    dst_offset: blit_offset,
                    dst_length: backbuffer_size,
                    filter: GfxFilterMode::Nearest,
                });
            },
        );

        // Egui pass, draws the editor UI.
        // TODO: Pass dependencies so its not just linear.
        egui_gpu.set_graph_egui_pass(&mut fg, swapchain_image, &[]);

        fg.present_image(swapchain_image);

        renderer.set_frame_graph(fg.bake().expect("Frame graph has an error oops"));
    }
}
