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

    pub fn forward(&self) -> Vector3<f32> {
        self.rotation.transform_vector(&Vector3::z())
    }

    pub fn right(&self) -> Vector3<f32> {
        self.rotation.transform_vector(&Vector3::x())
    }

    pub fn up(&self) -> Vector3<f32> {
        self.rotation.transform_vector(&Vector3::y())
    }

    pub fn most_opposite_face_normal(
        &self,
        direction: &Vector3<f32>,
    ) -> (Vector3<f32>, /*dot*/ f32) {
        let (local_axis, dot) = self.most_aligned_face_normal(&-direction);
        (-local_axis, dot)
    }

    pub fn face_from_local_axis(&self, local_axis: &Vector3<f32>) -> Face {
        let (axis, sign) = local_axis
            .iter()
            .enumerate()
            .find_map(|(i, &x)| (x.abs() > 0.0).then_some((i, x.signum())))
            .unwrap();

        const AXES: [Vector3<f32>; 3] = [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ];
        let u = self.rotation * AXES[(axis + 1) % 3] * sign;
        let v = self.rotation * AXES[(axis + 2) % 3] * sign;
        let center = self.rotation * AXES[axis] * sign;
        Face::new(vec![
            center - u - v,
            center + u - v,
            center + u + v,
            center - u + v,
        ])
    }

    // Returns local axis most aligned with the given direction.
    pub fn most_aligned_face_normal(
        &self,
        direction: &Vector3<f32>,
    ) -> (Vector3<f32>, /*dot*/ f32) {
        let local_dir = self.rotation.inverse() * direction;
        let directions = vec![Vector3::x(), Vector3::y(), Vector3::z()];
        let mut best_dir = directions[0];
        let mut best_dot = best_dir.dot(direction);
        for dir in directions.iter().skip(1) {
            let dot = dir.dot(direction);
            if dot.abs() > best_dot {
                best_dir = dot.signum() * dir;
                best_dot = dot.abs();
            }
        }
        (best_dir, best_dot)
    }

    pub fn rotated_min_max(&self) -> (Vector3<f32>, Vector3<f32>) {
        let dv = (self.aabb.max - self.aabb.min);
        let center = self.aabb.center();
        let anchor = self.rotation_anchor + center;
        let min = self.rotation.transform_vector(&(self.aabb.min - anchor)) + anchor;
        let max = self.rotation.transform_vector(&(self.aabb.max - anchor)) + anchor;
        (min, max)
    }

    pub fn bounding_aabb(&self) -> AABB {
        let (mut min, mut max) = self.rotated_min_max();
        for point in Shape::collect_vertices(self) {
            min = min.zip_map(&point, |x, y| x.min(y));
            max = max.zip_map(&point, |x, y| x.max(y));
        }
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
            Face::new(vec![min, min + forward, min + right + forward, min + right]),
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
                min + up + forward,
                min + right + up + forward,
                min + right + forward,
            ]),
            // Left
            Face::new(vec![min, min + up, min + forward + up, min + forward]),
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
