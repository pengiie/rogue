use bytemuck::Zeroable;
use erased_serde::Serialize;
use log::debug;
use nalgebra::{
    AbstractRotation, Isometry, Isometry3, Matrix4, Point3, Quaternion, Rotation3, Translation3,
    Unit, UnitQuaternion, Vector, Vector3,
};

use crate::common::geometry::aabb::AABB;
use crate::common::geometry::obb::OBB;
use crate::common::geometry::ray::Ray;
use crate::engine::entity::component::GameComponent;
use crate::{consts, engine::entity::ecs_world::ECSWorld};

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

    pub fn transform_obb(&self, obb: &OBB) -> OBB {
        return OBB::new(
            AABB::new_two_point(obb.aabb.min + self.position, obb.aabb.max + self.position),
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

        OBB::new(AABB::new_two_point(min, max), self.rotation, half_length)
    }
}

impl GameComponent for Transform {
    fn clone_component(
        &self,
        ctx: &mut crate::engine::entity::component::GameComponentContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let dst_ptr = dst_ptr as *mut Transform;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(self.clone()) };
    }

    fn serialize_component(
        &self,
        ctx: crate::engine::entity::component::GameComponentContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        erased_serde::Serialize::erased_serialize(self, ser)
    }

    fn deserialize_component(
        &self,
        ctx: crate::engine::entity::component::GameComponentContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        let dst_ptr = dst_ptr as *mut Transform;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(erased_serde::deserialize(de)?) };
        Ok(())
    }
}
