use bytemuck::Zeroable;
use log::debug;
use nalgebra::{
    AbstractRotation, Isometry, Isometry3, Matrix4, Point3, Quaternion, Rotation3, Translation3,
    Unit, UnitQuaternion, Vector, Vector3,
};

use crate::{
    common::{aabb::AABB, obb::OBB, ray::Ray},
    consts,
};

pub struct Transform {
    pub position: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
    pub scale: f32,
}

impl Transform {
    pub fn new() -> Self {
        Self {
            position: Vector3::zeros(),
            rotation: UnitQuaternion::identity(),
            scale: 1.0,
        }
    }

    pub fn with_translation(translation: Translation3<f32>) -> Self {
        log::info!("with trans {:?}", translation);
        Self {
            position: translation.vector,
            rotation: UnitQuaternion::identity(),
            scale: 1.0,
        }
    }

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
        Ray::new(
            self.position,
            self.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0)),
        )
    }

    pub fn as_voxel_model_obb(&self, model_dimensions: Vector3<u32>) -> OBB {
        let half_length = model_dimensions.map(|x| x as f32)
            * consts::voxel::VOXEL_METER_LENGTH
            * self.scale
            * 0.5;
        let min = self.position() - half_length;
        let max = self.position() + half_length;

        let rotation_anchor = self.position();

        OBB::new(
            AABB::new_two_point(min, max),
            self.rotation,
            rotation_anchor,
        )
    }

    pub fn rotation(&self) -> Rotation3<f32> {
        self.rotation.to_rotation_matrix()
    }

    pub fn position(&self) -> Vector3<f32> {
        self.position
    }
}
