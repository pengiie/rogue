use log::debug;
use rogue_macros::Resource;

use crate::engine::ecs::ecs_world::Entity;

use super::renderer;

#[derive(Resource)]
pub struct MainCamera {
    camera: Option<Entity>,
}

impl MainCamera {
    pub fn new() -> Self {
        Self { camera: None }
    }

    pub fn camera(&self) -> Option<Entity> {
        self.camera
    }

    pub fn set_camera(&mut self, camera: Entity, camera_name: &str) {
        debug!(
            "Switched to main camera `{}`, entity id {:?}.",
            camera_name, camera
        );
        self.camera = Some(camera);
    }
}

pub struct Camera {
    fov: f32,
}

impl Camera {
    pub fn new(fov: f32) -> Self {
        Self { fov }
    }

    pub fn fov(&self) -> f32 {
        self.fov
    }
}
