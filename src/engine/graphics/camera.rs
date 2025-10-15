use log::debug;
use nalgebra::{Isometry3, Matrix4};
use rogue_macros::Resource;

use crate::{
    consts,
    engine::entity::{component::GameComponent, ecs_world::Entity},
};

use super::renderer;

#[derive(Resource)]
pub struct MainCamera {
    pub camera: Option<(Entity, String)>,
}

impl MainCamera {
    pub fn new_empty() -> Self {
        Self { camera: None }
    }

    pub fn new(entity: Entity, name: impl ToString) -> Self {
        Self {
            camera: Some((entity, name.to_string())),
        }
    }

    pub fn camera(&self) -> Option<Entity> {
        self.camera.as_ref().map(|x| x.0)
    }

    pub fn camera_name(&self) -> Option<&str> {
        self.camera.as_ref().map(|x| x.1.as_ref())
    }

    pub fn set_camera(&mut self, camera: Entity, camera_name: &str) {
        debug!(
            "Switched to main camera `{}`, entity id {:?}.",
            camera_name, camera
        );
        self.camera = Some((camera, camera_name.to_owned()));
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Camera {
    pub fov: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl Camera {
    pub const FOV_90: f32 = std::f32::consts::FRAC_PI_2;

    pub fn new(fov: f32) -> Self {
        Self {
            fov,
            near_plane: consts::gfx::CAMERA_NEAR_PLANE,
            far_plane: consts::gfx::CAMERA_FAR_PLANE,
        }
    }

    pub fn projection_matrix(&self, aspect_ratio: f32) -> Matrix4<f32> {
        let mut mat = Matrix4::identity();
        mat.m11 = 1.0 / aspect_ratio;
        mat.m43 = 1.0;
        mat.m44 = 0.0;
        //mat.m34 *= -1.0;
        mat
    }

    pub fn fov(&self) -> f32 {
        self.fov
    }

    pub fn near_plane(&self) -> f32 {
        self.near_plane
    }

    pub fn far_plane(&self) -> f32 {
        self.far_plane
    }
}

impl GameComponent for Camera {
    fn clone_component(
        &self,
        ctx: &mut crate::engine::entity::component::GameComponentContext<'_>,
        dst_ptr: *mut u8,
    ) {
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { (dst_ptr as *mut Self).write(self.clone()) };
    }

    fn serialize_component(
        &self,
        ctx: crate::engine::entity::component::GameComponentContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    fn deserialize_component(
        &self,
        ctx: crate::engine::entity::component::GameComponentContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        todo!()
    }
}
