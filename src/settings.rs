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
    engine::{
        asset::repr::settings::SettingsAsset,
        graphics::{backend::GfxPresentMode, renderer::Antialiasing},
    },
};

/// Called/recieved whenever a graphics setting is changed.
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
            present_mode: GfxPresentMode::Vsync,
            triple_buffering: true,
        }
    }
}

/// Called/recieved whenever a setting is changed.
pub enum SettingsEvent {
    TicksPerSecond(u32),
}

#[derive(Resource, Serialize, Deserialize)]
pub struct Settings {
    /// The field of view in degrees of the camera.
    pub camera_fov: f32,

    /// The mouse sensitivity of pixels per degree of rotation.
    pub mouse_sensitivity: f32,

    /// The chunk render distance, also acts as the load
    /// and simulation distance for simplicity.
    pub chunk_render_distance: u32,

    /// The amount of chunk that can be enqueued at a time.
    /// The current default is number of logical CPUs.
    pub chunk_queue_capacity: u32,

    // Tick rate only affects world events.
    pub ticks_per_seconds: u32,

    pub graphics: GraphicsSettings,
    pub frame_rate_cap: u32,
}

impl From<&SettingsAsset> for Settings {
    fn from(s: &SettingsAsset) -> Self {
        Self {
            camera_fov: consts::FRAC_PI_2,
            mouse_sensitivity: s.mouse_sensitivity,

            chunk_render_distance: s.chunk_render_distance,
            chunk_queue_capacity: std::thread::available_parallelism()
                .unwrap_or(NonZeroUsize::new(4).unwrap())
                .get() as u32,

            ticks_per_seconds: 10,

            graphics: GraphicsSettings::default(),
            frame_rate_cap: 200,
        }
    }
}

impl From<&Settings> for SettingsAsset {
    fn from(s: &Settings) -> Self {
        Self {
            mouse_sensitivity: s.mouse_sensitivity,
            chunk_render_distance: s.chunk_render_distance,
        }
    }
}
