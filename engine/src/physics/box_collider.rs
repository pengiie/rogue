use super::transform::Transform;
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::obb::OBB;
use crate::common::{color::Color, geometry::shape::Shape};
use crate::debug::debug_renderer::DebugRenderer;
use crate::egui::util::{position_ui, rotation_ui, scale_ui};
use crate::physics::collider::{Collider, ColliderDebugColoring, ContactManifold};
use crate::physics::collider_voxel_registry::VoxelColliderRegistry;
use erased_serde::Serialize;

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
    let box_a = entity_transform_a.transform_obb(&box_a.obb);
    let box_b = entity_transform_b.transform_obb(&box_b.obb);
    box_a.test_intersection(&box_b)
}

impl Collider for BoxCollider {
    const NAME: &str = "BoxCollider";

    fn aabb(&self, world_transform: &Transform, _: &VoxelColliderRegistry) -> Option<AABB> {
        Some(world_transform.transform_obb(&self.obb).bounding_aabb())
    }

    fn render_debug(
        &self,
        world_transform: &Transform,
        debug_renderer: &mut DebugRenderer,
        coloring: ColliderDebugColoring,
    ) {
        //debug_renderer.draw_obb(DebugOBB {
        //    obb: &world_transform.transform_obb(&self.obb),
        //    thickness: 0.025,
        //    color: coloring.color(),
        //    alpha: 0.75,
        //    flags: DebugFlags::XRAY,
        //});
        //debug_renderer.draw_obb(DebugOBB {
        //    obb: &world_transform
        //        .transform_obb(&self.obb)
        //        .bounding_aabb()
        //        .as_obb(),
        //    thickness: 0.025,
        //    color: Color::new_srgb_hex("#55AAB3"),
        //    alpha: 0.75,
        //    flags: DebugFlags::XRAY,
        //});
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
