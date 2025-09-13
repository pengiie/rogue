use nalgebra::{Quaternion, UnitQuaternion, Vector3};

use super::transform::Transform;
use crate::common::geometry::aabb::AABB;
use crate::{
    common::color::Color,
    engine::{
        debug::{DebugCapsule, DebugFlags, DebugLine, DebugRenderer},
        physics::{
            box_collider::BoxCollider,
            collider::{Collider, ColliderConcrete, ColliderType, CollisionInfo},
        },
    },
};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct CapsuleCollider {
    /// Relative to the current node's transform.
    pub center: Vector3<f32>,
    pub orientation: UnitQuaternion<f32>,
    pub radius: f32,
    pub half_height: f32,
}

impl CapsuleCollider {
    pub fn new() -> Self {
        Self {
            center: Vector3::zeros(),
            orientation: UnitQuaternion::identity(),
            radius: 1.0,
            half_height: 2.0,
        }
    }

    pub fn bottom_top_points(&self) -> (Vector3<f32>, Vector3<f32>) {
        let up = Vector3::y() * self.half_height;
        let bottom = self.center + self.orientation * -up;
        let top = self.center + self.orientation * up;
        return (bottom, top);
    }

    pub fn render_debug(&self, world_transform: &Transform, debug_renderer: &mut DebugRenderer) {
        debug_renderer.draw_capsule(DebugCapsule {
            center: self.center + world_transform.position,
            orientation: self.orientation * world_transform.rotation,
            radius: self.radius,
            height: self.half_height,
            color: Color::new_srgb(0.7, 0.1, 0.3),
            alpha: 0.3,
            flags: DebugFlags::SHADING,
        });
    }
}

impl ColliderConcrete for CapsuleCollider {
    fn concrete_collider_type() -> ColliderType {
        ColliderType::Capsule
    }
}

impl Collider for CapsuleCollider {
    fn test_collision(
        &self,
        other: &dyn Collider,
        transform_a: &Transform,
        transform_b: &Transform,
    ) -> Option<CollisionInfo> {
        match other.collider_type() {
            ColliderType::Box => {
                let box_collider = other.downcast_ref::<BoxCollider>().unwrap();
                return box_capsule_collision_test(box_collider, self, transform_b, transform_a);
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

    fn aabb(&self, world_transform: &Transform) -> crate::common::geometry::aabb::AABB {
        let up = Vector3::y() * self.half_height;
        let forward = Vector3::z() * self.radius;
        let right = Vector3::x() * self.radius;
        let min = self.center + self.orientation * (-up - forward - right);
        let max = self.center + self.orientation * (up + forward + right);
        return AABB::new_two_point(min, max);
    }

    fn collider_type(&self) -> ColliderType {
        ColliderType::Capsule
    }

    fn render_debug(&self, world_transform: &Transform, debug_renderer: &mut DebugRenderer) {
        let (mut bottom, mut top) = self.bottom_top_points();
        let matrix = world_transform.to_transformation_matrix();
        bottom = matrix.transform_vector(&bottom);
        top = matrix.transform_vector(&top);

        debug_renderer.draw_line(DebugLine {
            start: bottom,
            end: top,
            thickness: self.radius,
            color: Color::new_srgb_hex("#FF0000"),
            alpha: 0.8,
            flags: DebugFlags::NONE,
        });
    }
}

pub fn box_capsule_collision_test(
    box_collider: &BoxCollider,
    capsule: &CapsuleCollider,
    transform_box: &Transform,
    transform_capsule: &Transform,
) -> Option<CollisionInfo> {
    let transform_box = transform_box.to_transformation_matrix();
    let transform_capsule = transform_capsule.to_transformation_matrix();

    let (mut min, mut max) = box_collider.obb.rotated_min_max();
    min = transform_box.transform_vector(&min);
    max = transform_box.transform_vector(&max);

    let (mut bottom, mut top) = capsule.bottom_top_points();
    bottom = transform_capsule.transform_vector(&bottom);
    top = transform_capsule.transform_vector(&top);

    let lv = top - bottom;
    let mv = min - bottom;
    // Projective of mv onto the line lv.
    let p_t = (mv.dot(&lv) / lv.norm_squared()).clamp(0.0, 1.0);
    let pv = p_t * lv;

    let closest_vec = pv - min;

    if closest_vec.norm() > capsule.radius {
        return None;
    }

    let box_to_capsule_penetration = closest_vec.normalize() * capsule.radius;

    return Some(CollisionInfo {
        penetration_depth: box_to_capsule_penetration,
        contact_point: Vector3::zeros(),
    });
}
