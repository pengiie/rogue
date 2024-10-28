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

    pub fn as_acceleration_data(&self) -> Vec<u32> {
        let rotation_matrix = self
            .rotation
            .to_rotation_matrix()
            .matrix()
            .map(|x| x.to_bits());

        let min_bits = self.aabb.min.map(|x| x.to_bits());
        let max_bits = self.aabb.max.map(|x| x.to_bits());

        let rotation_anchor = self.rotation_anchor;
        let rotation_anchor_bits = rotation_anchor.map(|x| x.to_bits());

        vec![
            // AABB
            min_bits.x,
            min_bits.y,
            min_bits.z,
            max_bits.x,
            max_bits.y,
            max_bits.z,
            // Rotation anchor (what the ray origin rotates about)
            rotation_anchor_bits.x,
            rotation_anchor_bits.y,
            rotation_anchor_bits.z,
            // Rotation matrix
            rotation_matrix.m11,
            rotation_matrix.m12,
            rotation_matrix.m13,
            rotation_matrix.m21,
            rotation_matrix.m22,
            rotation_matrix.m23,
            rotation_matrix.m31,
            rotation_matrix.m32,
            rotation_matrix.m33,
        ]
    }
}
