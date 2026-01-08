use rogue_engine::graphics::{frame_graph::FrameGraphBuilder, renderer::Renderer};
use rogue_engine::resource::ResMut;

pub fn init_render_graph(mut renderer: ResMut<Renderer>) {
    let fg = FrameGraphBuilder::new();

    renderer.set_frame_graph(fg.bake().expect("Frame graph has an error oops"));
}
