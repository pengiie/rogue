use nalgebra::{Rotation3, UnitQuaternion, Vector2, Vector3};

use super::{capsule_collider::CapsuleCollider, transform::Transform};
use crate::common::geometry::aabb::AABB;
use crate::engine::physics::collider::{Collider, ColliderMethods, ContactManifold, ContactPair};

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
    const NAME: &str = "PlaneCollider";

    fn aabb(&self, world_transform: &Transform, voxel_world: &VoxelWorld) -> AABB {
        let rot = UnitQuaternion::from_rotation_matrix(
            &Rotation3::rotation_between(&Vector3::y(), &self.normal)
                .unwrap_or(Rotation3::identity()),
        );
        let size_3 = Vector3::new(self.size.x, 0.0, self.size.y);
        let min = self.center + rot * -size_3;
        let max = self.center + rot * size_3;
        return AABB::new_two_point(min, max);
    }

    fn serialize_collider(
        &self,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    unsafe fn deserialize_collider(
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        todo!()
    }
}
