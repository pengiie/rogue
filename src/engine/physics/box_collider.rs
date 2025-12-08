use erased_serde::Serialize;
use nalgebra::{Quaternion, Rotation3, UnitQuaternion, Vector2, Vector3};

use super::{capsule_collider::CapsuleCollider, transform::Transform};
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::obb::OBB;
use crate::engine::editor::ui::entity_properties::{position_ui, rotation_ui, scale_ui};
use crate::engine::physics::collider::{ColliderDebugColoring, ContactManifold};
use crate::engine::voxel::voxel_world::VoxelWorld;
use crate::{
    common::{color::Color, geometry::shape::Shape},
    engine::{
        debug::{DebugFlags, DebugOBB, DebugRenderer},
        physics::{
            capsule_collider::box_capsule_collision_test,
            collider::{Collider, ColliderMethods, ContactPair},
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

pub fn test_intersection_box_box(
    box_a: &BoxCollider,
    box_b: &BoxCollider,
    entity_transform_a: &Transform,
    entity_transform_b: &Transform,
) -> Option<ContactManifold> {
    // Transform each collider by its associated entity's world transform.
    let world_space_box_a = entity_transform_a.transform_obb(&box_a.obb);
    let world_space_box_b = entity_transform_b.transform_obb(&box_b.obb);
    // Use SAT for this one.
    return world_space_box_a.test_intersection(&world_space_box_b);
}

impl Collider for BoxCollider {
    const NAME: &str = "BoxCollider";

    fn aabb(&self, world_transform: &Transform, voxel_world: &VoxelWorld) -> AABB {
        world_transform.transform_obb(&self.obb).bounding_aabb()
    }

    fn render_debug(
        &self,
        world_transform: &Transform,
        debug_renderer: &mut DebugRenderer,
        coloring: ColliderDebugColoring,
    ) {
        debug_renderer.draw_obb(DebugOBB {
            obb: &world_transform.transform_obb(&self.obb),
            thickness: 0.025,
            color: coloring.color(),
            alpha: 0.75,
            flags: DebugFlags::XRAY,
        });
        debug_renderer.draw_obb(DebugOBB {
            obb: &world_transform
                .transform_obb(&self.obb)
                .bounding_aabb()
                .as_obb(),
            thickness: 0.025,
            color: Color::new_srgb_hex("#55AAB3"),
            alpha: 0.75,
            flags: DebugFlags::XRAY,
        });
    }

    fn serialize_collider(
        &self,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
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

    fn collider_component_ui(&mut self, ui: &mut egui::Ui) {
        let mut center = self.obb.aabb.center();
        let original_center = center.clone();
        position_ui(ui, &mut center);

        let mut half_side_length = self.obb.aabb.half_side_length();
        let original_half_side_length = half_side_length.clone();

        rotation_ui(ui, &mut self.obb.rotation);

        scale_ui(ui, &mut half_side_length);
        if center != original_center || half_side_length != original_half_side_length {
            self.obb.aabb = AABB::new_center_extents(center, half_side_length);
        }
    }
}
