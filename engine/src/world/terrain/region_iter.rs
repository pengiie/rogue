use nalgebra::{Vector2, Vector3};

use crate::world::terrain::region_map::RegionPos;

/// Iterator that starts from a center and expands outwards for regions specifically.
pub struct RegionIter {
    max_radius: u32,
    curr_radius: u32,
    curr_index: u32,
    // The center region which is iterated around.
    region_center: RegionPos,
}

impl RegionIter {
    pub fn new(region_center: RegionPos, render_distance: u32) -> Self {
        Self {
            max_radius: render_distance,
            curr_radius: 0,
            curr_index: 0,
            region_center,
        }
    }

    pub fn reset(&mut self) {
        self.curr_radius = 0;
        self.curr_index = 0;
    }

    pub fn update_max_radius(&mut self, new_max_radius: u32) {
        self.max_radius = new_max_radius;
        if self.max_radius < self.curr_radius {
            self.curr_radius = self.max_radius;
        }
        // TODO: Remove and do the transition nicer with the renderable chunks following.
        self.reset();
    }

    pub fn max_radius(&self) -> u32 {
        self.max_radius
    }

    pub fn curr_radius(&self) -> u32 {
        self.curr_radius
    }

    pub fn curr_index(&self) -> u32 {
        self.curr_index
    }

    pub fn max_index(&self) -> u32 {
        let curr_diameter = (self.curr_radius + 1) * 2;
        let curr_area = curr_diameter.pow(2);
        return curr_area * 6;
    }

    pub fn update_anchor(&mut self, new_region_center: RegionPos) {
        if new_region_center == self.region_center {
            return;
        }

        let distance = ((new_region_center - self.region_center).abs().max()) as u32;
        self.curr_radius = self.curr_radius.saturating_sub(distance);
        self.curr_index = 0;
        self.region_center = new_region_center;
    }

    /// Enqueues chunks in an iterator fashion so we don't waste time rechecking chunks.
    pub fn next_region(&mut self) -> Option<RegionPos> {
        if self.curr_radius == self.max_radius {
            return None;
        }

        let curr_diameter = self.curr_radius * 2 + 1;
        let curr_area = curr_diameter.pow(2);
        if self.curr_index >= curr_area * 6 {
            self.curr_radius += 1;
            self.curr_index = 0;
            return self.next_region();
        }

        let face = self.curr_index / curr_area;
        let local_index = self.curr_index % curr_area;
        let local_position = Vector2::new(
            (local_index % curr_diameter) as i32,
            (local_index / curr_diameter) as i32,
        );
        let mut region_position =
            self.region_center - Vector3::new(1, 1, 1) * self.curr_radius as i32;
        match face {
            // Bottom Face
            0 => region_position += Vector3::new(local_position.x, 0, local_position.y),
            // Top Face
            1 => {
                region_position +=
                    Vector3::new(local_position.x, curr_diameter as i32 - 1, local_position.y)
            }
            // Front Face
            2 => region_position += Vector3::new(local_position.x, local_position.y, 0),
            // Back Face
            3 => {
                region_position +=
                    Vector3::new(local_position.x, local_position.y, curr_diameter as i32 - 1)
            }
            // Left Face
            4 => region_position += Vector3::new(0, local_position.x, local_position.y),
            // Right Face
            5 => {
                region_position +=
                    Vector3::new(curr_diameter as i32 - 1, local_position.x, local_position.y)
            }
            _ => unreachable!(),
        }

        self.curr_index += 1;
        return Some(region_position.into());
    }
}
