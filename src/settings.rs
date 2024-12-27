use std::{
    collections::HashSet,
    f32::consts,
    num::{NonZero, NonZeroUsize},
};

use downcast::Any;
use log::debug;
use nalgebra::Vector2;
use rogue_macros::Resource;
use serde::{Deserialize, Serialize};

use crate::{
    common::set::{AttributeSet, AttributeSetImpl},
    engine::graphics::{backend::GfxPresentMode, renderer::Antialiasing},
};

/// Called/recieved whenever a setting is changed.
pub enum GraphicsSettingsEvent {
    RTSize(Vector2<u32>),
    Antialiasing(Antialiasing),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphicsSettings {
    pub rt_size: Vector2<u32>,
    pub antialiasing: Antialiasing,
    pub present_mode: GfxPresentMode,
    pub triple_buffering: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            // Target 720p upscaled to native resolution running at >90fps on my gtx 1070.
            rt_size: Vector2::new(1280, 720),
            antialiasing: Antialiasing::None,
            present_mode: GfxPresentMode::NoVsync,
            triple_buffering: true,
        }
    }
}

#[derive(Resource, Serialize, Deserialize)]
pub struct Settings {
    /// The field of view in degrees of the camera.
    pub camera_fov: f32,

    /// The mouse sensitivity of pixels per degree of rotation.
    pub mouse_sensitivity: f32,

    /// The chunk render distance.
    pub chunk_render_distance: u32,
    /// The amount of chunk that can be enqueued at a time.
    /// The current default is number of logical CPUs.
    pub chunk_queue_capacity: u32,

    pub graphics: GraphicsSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            camera_fov: consts::FRAC_PI_2,
            mouse_sensitivity: 0.002,

            chunk_render_distance: 8,
            chunk_queue_capacity: std::thread::available_parallelism()
                .unwrap_or(NonZeroUsize::new(4).unwrap())
                .get() as u32,

            graphics: GraphicsSettings::default(),
        }
    }
}
