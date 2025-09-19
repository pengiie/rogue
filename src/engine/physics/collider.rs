use nalgebra::Vector3;

use crate::common::geometry::aabb::AABB;
use crate::engine::{
    debug::DebugRenderer,
    physics::{collider_registry::ColliderId, transform::Transform},
};

pub struct CollisionInfo {
    // Normal is facing away from the first object and towards the second object.
    pub penetration_depth: Vector3<f32>,
    // The world-space contact points of the first collider.
    pub contact_points_a: Vec<Vector3<f32>>,
    // The world-space contact points of the second collider.
    pub contact_points_b: Vec<Vector3<f32>>,
}

pub trait ColliderConcrete {
    fn concrete_collider_type() -> ColliderType;
}

pub trait Collider: downcast::Any {
    fn test_collision(
        &self,
        other: &dyn Collider,
        transform_a: &Transform,
        transform_b: &Transform,
    ) -> Option<CollisionInfo>;
    fn aabb(&self, world_transform: &Transform) -> AABB;
    fn collider_type(&self) -> ColliderType;

    fn render_debug(&self, world_transform: &Transform, debug_renderer: &mut DebugRenderer) {}
}

downcast::downcast!(dyn Collider);

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Colliders {
    pub colliders: Vec<ColliderId>,
}

impl Default for Colliders {
    fn default() -> Self {
        Self::new()
    }
}

impl Colliders {
    pub fn new() -> Self {
        Self {
            colliders: Vec::new(),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, serde::Serialize, serde::Deserialize, Debug)]
pub enum ColliderType {
    Null,
    Capsule,
    Plane,
    Box,
}
