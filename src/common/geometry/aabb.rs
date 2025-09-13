use nalgebra::Vector3;

use crate::common::geometry::shape::{Face, Shape, Vertex};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AABB {
    pub min: Vector3<f32>,
    pub max: Vector3<f32>,
}

impl AABB {
    pub fn new_two_point(a: Vector3<f32>, b: Vector3<f32>) -> Self {
        let min = a.zip_map(&b, |x, y| x.min(y));
        let max = a.zip_map(&b, |x, y| x.max(y));

        Self { min, max }
    }

    pub fn new_center_extents(center: Vector3<f32>, extents: Vector3<f32>) -> Self {
        assert!(
            extents.iter().all(|x| *x > 0.0),
            "AABB extents must be greater than 0."
        );
        Self {
            min: center - extents,
            max: center + extents,
        }
    }

    pub fn center(&self) -> Vector3<f32> {
        (self.max + self.min) * 0.5
    }

    pub fn side_length(&self) -> Vector3<f32> {
        self.max - self.min
    }

    pub fn half_side_length(&self) -> Vector3<f32> {
        return self.side_length() * 0.5;
    }
}

impl Shape for AABB {
    fn collect_vertices(&self) -> Vec<Vertex> {
        let min = self.min;
        let max = self.max;
        vec![
            Vector3::new(min.x, min.y, min.z),
            Vector3::new(min.x, min.y, max.z),
            Vector3::new(min.x, max.y, min.z),
            Vector3::new(min.x, max.y, max.z),
            Vector3::new(max.x, min.y, min.z),
            Vector3::new(max.x, min.y, max.z),
            Vector3::new(max.x, max.y, min.z),
            Vector3::new(max.x, max.y, max.z),
        ]
    }

    fn collect_faces(&self) -> Vec<Face> {
        vec![
            // Bottom
            Face::new(vec![
                Vector3::new(self.min.x, self.min.y, self.min.z),
                Vector3::new(self.max.x, self.min.y, self.min.z),
                Vector3::new(self.max.x, self.min.y, self.max.z),
                Vector3::new(self.min.x, self.min.y, self.max.z),
            ]),
            // Top
            Face::new(vec![
                Vector3::new(self.min.x, self.max.y, self.min.z),
                Vector3::new(self.max.x, self.max.y, self.min.z),
                Vector3::new(self.max.x, self.max.y, self.max.z),
                Vector3::new(self.min.x, self.max.y, self.max.z),
            ]),
            // Front
            Face::new(vec![
                Vector3::new(self.min.x, self.min.y, self.min.z),
                Vector3::new(self.max.x, self.min.y, self.min.z),
                Vector3::new(self.max.x, self.max.y, self.min.z),
                Vector3::new(self.min.x, self.max.y, self.min.z),
            ]),
            // Back
            Face::new(vec![
                Vector3::new(self.min.x, self.min.y, self.max.z),
                Vector3::new(self.max.x, self.min.y, self.max.z),
                Vector3::new(self.max.x, self.max.y, self.max.z),
                Vector3::new(self.min.x, self.max.y, self.max.z),
            ]),
            // Left
            Face::new(vec![
                Vector3::new(self.min.x, self.min.y, self.min.z),
                Vector3::new(self.min.x, self.min.y, self.max.z),
                Vector3::new(self.min.x, self.max.y, self.max.z),
                Vector3::new(self.min.x, self.max.y, self.min.z),
            ]),
            // Right
            Face::new(vec![
                Vector3::new(self.max.x, self.min.y, self.min.z),
                Vector3::new(self.max.x, self.min.y, self.max.z),
                Vector3::new(self.max.x, self.max.y, self.max.z),
                Vector3::new(self.max.x, self.max.y, self.min.z),
            ]),
        ]
    }
}
