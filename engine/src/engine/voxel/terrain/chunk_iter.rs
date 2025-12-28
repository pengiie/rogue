use nalgebra::{Vector2, Vector3};

pub struct ChunkIter {
    max_radius: u32,
    curr_radius: u32,
    curr_index: u32,
    /// Anchor is in the center with the iterator iterating around.
    current_chunk_anchor: Vector3<i32>,
}

impl ChunkIter {
    pub fn new(chunk_anchor: Vector3<i32>, render_distance: u32) -> Self {
        Self {
            max_radius: render_distance,
            curr_radius: 0,
            curr_index: 0,
            current_chunk_anchor: chunk_anchor,
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

    pub fn update_anchor(&mut self, new_chunk_anchor: Vector3<i32>) {
        if new_chunk_anchor == self.current_chunk_anchor {
            return;
        }

        let distance = ((new_chunk_anchor - self.current_chunk_anchor).abs().max()) as u32;
        self.curr_radius = self.curr_radius.saturating_sub(distance);
        self.curr_index = 0;
        self.current_chunk_anchor = new_chunk_anchor;
    }

    /// Enqueues chunks in an iterator fashion so we don't waste time rechecking chunks.
    pub fn next_chunk(&mut self) -> Option<Vector3<i32>> {
        if self.curr_radius == self.max_radius {
            return None;
        }

        let curr_diameter = (self.curr_radius + 1) * 2;
        let curr_area = curr_diameter.pow(2);
        if self.curr_index >= curr_area * 6 {
            self.curr_radius += 1;
            self.curr_index = 0;
            return None;
        }

        let face = self.curr_index / curr_area;
        let local_index = self.curr_index % curr_area;
        let local_position = Vector2::new(
            (local_index % curr_diameter) as i32,
            (local_index / curr_diameter) as i32,
        );
        let mut chunk_position =
            self.current_chunk_anchor - Vector3::new(1, 1, 1) * (self.curr_radius + 1) as i32;
        match face {
            // Bottom Face
            0 => chunk_position += Vector3::new(local_position.x, 0, local_position.y),
            // Top Face
            1 => {
                chunk_position +=
                    Vector3::new(local_position.x, curr_diameter as i32 - 1, local_position.y)
            }
            // Front Face
            2 => chunk_position += Vector3::new(local_position.x, local_position.y, 0),
            // Back Face
            3 => {
                chunk_position +=
                    Vector3::new(local_position.x, local_position.y, curr_diameter as i32 - 1)
            }
            // Left Face
            4 => chunk_position += Vector3::new(0, local_position.x, local_position.y),
            // Right Face
            5 => {
                chunk_position +=
                    Vector3::new(curr_diameter as i32 - 1, local_position.x, local_position.y)
            }
            _ => unreachable!(),
        }

        self.curr_index += 1;
        return Some(chunk_position);
    }
}
