use log::debug;
use nalgebra::{Isometry3, Matrix4, Vector2, Vector3};
use rogue_macros::{Resource, game_component};

use super::renderer;
use crate::common::geometry::ray::Ray;
use crate::consts;
use crate::entity::component::GameComponentSerializeContext;
use crate::entity::{component::GameComponent, ecs_world::Entity};
use crate::physics::transform::Transform;

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
#[game_component(name = "Camera")]
pub struct Camera {
    pub fov: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera::new(90.0)
    }
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
        let mut mat = Matrix4::<f32>::identity();
        mat.m11 = 1.0 / (aspect_ratio);
        mat.m22 = 1.0;
        mat.m33 = -self.far_plane / (self.far_plane - self.near_plane);
        mat.m43 = -1.0;
        mat.m34 = (-self.far_plane * self.near_plane) / (self.far_plane - self.near_plane);
        mat.m44 = 0.0;
        mat
    }

    pub fn fov(&self) -> f32 {
        self.fov
    }

    pub fn near_plane(&self) -> f32 {
        self.near_plane
    }

    pub fn create_ray(&self, transform: &Transform, uv: Vector2<f32>, aspect_ratio: f32) -> Ray {
        let ndc = Vector2::new(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let scaled_ndc = Vector2::new(ndc.x * aspect_ratio, ndc.y) * (self.fov / 2.0).tan();
        let dir = Vector3::new(scaled_ndc.x, scaled_ndc.y, -1.0).normalize();
        Ray::new(
            transform.position,
            transform.rotation.transform_vector(&dir),
        )
    }

    pub fn far_plane(&self) -> f32 {
        self.far_plane
    }
}
