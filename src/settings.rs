use std::f32::consts;

use rogue_macros::Resource;

use crate::common::set::{AttributeSet, AttributeSetImpl};

pub type GraphicsSettingsSet = AttributeSet<GraphicsSettings>;

// TODO: I can derive macro this enum generation and the `AttributeSetImpl` generation. This should
// be done when this attribute set is more used for cases like UI and more fields are added.
#[derive(Clone)]
pub enum GraphicsSettingsAttributes {
    RenderSize((u32, u32)),
}

#[derive(Clone)]
pub struct GraphicsSettings {
    pub render_size: (u32, u32),
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            render_size: (1080, 720),
        }
    }
}

impl AttributeSetImpl for GraphicsSettings {
    type E = GraphicsSettingsAttributes;

    fn aggregate_updates(&self, last: &Self) -> Vec<GraphicsSettingsAttributes> {
        let mut updates = Vec::new();
        if self.render_size != last.render_size {
            updates.push(GraphicsSettingsAttributes::RenderSize(self.render_size));
        }

        updates
    }

    fn aggregate_all_fields(&self) -> Vec<GraphicsSettingsAttributes> {
        vec![GraphicsSettingsAttributes::RenderSize(self.render_size)]
    }
}

#[derive(Resource)]
pub struct Settings {
    /// The field of view in degrees of the camera.
    pub camera_fov: f32,

    /// The mouse sensitivity of pixels per degree of rotation.
    pub mouse_sensitivity: f32,

    pub graphics: GraphicsSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            camera_fov: consts::FRAC_PI_2,
            mouse_sensitivity: 0.005,

            graphics: GraphicsSettings::default(),
        }
    }
}
