use std::ops::Rem;

use nalgebra::Vector3;

use crate::engine::physics::collider::{ContactManifold, ContactPair, ContactPoint};

pub type Vertex = Vector3<f32>;
pub struct Face {
    vertices: Vec<Vertex>,
    normal: Vector3<f32>,
}

pub struct Edge {
    v1: Vector3<f32>,
    v2: Vector3<f32>,
}

impl Face {
    pub fn new(vertices: Vec<Vertex>) -> Self {
        // TODO: Ensure all vertices are coplanar.
        let normal = (vertices[1] - vertices[0])
            .cross(&(vertices[2] - vertices[0]))
            .normalize();

        Self { vertices, normal }
    }

    pub fn collect_edges(&self) -> Vec<Edge> {
        let mut edges = Vec::new();
        for i in 0..self.vertices.len() {
            let v1 = self.vertices[i];
            let v2 = self.vertices[(i + 1) % self.vertices.len()];
            edges.push(Edge { v1, v2 });
        }
        edges
    }

    /// Calculates the separating axis theorem axes to test for each face
    pub fn calculate_sat_axes(&self) -> Vec<Vector3<f32>> {
        let mut axes = vec![self.normal];
        for i in 0..self.vertices.len() {
            let v1 = self.vertices[i];
            let v2 = self.vertices[(i + 1) % self.vertices.len()];
            axes.push((v2 - v1).cross(&self.normal).normalize());
        }
        axes
    }
}

pub struct Projection {
    pub min: f32,
    pub max: f32,
}

impl Projection {
    pub fn overlap(&self, other: &Projection) -> bool {
        self.min <= other.max && self.max >= other.min
    }
}

enum SATRefShape {
    ShapeA,
    ShapeB,
}
enum SATAxis {
    Face {
        normal: Vector3<f32>,
        shape: SATRefShape,
        face_idx: u32,
    },
    Edge {
        axis: Vector3<f32>,
        edge_idx_a: u32,
        edge_idx_b: u32,
    },
}

impl SATAxis {
    pub fn axis(&self) -> Vector3<f32> {
        match self {
            SATAxis::Face { normal, .. } => *normal,
            SATAxis::Edge { axis, .. } => *axis,
        }
    }
}

pub trait Shape {
    fn collect_vertices(&self) -> Vec<Vertex>;
    fn collect_faces(&self) -> Vec<Face>;

    /// Projects all of this shapes vertices onto the given axis and figures out the
    /// min and max of the projection.
    fn project(&self, axis: &Vector3<f32>) -> Projection {
        let mut min = std::f32::MAX;
        let mut max = std::f32::MIN;

        for vertex in self.collect_vertices() {
            let projection = vertex.dot(&axis);
            min = min.min(projection);
            max = max.max(projection);
        }

        Projection { min, max }
    }

    /// SAT intersection test.
    fn test_intersection(&self, other: &dyn Shape) -> Option<ContactManifold> {
        // Collect all axes we have to test
        // - The cross product of all edges in shape A with all edges in shape B.
        // - The normal of each face
        let faces_a = self.collect_faces();
        let faces_b = other.collect_faces();
        let mut sat_axes = Vec::new();

        for (i, face) in faces_a.iter().enumerate() {
            sat_axes.push(SATAxis::Face {
                normal: face.normal,
                shape: SATRefShape::ShapeA,
                face_idx: i as u32,
            })
        }
        for (i, face) in faces_b.iter().enumerate() {
            sat_axes.push(SATAxis::Face {
                normal: face.normal,
                shape: SATRefShape::ShapeB,
                face_idx: i as u32,
            })
        }

        let edges_a = faces_a
            .iter()
            .flat_map(|face| face.collect_edges())
            .collect::<Vec<_>>();
        let edges_b = faces_b
            .iter()
            .flat_map(|face| face.collect_edges())
            .collect::<Vec<_>>();
        for (idx_a, edge_a) in edges_a.iter().enumerate() {
            for (idx_b, edge_b) in edges_b.iter().enumerate() {
                let axis_a = edge_a.v2 - edge_a.v1;
                let axis_b = edge_b.v2 - edge_b.v1;
                let mut axis = axis_a.cross(&axis_b).normalize();
                sat_axes.push(SATAxis::Edge {
                    axis,
                    edge_idx_a: idx_a as u32,
                    edge_idx_b: idx_b as u32,
                });
            }
        }

        let vertices_a = self.collect_vertices();
        let vertices_b = other.collect_vertices();

        // Determine the minimum penetrating axis.
        let mut penetration_axis = None;
        let mut max_depth = 10000.0;
        for axis in sat_axes {
            let p1 = {
                let mut min = std::f32::MAX;
                let mut max = std::f32::MIN;

                for vertex in &vertices_a {
                    let projection = vertex.dot(&axis.axis());
                    min = min.min(projection);
                    max = max.max(projection);
                }

                Projection { min, max }
            };
            let p2 = {
                let mut min = std::f32::MAX;
                let mut max = std::f32::MIN;

                for vertex in &vertices_b {
                    let projection = vertex.dot(&axis.axis());
                    min = min.min(projection);
                    max = max.max(projection);
                }

                Projection { min, max }
            };

            // If the projection of both of the shapes onto this axis don't overlap,
            // then by SAT they are not intersecting since there is a separating axis.
            if !p1.overlap(&p2) {
                return None;
            }

            let overlap = (p1.min - p2.max).max(p1.max - p2.min);
            if overlap < max_depth {
                max_depth = overlap;
                penetration_axis = Some(axis);
            }
        }

        let penetration_axis = penetration_axis.unwrap().axis();
        let center_a = vertices_a.iter().fold(Vector3::zeros(), |acc, v| acc + v)
            / (self.collect_vertices().len() as f32);
        let center_b = vertices_b.iter().fold(Vector3::zeros(), |acc, v| acc + v)
            / (other.collect_vertices().len() as f32);
        // Ensure the normal is pointing from A to B.
        let mut penetration_normal = penetration_axis.normalize();
        if (center_b - center_a).dot(&penetration_normal) < 0.0 {
            penetration_normal = -penetration_normal;
        }

        // Find the reference face and incident face depending on the normal.
        let reference_face = faces_a
            .iter()
            .min_by(|a, b| {
                let da = a.normal.dot(&penetration_normal);
                let db = b.normal.dot(&penetration_normal);
                da.partial_cmp(&db).unwrap()
            })
            .unwrap();
        let incident_face = faces_b
            .iter()
            .min_by(|a, b| {
                let da = a.normal.dot(&-penetration_normal);
                let db = b.normal.dot(&-penetration_normal);
                da.partial_cmp(&db).unwrap()
            })
            .unwrap();

        // Use Sutherland-Hodgman clipping to clip the incident face (subject polygon)
        // against the reference face (clipping polygon).
        let mut clipped_points = incident_face.vertices.clone();
        for clip_edge in reference_face.collect_edges() {
            // Clip reference face vertices against this edge.
            let mut input_list = Vec::new();
            std::mem::swap(&mut input_list, &mut clipped_points);

            for i in 0..input_list.len() {
                let curr_point = input_list[i];
                let last_i = (i as i32 - 1).rem_euclid(input_list.len() as i32) as usize;
                let prev_point = input_list[last_i];

                let edge_dir = clip_edge.v2 - clip_edge.v1;
                let edge_normal = edge_dir.cross(&reference_face.normal).normalize();

                let curr_inside_edge = (curr_point - clip_edge.v1).dot(&edge_normal) <= 0.0;
                let prev_inside_edge = (prev_point - clip_edge.v1).dot(&edge_normal) <= 0.0;

                // Intersection against clipping plane (clip edge extended along incident face
                // normal).
                // https://en.wikipedia.org/wiki/Line%E2%80%93plane_intersection
                let intersection_point = {
                    let line_dir = curr_point - prev_point;
                    let denom = edge_normal.dot(&line_dir);
                    let t = (clip_edge.v1 - prev_point).dot(&edge_normal) / denom;
                    prev_point + line_dir * t
                };

                if curr_inside_edge {
                    if !prev_inside_edge {
                        clipped_points.push(intersection_point);
                    }
                    clipped_points.push(curr_point);
                } else if prev_inside_edge {
                    clipped_points.push(intersection_point);
                }
            }
        }

        let contact_points = clipped_points
            .into_iter()
            .filter_map(|pt| {
                // Only keep contact points behind the reference face.
                let dir = reference_face.vertices[0] - pt;
                let distance = dir.dot(&reference_face.normal);
                if distance > 0.0 {
                    return None;
                }
                Some(ContactPoint {
                    position: pt,
                    distance,
                    normal_impulse: 0.0,
                    tangent_impulse: 0.0,
                })
            })
            .collect::<Vec<_>>();

        Some(ContactManifold {
            points: contact_points,
            normal: penetration_axis,
        })
    }
}

#[cfg(test)]
mod tests {
    use nalgebra::Point3;

    use crate::common::geometry::aabb::AABB;

    use super::*;

    #[test]
    fn test_aabb_intersection() {
        let aabb_1 =
            AABB::new_center_extents(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0));
        let aabb_2 =
            AABB::new_center_extents(Vector3::new(0.5, 0.5, 0.5), Vector3::new(1.0, 1.0, 1.0));

        assert!(aabb_1.test_intersection(&aabb_2).is_some());
    }
}
