use rogue_macros::Resource;

#[derive(Resource)]
pub struct UIState {
    pub zoom_factor: f32,
    pub player_fov: f32,
}

impl Default for UIState {
    fn default() -> Self {
        Self {
            zoom_factor: 1.0,
            player_fov: 90.0,
        }
    }
}

impl UIState {}
