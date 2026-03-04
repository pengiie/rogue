use std::collections::HashSet;

use crate::common::freelist::FreeListHandle;
use crate::voxel::voxel_registry::VoxelModelId;
use crate::world::region_map::{ChunkLOD, RegionPos};
use nalgebra::Vector3;

pub struct WorldRegion {
    pub tree: RegionTree,
    pub model_handles: Vec<VoxelModelId>,
    /// Acts as our refcount, points to leaves/pseudo-leaves which have their model
    /// loaded, they may also not have a model but still count as loaded since we know their
    /// representation.
    pub active_leaves: HashSet<u32>,
    pub ref_count: u32,
}

impl WorldRegion {
    pub fn new_empty() -> Self {
        let mut active_leaves = HashSet::new();
        active_leaves.insert(0);
        Self {
            tree: RegionTree::new_empty(),
            model_handles: Vec::new(),
            active_leaves,
            ref_count: 0,
        }
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
