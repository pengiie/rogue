use nalgebra::{Rotation3, UnitQuaternion, Vector2, Vector3};

use super::{capsule_collider::CapsuleCollider, transform::Transform};
use crate::common::geometry::aabb::AABB;
use crate::engine::physics::collider::{Collider, ColliderConcrete, ColliderType, CollisionInfo};

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

impl ColliderConcrete for PlaneCollider {
    fn concrete_collider_type() -> ColliderType {
        ColliderType::Plane
    }
}

impl Collider for PlaneCollider {
    fn test_collision(
        &self,
        other: &dyn Collider,
        transform_a: &Transform,
        transform_b: &Transform,
    ) -> Option<CollisionInfo> {
        match other.collider_type() {
            ColliderType::Capsule => {
                let capsule = other.downcast_ref::<CapsuleCollider>().unwrap();

                Some(CollisionInfo {
                    penetration_depth: Vector3::zeros(),
                    contact_point: Vector3::zeros(),
                })
            }
            _ => None,
            ColliderType::Null | ColliderType::Plane => None,
        }
    }

    fn aabb(&self, world_transform: &Transform) -> AABB {
        let rot = UnitQuaternion::from_rotation_matrix(
            &Rotation3::rotation_between(&Vector3::y(), &self.normal)
                .unwrap_or(Rotation3::identity()),
        );
        let size_3 = Vector3::new(self.size.x, 0.0, self.size.y);
        let min = self.center + rot * -size_3;
        let max = self.center + rot * size_3;
        return AABB::new_two_point(min, max);
    }

    fn collider_type(&self) -> ColliderType {
        ColliderType::Plane
    }
}
