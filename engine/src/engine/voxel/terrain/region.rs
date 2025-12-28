use crate::common::morton;
use crate::consts;
use crate::engine::voxel::voxel_registry::VoxelModelId;
use core::panic;
use nalgebra::Vector3;
use std::{assert, todo};

pub enum VoxelChunkRegion {
    Loading,
    Data(VoxelChunkRegionData),
}

#[derive(Clone)]
pub struct VoxelChunkRegionData {
    pub region_pos: Vector3<i32>,
    pub region_chunk_anchor: Vector3<i32>,

    pub root_node: Box<VoxelChunkRegionNode>,
}

impl VoxelChunkRegionData {
    pub fn empty(region_pos: Vector3<i32>) -> Self {
        assert!(consts::voxel::TERRAIN_REGION_TREE_HEIGHT > 0);
        let root_node = if consts::voxel::TERRAIN_REGION_TREE_HEIGHT == 1 {
            VoxelChunkRegionNode::new_preleaf()
        } else {
            VoxelChunkRegionNode::new_internal()
        };

        Self {
            region_pos,
            region_chunk_anchor: region_pos * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32,
            root_node: Box::new(root_node),
        }
    }

    pub fn get_chunk_traversal(&self, world_chunk_pos: &Vector3<i32>) -> u64 {
        let local_chunk_pos = (world_chunk_pos - self.region_chunk_anchor).map(|x| x as u32);

        return morton::morton_traversal_octree(
            morton::morton_encode(local_chunk_pos),
            consts::voxel::TERRAIN_REGION_TREE_HEIGHT,
        );
    }

    pub fn get_chunk(&self, world_chunk_pos: &Vector3<i32>) -> &VoxelRegionLeafNode {
        let mut traversal = self.get_chunk_traversal(world_chunk_pos);
        let mut curr_node = self.root_node.as_ref();
        loop {
            let curr_child_idx = traversal as usize & 0b111;
            match curr_node {
                VoxelChunkRegionNode::Internal(child_nodes) => {
                    if let Some(child_node) = &child_nodes[curr_child_idx] {
                        curr_node = child_node.as_ref();
                        traversal = traversal >> 3;
                    } else {
                        return &const { VoxelRegionLeafNode::Empty };
                    }
                }
                VoxelChunkRegionNode::Preleaf(leaf_nodes) => {
                    return &leaf_nodes[curr_child_idx];
                }
            }
        }
    }

    pub fn get_existing_chunk_mut(
        &mut self,
        world_chunk_pos: &Vector3<i32>,
    ) -> Option<&mut VoxelRegionLeafNode> {
        let mut traversal = self.get_chunk_traversal(world_chunk_pos);
        let mut curr_node = self.root_node.as_mut();
        loop {
            let curr_child_idx = traversal as usize & 0b111;
            match curr_node {
                VoxelChunkRegionNode::Internal(child_nodes) => {
                    if let Some(child_node) = &mut child_nodes[curr_child_idx] {
                        curr_node = child_node.as_mut();
                        traversal = traversal >> 3;
                    } else {
                        return None;
                    }
                }
                VoxelChunkRegionNode::Preleaf(leaf_nodes) => {
                    let node = &mut leaf_nodes[curr_child_idx];
                    match node {
                        VoxelRegionLeafNode::Empty => return None,
                        VoxelRegionLeafNode::Existing { uuid, model } => {}
                    }
                    return Some(node);
                }
            }
        }
    }

    pub fn get_or_create_chunk_mut(
        &mut self,
        world_chunk_pos: &Vector3<i32>,
    ) -> &mut VoxelRegionLeafNode {
        let mut traversal = self.get_chunk_traversal(world_chunk_pos);
        let mut curr_node = self.root_node.as_mut();
        let mut curr_height = 0;
        loop {
            let curr_child_idx = traversal as usize & 0b111;
            match curr_node {
                VoxelChunkRegionNode::Internal(child_nodes) => {
                    if child_nodes[curr_child_idx].is_none() {
                        let new_node =
                            if curr_height + 2 < consts::voxel::TERRAIN_REGION_TREE_HEIGHT {
                                Box::new(VoxelChunkRegionNode::new_internal())
                            } else {
                                Box::new(VoxelChunkRegionNode::new_preleaf())
                            };
                        child_nodes[curr_child_idx] = Some(new_node);
                    }
                    curr_node = child_nodes[curr_child_idx].as_mut().unwrap().as_mut();
                    traversal = traversal >> 3;
                }
                VoxelChunkRegionNode::Preleaf(leaf_nodes) => {
                    return &mut leaf_nodes[curr_child_idx]
                }
            }
            curr_height += 1;
        }
    }

    pub fn set_chunk(&self, world_chunk_pos: &Vector3<i32>, data: VoxelRegionLeafNode) {
        todo!("impl set_chunk")
    }

    pub fn set_chunk_local(&self, local_chunk_pos: &Vector3<u32>, data: VoxelRegionLeafNode) {
        todo!("impl set_chunk_local")
    }
}

impl VoxelChunkRegion {
    pub fn empty(region_pos: Vector3<i32>) -> Self {
        Self::Data(VoxelChunkRegionData::empty(region_pos))
    }

    pub fn is_loading(&self) -> bool {
        match self {
            VoxelChunkRegion::Loading => true,
            VoxelChunkRegion::Data(_) => false,
        }
    }

    pub fn data(&self) -> &VoxelChunkRegionData {
        match &self {
            VoxelChunkRegion::Loading => {
                panic!("Tried to get data VoxelChunkRegion when it is loading.")
            }
            VoxelChunkRegion::Data(data) => data,
        }
    }

    pub fn data_mut(&mut self) -> &mut VoxelChunkRegionData {
        match self {
            VoxelChunkRegion::Loading => {
                panic!("Tried to get data VoxelChunkRegion when it is loading.")
            }
            VoxelChunkRegion::Data(data) => data,
        }
    }
}

#[derive(Clone)]
pub enum VoxelChunkRegionNode {
    Internal([Option<Box<VoxelChunkRegionNode>>; 8]),
    Preleaf([VoxelRegionLeafNode; 8]),
}

impl VoxelChunkRegionNode {
    pub fn new_internal() -> Self {
        Self::Internal([const { None }; 8])
    }

    pub fn new_preleaf() -> Self {
        Self::Preleaf([const { VoxelRegionLeafNode::Empty }; 8])
    }
}

#[derive(Debug, Clone)]
pub enum VoxelRegionLeafNode {
    Empty,
    Existing {
        uuid: uuid::Uuid,
        model: Option<VoxelModelId>,
    },
}

impl VoxelRegionLeafNode {
    pub fn new_with_model(model_id: VoxelModelId) -> Self {
        Self::Existing {
            uuid: uuid::Uuid::new_v4(),
            model: Some(model_id),
        }
    }

    pub fn new_air() -> Self {
        Self::Empty
    }

    pub fn is_empty(&self) -> bool {
        match self {
            VoxelRegionLeafNode::Empty => true,
            VoxelRegionLeafNode::Existing { .. } => false,
        }
    }
}
