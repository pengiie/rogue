use super::transform::Transform;
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::obb::OBB;
use crate::common::{color::Color, geometry::shape::Shape};
use crate::debug::{DebugFlags, DebugOBB, DebugRenderer};
use crate::egui::util::{position_ui, rotation_ui, scale_ui};
use crate::physics::collider::{Collider, ColliderDebugColoring, ContactManifold};
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
    //let axes_a = vec![box_a.forward(), box_a.up(), box_a.right()];
    //let axes_b = vec![box_b.forward(), box_b.up(), box_b.right()];
    //let vertices_a = box_a.collect_vertices();
    //let vertices_b = box_b.collect_vertices();

    //// Collect SAT axes.
    //let mut test_axes = Vec::with_capacity(15);
    //test_axes.extend(axes_a.iter());
    //test_axes.extend(axes_b.iter());
    //for axis_a in &axes_a {
    //    for axis_b in &axes_b {
    //        test_axes.push(axis_a.cross(&axis_b));
    //    }
    //}

    //// Determine the minimum penetrating axis.
    //let mut penetration_axis = None;
    //let mut max_depth = 10000.0;
    //for axis in test_axes {
    //    let p1 = {
    //        let mut min = std::f32::MAX;
    //        let mut max = std::f32::MIN;

    //        for vertex in &vertices_a {
    //            let projection = vertex.dot(&axis);
    //            min = min.min(projection);
    //            max = max.max(projection);
    //        }

    //        Projection { min, max }
    //    };
    //    let p2 = {
    //        let mut min = std::f32::MAX;
    //        let mut max = std::f32::MIN;

    //        for vertex in &vertices_b {
    //            let projection = vertex.dot(&axis);
    //            min = min.min(projection);
    //            max = max.max(projection);
    //        }

    //        Projection { min, max }
    //    };

    //    // If the projection of both of the shapes onto this axis don't overlap,
    //    // then by SAT they are not intersecting since there is a separating axis.
    //    if !p1.overlap(&p2) {
    //        return None;
    //    }

    //    let overlap = (p1.min - p2.max).max(p1.max - p2.min);
    //    if overlap < max_depth {
    //        max_depth = overlap;
    //        penetration_axis = Some(axis);
    //    }
    //}

    //let center_a = box_a.aabb.center();
    //let center_b = box_b.aabb.center();

    //// Ensure the normal is pointing from A to B.
    //let mut penetration_normal = penetration_axis.unwrap().normalize();
    //if (center_b - center_a).dot(&penetration_normal) < 0.0 {
    //    penetration_normal = -penetration_normal;
    //}

    //let (reference_face, from_a) = {
    //    let (face_a, dot_a) = box_a.most_aligned_face_normal(&penetration_normal);
    //    let (face_b, dot_b) = box_b.most_aligned_face_normal(&-penetration_normal);
    //    if dot_a.abs() > dot_b.abs() {
    //        (box_a.face_from_local_axis(&face_a), true)
    //    } else {
    //        (box_b.face_from_local_axis(&face_b), false)
    //    }
    //};

    //let incident_face = {
    //    if from_a {
    //        box_b.face_from_local_axis(&box_b.most_opposite_face_normal(&reference_face.normal()).0)
    //    } else {
    //        box_a.face_from_local_axis(&box_a.most_opposite_face_normal(&reference_face.normal()).0)
    //    }
    //};

    //// Use Sutherland-Hodgman clipping to clip the incident face (subject polygon)
    //// against the reference face (clipping polygon).
    //let mut clipped_points = incident_face.vertices.clone();
    //for clip_edge in reference_face.collect_edges() {
    //    // Clip reference face vertices against this edge.
    //    let mut input_list = Vec::new();
    //    std::mem::swap(&mut input_list, &mut clipped_points);

    //    for i in 0..input_list.len() {
    //        let curr_point = input_list[i];
    //        let last_i = (i as i32 - 1).rem_euclid(input_list.len() as i32) as usize;
    //        let prev_point = input_list[last_i];

    //        let edge_dir = clip_edge.v2 - clip_edge.v1;
    //        let edge_normal = edge_dir.cross(&reference_face.normal).normalize();

    //        let curr_inside_edge = (curr_point - clip_edge.v1).dot(&edge_normal) <= 0.0;
    //        let prev_inside_edge = (prev_point - clip_edge.v1).dot(&edge_normal) <= 0.0;

    //        // Intersection against clipping plane (clip edge extended along incident face
    //        // normal).
    //        // https://en.wikipedia.org/wiki/Line%E2%80%93plane_intersection
    //        let intersection_point = {
    //            let line_dir = curr_point - prev_point;
    //            let denom = edge_normal.dot(&line_dir);
    //            let t = (clip_edge.v1 - prev_point).dot(&edge_normal) / denom;
    //            prev_point + line_dir * t
    //        };

    //        if curr_inside_edge {
    //            if !prev_inside_edge {
    //                clipped_points.push(intersection_point);
    //            }
    //            clipped_points.push(curr_point);
    //        } else if prev_inside_edge {
    //            clipped_points.push(intersection_point);
    //        }
    //    }
    //}

    //let contact_points = clipped_points
    //    .into_iter()
    //    .filter_map(|pt| {
    //        // Only keep contact points behind the reference face.
    //        let dir = reference_face.vertices[0] - pt;
    //        let distance = dir.dot(&reference_face.normal);
    //        if distance > 0.0 {
    //            return None;
    //        }
    //        Some(ContactPoint {
    //            position: pt,
    //            distance,
    //            normal_impulse: 0.0,
    //            tangent_impulse: 0.0,
    //        })
    //    })
    //    .collect::<Vec<_>>();

    //Some(ContactManifold {
    //    points: contact_points,
    //    normal: penetration_normal,
    //})
}

impl Collider for BoxCollider {
    const NAME: &str = "BoxCollider";

    fn aabb(&self, world_transform: &Transform) -> AABB {
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
