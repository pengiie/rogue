use std::collections::HashSet;

use crate::common::freelist::FreeListHandle;
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::ray::{Ray, RayAABBHitInfo};
use crate::common::morton;
use crate::consts;
use crate::voxel::voxel_registry::{VoxelModelId, VoxelModelRegistry};
use crate::world::terrain::region_map::{ChunkLOD, RegionPos, TerrainRaycastHit};
use nalgebra::Vector3;

pub struct WorldRegion {
    pub tree: RegionTree,
    pub model_handles: Vec<VoxelModelId>,
    /// Acts as our refcount, points to leaves/pseudo-leaves which have their model
    /// loaded, they may also not have a model but still count as loaded since we know their
    /// representation.
    pub active_leaves: HashSet<u32>,
    pub ref_count: u32,
    pub region_pos: RegionPos,
}

impl WorldRegion {
    pub fn new_empty(pos: RegionPos) -> Self {
        let mut active_leaves = HashSet::new();
        active_leaves.insert(0);
        Self {
            tree: RegionTree::new_empty(),
            model_handles: Vec::new(),
            active_leaves,
            ref_count: 0,
            region_pos: pos,
        }
    }

    pub fn in_bounds_local(&self, chunk_pos: Vector3<i32>) -> bool {
        return chunk_pos.x >= 0
            && chunk_pos.y >= 0
            && chunk_pos.z >= 0
            && chunk_pos.x < consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32
            && chunk_pos.y < consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32
            && chunk_pos.z < consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32;
    }

    pub fn raycast_region(
        &self,
        voxel_registry: &VoxelModelRegistry,
        in_ray: &Ray,
        max_t: f32,
    ) -> Option<TerrainRaycastHit> {
        //
        // Most of this code is similar to SFTCompressed::trace but follows same logic as
        // the region in terrain.slang
        //
        let mut ray = in_ray.clone();
        let min = self.region_pos.cast::<f32>() * consts::voxel::TERRAIN_REGION_METER_LENGTH;
        let max = min.map(|x| x + consts::voxel::TERRAIN_REGION_METER_LENGTH);
        let aabb = &AABB::new_two_point(min, max);
        // Early exit if the ray doesn't intersect the bounding box of this model.
        let Some(RayAABBHitInfo {
            t_enter: model_t, ..
        }) = ray.intersect_aabb(aabb)
        else {
            return None;
        };
        ray.advance(model_t);

        // DDA through the 4x4x4 nodes at varying step sizes depending on how far we
        // are down the tree. While we DDA we check if the current child exists, if it is
        // a node we push onto the traversal stack of the last node index we were at and
        // decrease our step size by a fourth. Effectively doing DDA on that child node now.
        // If if a leaf we calculate the voxel position dcepending on the leaf node intersection
        // and return with that.
        let local_pos = ray.origin - aabb.min;
        let norm_pos = local_pos.zip_map(&aabb.side_length(), |x, y| (x / y).clamp(0.0, 0.9999));
        // Our scaled position from [0, bounds).
        let dda_pos = norm_pos * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as f32;

        let height = consts::voxel::TERRAIN_REGION_TREE_HEIGHT - 1;
        let sl = consts::voxel::TERRAIN_REGION_CHUNK_LENGTH;
        let quarter_sl = consts::voxel::TERRAIN_REGION_CHUNK_LENGTH >> 2;
        let unit_grid = ray.dir.map(|x| x.signum() as i32);

        let mut last_mask = Vector3::zeros();
        let mut curr_ray = Ray::new(dda_pos, ray.dir);
        let mut curr_height = 0;
        let mut curr_local_grid = curr_ray
            .origin
            .map(|x| (x.floor() as u32 >> (height * 2)) as i32);
        let mut curr_anchor = Vector3::<u32>::zeros();
        // Don't include the leaf layer in the height.
        let mut stack = vec![&self.tree.nodes[0]];
        let mut i = 0;
        let mut was_last_leaf = false;
        while self.in_bounds_local(curr_ray.origin.map(|x| x.floor() as i32))
            && (curr_ray.origin.metric_distance(&dda_pos)
                * consts::voxel::TERRAIN_CHUNK_METER_LENGTH)
                < max_t
        {
            assert!(i < 10000, "Shouldn't ever iterate over 10k times.");
            i += 1;
            let should_pop = was_last_leaf
                || curr_local_grid.x < 0
                || curr_local_grid.y < 0
                || curr_local_grid.z < 0
                || curr_local_grid.x > 3
                || curr_local_grid.y > 3
                || curr_local_grid.z > 3;
            if should_pop {
                if curr_height == 0 {
                    break;
                }
                stack.pop().unwrap();
                curr_height -= 1;
                curr_local_grid =
                    curr_anchor.map(|x| ((x >> ((height - curr_height) * 2)) & 3) as i32);
                curr_anchor = curr_anchor.map(|x| {
                    (x >> ((height - curr_height + 1) * 2)) << ((height - curr_height + 1) * 2)
                });
            } else {
                let curr_node = stack.last().unwrap();
                if (curr_height != consts::voxel::TERRAIN_REGION_TREE_HEIGHT) {
                    let child_index =
                        morton::morton_encode(curr_local_grid.map(|x| x as u32)) as u32;
                    let is_child_present = (curr_node.child_mask & (1 << child_index)) > 0;
                    if is_child_present {
                        let node_size = quarter_sl >> (curr_height * 2);
                        curr_anchor =
                            curr_anchor.zip_map(&curr_local_grid, |x, y| x + y as u32 * node_size);
                        let global_grid_pos = curr_ray.origin.zip_map(&curr_anchor, |x, y| {
                            (x.floor() as u32).clamp(y, y + node_size - 1)
                        });

                        assert!(curr_node.child_ptr != u32::MAX);
                        let next_node_index = (curr_node.child_ptr + child_index) as usize;
                        stack.push(&self.tree.nodes[next_node_index]);

                        curr_height += 1;
                        curr_local_grid = global_grid_pos.map(|x| {
                            ((x >> (height.saturating_sub(curr_height) * 2)) & 0b11) as i32
                        });
                        continue;
                    }
                } else {
                    if let Some(model_ptr) = curr_node.model_ptr() {
                        let model_id = self.model_handles[model_ptr as usize];
                        let min = aabb.min
                            + curr_anchor.cast::<f32>() * consts::voxel::TERRAIN_CHUNK_METER_LENGTH;
                        let max = min.map(|x| x + consts::voxel::TERRAIN_CHUNK_METER_LENGTH);
                        let chunk_aabb = &AABB::new_two_point(min, max);
                        if let Some(model_trace) = voxel_registry
                            .get_dyn_model(model_id)
                            .trace(&in_ray, chunk_aabb)
                        {
                            let world_voxel_pos = (*self.region_pos.into_chunk_pos()
                                + curr_anchor.cast::<i32>())
                                * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32
                                + model_trace.local_position.cast::<i32>();
                            return Some(TerrainRaycastHit {
                                world_voxel_pos,
                                model_trace,
                            });
                        }
                    }
                    let next_point = curr_anchor + unit_grid.map(|x| x.max(0) as u32);
                    let curr_t = curr_ray.intersect_point(next_point.cast::<f32>());
                    let next_t = curr_t.min();
                    curr_ray.advance(next_t + 0.00001);
                    was_last_leaf = true;
                    continue;
                }
            }

            let node_size = quarter_sl >> (curr_height * 2);
            let next_point = curr_anchor
                + (curr_local_grid.map(|x| x as u32) + unit_grid.map(|x| x.max(0) as u32))
                    * node_size;
            let next_t = curr_ray.intersect_point(next_point.cast::<f32>());
            let min_t = next_t.min();
            let mask = next_t.map(|x| if x == min_t { 1 } else { 0 });
            last_mask = mask;

            curr_local_grid += unit_grid.component_mul(&mask);
            // Epsilon since sometimes we advance out of bounds but due to fp math it's just barely
            // off, messing up the traversal.
            curr_ray.advance(min_t + 0.0001);
        }

        return None;
    }

    pub fn set_leaf_active(&mut self, node_handle: u32) {}
}

#[derive(Clone)]
pub struct RegionTree {
    pub nodes: Vec<WorldRegionNode>,
}

impl RegionTree {
    pub fn new_empty() -> Self {
        Self {
            nodes: vec![WorldRegionNode::new_empty(u32::MAX)],
        }
    }

    /// Creates a region tree from an morton encoded array of handles
    pub fn from_array(model_handles: Vec<u32>, lod: ChunkLOD) -> Self {
        let length = lod.region_chunk_length();
        assert_eq!(model_handles.len() as u32, length * length * length);
        let height = lod.max_tree_height() as usize;

        let mut nodes = Vec::new();
        let mut levels = vec![Vec::new(); height + 1];
        for handle in model_handles.iter() {
            levels[height as usize].push(WorldRegionNode {
                model_ptr: *handle,
                parent_ptr: u32::MAX,
                child_ptr: u32::MAX,
                child_mask: 0,
            });

            for h in (1..=height).rev() {
                if levels[h].len() != 64 {
                    break;
                }

                let non_empty_children =
                    levels[h].iter().filter(|node| node.has_model_ptr()).count() as u32;
                let parent_ptr = nodes.len() as u32 + non_empty_children;
                let mut parent_node = WorldRegionNode::new_empty(u32::MAX);
                let mut child_ptr = u32::MAX;
                for i in (0..64).rev() {
                    let child_node = &levels[h][i];
                    if child_node.model_ptr != u32::MAX {
                        parent_node.child_mask |= 1 << i;
                        child_ptr = nodes.len() as u32;
                        let mut child_node = child_node.clone();
                        child_node.parent_ptr = parent_ptr;
                        nodes.push(child_node.clone());
                    }
                }
                parent_node.child_ptr = child_ptr;
                levels[h - 1].push(parent_node);
                levels[h].clear();
            }
        }
        nodes.push(levels[0][0].clone());
        nodes.reverse();

        Self { nodes }
    }

    pub fn set(chunk: Vector3<u32>, lod: ChunkLOD, model_handle: u32) {
        todo!()
    }

    pub fn lod_loaded(&self, lod: ChunkLOD) -> Self {
        todo!("Traverse to maek sure any leaves at this lod are loaded.")
    }
}

#[derive(Clone)]
pub struct WorldRegionNode {
    pub model_ptr: u32,
    pub parent_ptr: u32,
    pub child_ptr: u32,
    pub child_mask: u64,
}

impl WorldRegionNode {
    pub fn new_empty(parent_ptr: u32) -> Self {
        Self {
            model_ptr: u32::MAX,
            parent_ptr,
            child_ptr: u32::MAX,
            child_mask: 0,
        }
    }

    pub fn has_model_ptr(&self) -> bool {
        self.model_ptr != u32::MAX
    }

    pub fn model_ptr(&self) -> Option<u32> {
        (self.has_model_ptr()).then_some(self.model_ptr)
    }

    pub fn is_child_ptr_valid(&self) -> bool {
        self.child_ptr != u32::MAX
    }

    pub fn child_ptr(&self) -> Option<u32> {
        if self.is_child_ptr_valid() {
            Some(self.child_ptr)
        } else {
            None
        }
    }

    pub fn is_parent_ptr_valid(&self) -> bool {
        self.parent_ptr != u32::MAX
    }
}
