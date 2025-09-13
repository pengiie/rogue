use log::debug;
use nalgebra::{UnitQuaternion, Vector3};

use crate::common::geometry::{
    aabb::AABB,
    shape::{Face, Shape, Vertex},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OBB {
    pub aabb: AABB,
    pub rotation: UnitQuaternion<f32>,
    // Relative to `aabb.center()`.
    pub rotation_anchor: Vector3<f32>,
}

impl OBB {
    pub fn new_identity() -> Self {
        Self {
            aabb: AABB::new_center_extents(Vector3::zeros(), Vector3::new(0.5, 0.5, 0.5)),
            rotation: UnitQuaternion::identity(),
            rotation_anchor: Vector3::zeros(),
        }
    }

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
        let dv = (self.aabb.max - self.aabb.min);
        let min = self.rotation.transform_vector(&-self.rotation_anchor)
            + self.rotation_anchor
            + self.aabb.min;
        let max = self.rotation.transform_vector(&(dv - self.rotation_anchor))
            + self.rotation_anchor
            + self.aabb.min;
        (min, max)
    }

    pub fn bounding_aabb(&self) -> AABB {
        let (min, max) = self.rotated_min_max();
        return AABB::new_two_point(min, max);
    }
}

impl Shape for OBB {
    fn collect_vertices(&self) -> Vec<Vertex> {
        let (min, _) = self.rotated_min_max();
        let right = self
            .rotation
            .transform_vector(&(Vector3::x() * (self.aabb.max.x - self.aabb.min.x)));
        let up = self
            .rotation
            .transform_vector(&(Vector3::y() * (self.aabb.max.y - self.aabb.min.y)));
        let forward = self
            .rotation
            .transform_vector(&(Vector3::z() * (self.aabb.max.z - self.aabb.min.z)));
        vec![
            min,
            min + forward,
            min + up,
            min + up + forward,
            min + right,
            min + right + forward,
            min + right + up,
            min + right + up + forward,
        ]
    }

    fn collect_faces(&self) -> Vec<Face> {
        let (min, _) = self.rotated_min_max();
        let right = self
            .rotation
            .transform_vector(&(Vector3::x() * (self.aabb.max.x - self.aabb.min.x)));
        let up = self
            .rotation
            .transform_vector(&(Vector3::y() * (self.aabb.max.y - self.aabb.min.y)));
        let forward = self
            .rotation
            .transform_vector(&(Vector3::z() * (self.aabb.max.z - self.aabb.min.z)));
        vec![
            // Bottom
            Face::new(vec![min, min + right, min + right + forward, min + forward]),
            // Top
            Face::new(vec![
                min + up,
                min + up + right,
                min + up + right + forward,
                min + up + forward,
            ]),
            // Front
            Face::new(vec![min, min + right, min + right + up, min + up]),
            // Back
            Face::new(vec![
                min + forward,
                min + right + forward,
                min + right + up + forward,
                min + up + forward,
            ]),
            // Left
            Face::new(vec![min, min + forward, min + forward + up, min + up]),
            // Right
            Face::new(vec![
                min + right,
                min + right + forward,
                min + right + forward + up,
                min + right + up,
            ]),
        ]
    }
}
