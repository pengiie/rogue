use nalgebra::{
    AbstractRotation, Isometry3, Matrix4, Point3, Rotation3, Translation3, UnitQuaternion, Vector3,
};
use rogue_macros::game_component;

use crate::common::geometry::aabb::AABB;
use crate::common::geometry::obb::OBB;
use crate::common::geometry::ray::Ray;
use crate::consts;
/// Transform relative to the world-space or parent transform if one exists.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[game_component(name = "Transform", constructible = false)]
pub struct Transform {
    pub position: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
    pub scale: Vector3<f32>,
}

impl Transform {
    pub fn new() -> Self {
        Self {
            position: Vector3::zeros(),
            rotation: UnitQuaternion::identity(),
            scale: Vector3::new(1.0, 1.0, 1.0),
        }
    }

    pub fn with_translation(translation: Translation3<f32>) -> Self {
        Self {
            position: translation.vector,
            rotation: UnitQuaternion::identity(),
            scale: Vector3::new(1.0, 1.0, 1.0),
        }
    }

    pub fn as_relative_transform(&self, parent_transform: &Transform) -> Self {
        // Rotate into the parent transforms reference frame.
        let rot_position = parent_transform.rotation.inverse() * self.position;
        let local_position =
            (rot_position - parent_transform.position).component_mul(&parent_transform.scale);
        let local_rot = parent_transform.rotation.inverse() * self.rotation;
        let local_scale = self.scale.component_div(&parent_transform.scale);
        Self {
            position: local_position,
            rotation: local_rot,
            scale: local_scale,
        }
    }

    pub fn apply_parent_transform(&mut self, parent_transform: &Transform) {
        self.position = (parent_transform.rotation * self.position)
            .component_mul(&parent_transform.scale)
            + parent_transform.position;
        self.rotation = parent_transform.rotation * self.rotation;
        self.scale = self.scale.component_mul(&parent_transform.scale);
    }

    // The world-space transformation matrix of this entity.
    pub fn to_transformation_matrix(&self) -> Matrix4<f32> {
        let translation = Matrix4::<f32>::new_translation(&self.position);
        let rot = self.rotation.to_homogeneous();

        translation * rot
    }

    pub fn to_view_matrix(&self) -> Matrix4<f32> {
        let iso = Isometry3::from_parts(Translation3::from(self.position), self.rotation);
        iso.inverse().to_homogeneous()
    }

    pub fn get_ray(&self) -> Ray {
        Ray::new(self.position, self.forward())
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

    pub fn transform_obb(&self, obb: &OBB) -> OBB {
        return OBB::new(
            AABB::new_two_point(
                obb.aabb.min.component_mul(&self.scale) + self.position,
                obb.aabb.max.component_mul(&self.scale) + self.position,
            ),
            obb.rotation * self.rotation,
            Vector3::zeros(),
        );
    }

    pub fn as_voxel_model_obb(&self, model_dimensions: Vector3<u32>) -> OBB {
        let half_length = model_dimensions.zip_map(&self.scale, |x, y| x as f32 * y)
            * consts::voxel::VOXEL_METER_LENGTH
            * 0.5;
        let min = self.position - half_length;
        let max = self.position + half_length;

        OBB::new(
            AABB::new_two_point(min, max),
            self.rotation,
            Vector3::zeros(),
        )
    }
}
