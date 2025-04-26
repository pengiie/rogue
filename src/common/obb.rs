use log::debug;
use nalgebra::{UnitQuaternion, Vector3};

use super::aabb::AABB;

#[derive(Debug)]
pub struct OBB {
    pub aabb: AABB,
    pub rotation: UnitQuaternion<f32>,
    pub rotation_anchor: Vector3<f32>,
}

impl OBB {
    /// The rotation anchor is in the same coordinate space as the AABB, world space.
    pub fn new(aabb: AABB, rotation: UnitQuaternion<f32>, rotation_anchor: Vector3<f32>) -> Self {
        Self {
            aabb,
            rotation,
            rotation_anchor,
        }
    }

    pub fn length(&self) -> Vector3<f32> {
        self.aabb.max - self.aabb.min
    }

    pub fn rotated_min_max(&self) -> (Vector3<f32>, Vector3<f32>) {
        let min = self
            .rotation
            .transform_vector(&(self.aabb.min - self.rotation_anchor))
            + self.rotation_anchor;
        let max = self
            .rotation
            .transform_vector(&(self.aabb.max - self.rotation_anchor))
            + self.rotation_anchor;
        (min, max)
    }
}
