use nalgebra::{UnitQuaternion, Vector3};

use super::transform::Transform;
use crate::common::color::Color;
use crate::common::geometry::aabb::AABB;
use crate::debug::debug_renderer::DebugRenderer;
use crate::physics::collider::{ColliderDebugColoring, ContactManifold};
use crate::physics::collider_voxel_registry::VoxelColliderRegistry;
use crate::physics::{box_collider::BoxCollider, collider::Collider};

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
        //debug_renderer.draw_capsule(DebugCapsule {
        //    center: self.center + world_transform.position,
        //    orientation: self.orientation * world_transform.rotation,
        //    radius: self.radius,
        //    height: self.half_height,
        //    color: Color::new_srgb(0.7, 0.1, 0.3),
        //    alpha: 0.3,
        //    flags: DebugFlags::SHADING,
        //});
    }
}

impl Collider for CapsuleCollider {
    const NAME: &str = "CapsuleCollider";

    fn aabb(&self, world_transform: &Transform, _: &VoxelColliderRegistry) -> Option<AABB> {
        let up = Vector3::y() * self.half_height;
        let forward = Vector3::z() * self.radius;
        let right = Vector3::x() * self.radius;
        let min = self.center + self.orientation * (-up - forward - right);
        let max = self.center + self.orientation * (up + forward + right);
        return Some(AABB::new_two_point(min, max));
    }

    fn render_debug(
        &self,
        world_transform: &Transform,
        debug_renderer: &mut DebugRenderer,
        coloring: ColliderDebugColoring,
    ) {
        //let (mut bottom, mut top) = self.bottom_top_points();
        //let matrix = world_transform.to_transformation_matrix();
        //bottom = matrix.transform_vector(&bottom);
        //top = matrix.transform_vector(&top);

        //debug_renderer.draw_line(DebugLine {
        //    start: bottom,
        //    end: top,
        //    thickness: self.radius,
        //    color: coloring.color(),
        //    alpha: 0.8,
        //    flags: DebugFlags::NONE,
        //});
    }

    fn serialize_collider(
        &self,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        use erased_serde::Serialize;
        self.erased_serialize(ser)
    }

    unsafe fn deserialize_collider(
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(erased_serde::deserialize::<Self>(de)?) };
        Ok(())
    }
}

pub fn box_capsule_collision_test(
    box_collider: &BoxCollider,
    capsule: &CapsuleCollider,
    transform_box: &Transform,
    transform_capsule: &Transform,
) -> Option<ContactManifold> {
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

    todo!();
}
