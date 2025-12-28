use crate::consts;
use crate::engine::asset::asset::{AssetLoader, GameAssetPath};
use crate::engine::voxel::voxel_registry::VoxelModelId;
use crate::{common::morton, engine::world::region_map::RegionPos};
use core::panic;
use nalgebra::Vector3;
use std::{assert, todo};

pub type ChunkPos = Vector3<i32>;

#[derive(Clone)]
pub struct WorldRegion {
    pub region_pos: RegionPos,

    pub root_node: Box<WorldRegionNode>,
    pub model_handles: Vec<VoxelModelId>,
}

impl WorldRegion {
    pub fn new_empty(region_pos: Vector3<i32>) -> Self {
        Self {
            region_pos,
            root_node: Box::new(WorldRegionNode::new_empty()),
            model_handles: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct WorldRegionNode {
    /// Most signifigant bit determines if this pointer,
    /// actually points to a voxel model handle. If this is
    /// u32::MAX then this node is empty.
    child_ptr: u32,
    child_mask: u64,
}

impl WorldRegionNode {
    pub fn new_empty() -> Self {
        Self {
            child_ptr: u32::MAX,
            child_mask: 0,
        }
    }
}
