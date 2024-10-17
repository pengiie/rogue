use std::{collections::HashSet, f32::consts};

use downcast::Any;
use log::debug;
use rogue_macros::Resource;

use crate::{
    common::set::{AttributeSet, AttributeSetImpl},
    engine::graphics::renderer::Antialiasing,
};

pub type GraphicsSettingsSet = AttributeSet<GraphicsSettings>;

// TODO: I can derive macro this enum generation and the `AttributeSetImpl` generation. This should
// be done when this attribute set is more used for cases like UI and more fields are added.
#[derive(Clone, Hash, PartialEq, PartialOrd, Ord, Eq, Debug)]
pub enum GraphicsSettingsAttributes {
    RenderSize((u32, u32)),
    Antialiasing(Antialiasing),
}

#[derive(Clone, Debug)]
pub struct GraphicsSettings {
    pub render_size: (u32, u32),
    pub antialiasing: Antialiasing,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            // Target 720p upscaled to native resolution running at >90fps on my gtx 1070.
            render_size: (720, 480),
            antialiasing: Antialiasing::None,
        }
    }
}

impl AttributeSetImpl for GraphicsSettings {
    type E = GraphicsSettingsAttributes;

    fn aggregate_updates(&self, last: &Self) -> HashSet<GraphicsSettingsAttributes> {
        let mut updates = HashSet::new();
        if self.render_size != last.render_size {
            updates.insert(GraphicsSettingsAttributes::RenderSize(self.render_size));
        }
        if self.antialiasing != last.antialiasing {
            updates.insert(GraphicsSettingsAttributes::Antialiasing(self.antialiasing));
        }

        updates
    }

    fn aggregate_all_fields(&self) -> HashSet<GraphicsSettingsAttributes> {
        vec![
            GraphicsSettingsAttributes::RenderSize(self.render_size),
            GraphicsSettingsAttributes::Antialiasing(self.antialiasing),
        ]
        .into_iter()
        .collect::<HashSet<_>>()
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
