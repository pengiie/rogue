use rogue_macros::game_component;

use crate::common::serde_util::impl_unit_type_serde;

#[derive(Clone)]
#[game_component(name = "CameraController")]
pub struct CameraController {}

// Don't serialize data for this component.
impl_unit_type_serde!(CameraController);

impl Default for CameraController {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraController {
    pub fn new() -> Self {
        CameraController {}
    }
}
