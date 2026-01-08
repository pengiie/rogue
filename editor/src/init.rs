use rogue_engine::app::{App, AppStage};

use crate::render_graph;

pub fn init_post_graphics(app: &mut App) {
    app.insert_system(AppStage::InitPostGraphics, render_graph::init_render_graph);
}
