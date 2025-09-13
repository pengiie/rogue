use nalgebra::{Quaternion, Rotation3, UnitQuaternion, Vector2, Vector3};

use super::{capsule_collider::CapsuleCollider, transform::Transform};
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::obb::OBB;
use crate::{
    common::{color::Color, geometry::shape::Shape},
    engine::{
        debug::{DebugFlags, DebugOBB, DebugRenderer},
        physics::{
            capsule_collider::box_capsule_collision_test,
            collider::{Collider, ColliderConcrete, ColliderType, CollisionInfo},
        },
    },
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BoxCollider {
    pub obb: OBB,
}

impl Default for BoxCollider {
    fn default() -> Self {
        Self {
            obb: OBB::new_identity(),
        }
    }
}

impl BoxCollider {}

impl ColliderConcrete for BoxCollider {
    fn concrete_collider_type() -> ColliderType {
        ColliderType::Box
    }
}

impl Collider for BoxCollider {
    fn test_collision(
        &self,
        other: &dyn Collider,
        transform_a: &Transform,
        transform_b: &Transform,
    ) -> Option<CollisionInfo> {
        match other.collider_type() {
            ColliderType::Capsule => {
                let capsule = other.downcast_ref::<CapsuleCollider>().unwrap();
                return box_capsule_collision_test(self, capsule, transform_a, transform_b);
            }
            ColliderType::Box => {
                let other = other.downcast_ref::<BoxCollider>().unwrap();
                let self_world_space = transform_a.transform_obb(&self.obb);
                let other_world_space = transform_b.transform_obb(&other.obb);
                return self_world_space.test_intersection(&other_world_space);
            }
            ColliderType::Null => None,
            _ => {
                log::error!(
                    "Collision not implemented for {:?} and {:?}",
                    self.collider_type(),
                    other.collider_type()
                );
                None
            }
        }
    }

    fn aabb(&self, world_transform: &Transform) -> AABB {
        return self.obb.bounding_aabb();
    }

    fn collider_type(&self) -> ColliderType {
        ColliderType::Box
    }

    fn render_debug(&self, world_transform: &Transform, debug_renderer: &mut DebugRenderer) {
        debug_renderer.draw_obb(DebugOBB {
            obb: &world_transform.transform_obb(&self.obb),
            thickness: 0.025,
            color: Color::new_srgb_hex("#FF00FF"),
            alpha: 0.75,
            flags: DebugFlags::XRAY,
        });
    }
}
