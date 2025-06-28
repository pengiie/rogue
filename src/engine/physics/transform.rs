use bytemuck::Zeroable;
use log::debug;
use nalgebra::{
    AbstractRotation, Isometry, Isometry3, Matrix4, Point3, Quaternion, Rotation3, Translation3,
    Unit, UnitQuaternion, Vector, Vector3,
};

use crate::{
    common::{aabb::AABB, obb::OBB, ray::Ray},
    consts,
    engine::entity::ecs_world::ECSWorld,
};

/// Transform relative to the world-space or parent transform if one exists.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
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
        log::info!("with trans {:?}", translation);
        Self {
            position: translation.vector,
            rotation: UnitQuaternion::identity(),
            scale: Vector3::new(1.0, 1.0, 1.0),
        }
    }

    // The world-space transformation matrix of this entity.
    pub fn to_transformation_matrix(&self) -> Matrix4<f32> {
        let translation = Matrix4::<f32>::new_translation(&self.position);
        let rot = self.rotation.to_homogeneous();

        translation * rot
    }

    pub fn to_view_matrix(&self) -> Matrix4<f32> {
        let mut translation = Matrix4::<f32>::new_translation(&-self.position);
        // Perspective expects fowards to be the -z axis.
        let mut rot = self.rotation.to_homogeneous().transpose();
        //let iso = Isometry3::look_at_rh(
        //    &Point3::new(0.0, 0.0, 0.0),
        //    &self
        //        .rotation()
        //        .transform_point(&Point3::new(0.0, 0.0, -1.0)),
        //    &Vector3::y(),
        //);

        rot * translation
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

    pub fn as_voxel_model_obb(&self, model_dimensions: Vector3<u32>) -> OBB {
        let half_length = model_dimensions.zip_map(&self.scale, |x, y| x as f32 * y)
            * consts::voxel::VOXEL_METER_LENGTH
            * 0.5;
        let min = self.position - half_length;
        let max = self.position + half_length;
        let rotation_anchor = self.position;

        OBB::new(
            AABB::new_two_point(min, max),
            self.rotation,
            rotation_anchor,
        )
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct LocalTransform {
    pub transform: Transform,
}

impl std::ops::Deref for LocalTransform {
    type Target = Transform;

    fn deref(&self) -> &Self::Target {
        &self.transform
    }
}

impl std::ops::DerefMut for LocalTransform {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.transform
    }
}
