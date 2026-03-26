use nalgebra::Vector2;
use rogue_macros::Resource;

#[derive(Resource)]
pub struct EditorInput {
    pub global_mouse_pos: Vector2<f32>,
}

impl EditorInput {
    pub fn new() -> Self {
        Self {
            global_mouse_pos: Vector2::new(0.0, 0.0),
        }
    }
}
