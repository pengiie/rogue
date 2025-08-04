use nalgebra::{Quaternion, Vector2, Vector3};

use crate::{
    common::color::Color,
    engine::debug::{DebugFlags, DebugPlane, DebugRenderer},
};

use super::{
    capsule_collider::CapsuleCollider,
    physics_world::{Collider, ColliderType, CollisionInfo},
    transform::Transform,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlaneCollider {
    pub center: Vector3<f32>,
    pub normal: Vector3<f32>,
    pub size: Vector2<f32>,
}

impl Default for PlaneCollider {
    fn default() -> Self {
        Self {
            center: Vector3::zeros(),
            normal: Vector3::y(),
            size: Vector2::new(1.0, 1.0),
        }
    }
}

impl PlaneCollider {}

impl Collider for PlaneCollider {
    fn test_collision(&self, other: &dyn Collider) -> Option<CollisionInfo> {
        match other.collider_type() {
            ColliderType::Capsule => {
                let capsule = other.downcast_ref::<CapsuleCollider>().unwrap();

                Some(CollisionInfo {
                    penetration_depth: Vector3::zeros(),
                })
            }
            ColliderType::Null | ColliderType::Plane => None,
        }
    }

    fn aabb(&self, world_transform: &Transform) -> crate::common::aabb::AABB {
        todo!()
    }

    fn collider_type(&self) -> super::physics_world::ColliderType {
        todo!()
    }
}
