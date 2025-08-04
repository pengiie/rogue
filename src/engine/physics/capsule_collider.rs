use nalgebra::{Quaternion, UnitQuaternion, Vector3};

use crate::{
    common::color::Color,
    engine::debug::{DebugCapsule, DebugFlags, DebugRenderer},
};

use super::transform::Transform;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct CapsuleCollider {
    /// Relative to the current node's transform.
    pub center: Vector3<f32>,
    pub orientation: UnitQuaternion<f32>,
    pub radius: f32,
    pub height: f32,
}

impl CapsuleCollider {
    pub fn new() -> Self {
        Self {
            center: Vector3::zeros(),
            orientation: UnitQuaternion::identity(),
            radius: 1.0,
            height: 2.0,
        }
    }

    pub fn render_debug(&self, world_transform: &Transform, debug_renderer: &mut DebugRenderer) {
        debug_renderer.draw_capsule(DebugCapsule {
            center: self.center + world_transform.position,
            orientation: self.orientation * world_transform.rotation,
            radius: self.radius,
            height: self.height,
            color: Color::new_srgb(0.7, 0.1, 0.3),
            alpha: 0.3,
            flags: DebugFlags::SHADING,
        });
    }
}
