use std::collections::HashMap;

use fixedbitset::FixedBitSet;
use nalgebra::Vector3;
use pollster::block_on;
use wgpu::naga::valid;

use super::voxel::{Attributes, VoxelModelImpl, VoxelRange};

pub(crate) struct VoxelModelESVO {
    nodes: Nodes,
    attachments: Attachments,
    length: Vector3<u32>,
    root: u32,
}

impl VoxelModelESVO {
    pub fn new() -> Self {
        VoxelModelESVO {
            nodes: Nodes::new(),
            attachments: Attachments::new(),
            length: Vector3::new(2, 2, 2),
            root: 0,
        }
    }
}

impl VoxelModelImpl for VoxelModelESVO {
    /// Sets a voxel range relative to the current models origin.
    fn set_voxel_range(&mut self, range: VoxelRange) {
        let esvo_min_length = range.length().map(|x| x.next_power_of_two());
        let needs_resize = self
            .length
            .zip_fold(&esvo_min_length, false, |acc, a, b| acc || (a < b));
        if needs_resize {}
    }
    fn get_node_data(&self) -> &[u8] {
        bytemuck::cast_slice(self.nodes.nodes.as_slice())
    }

    fn get_attachment_lookup_data(&self) -> &[u8] {
        bytemuck::cast_slice(self.attachments.lookup.as_slice())
    }

    fn get_attachments_data(&self) -> HashMap<Attributes, &[u8]> {
        self.attachments
            .attributes
            .iter()
            .map(|(attr, data)| (*attr, bytemuck::cast_slice(data.as_slice())))
            .collect::<_>()
    }
    //fn get_range_mut(&self, point0: IVec3, point1: IVec3) -> VoxelRegion {}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Block {
    pub total_size: u32,
    pub node_size: u32,
    pub available_node_size: u32,
}

struct Nodes {
    nodes: Vec<u32>,
    blocks: Vec<Block>,
}

impl Nodes {
    /// Initializes with an empty root node
    fn new() -> Self {
        Self {
            nodes: vec![
            //        // Page header - points to block info
            //        3,
            //        // Nodes
            //        Self::make_node(1, false, 0b00000011, 0b00000010),
            //        Self::make_node(0, false, 0b00000100, 0b00000100),
            //        // Block info
            //        1, // Block Start (Makes node 0 at index 0)
            //        0, // Lookup Pointer
                ],
            blocks: vec![],
        }
    }

    fn new_node(pointer: u32, far: bool, valid_mask: u32, leaf_mask: u32) -> u32 {
        assert!(pointer < 0b1000000000000000, "Pointer is too big.");
        assert!(valid_mask < 0b100000000, "valid mask is too big.");
        assert!(leaf_mask < 0b100000000, "leaf mask is too big.");
        let mut x = 0;
        x |= pointer << 17;
        if far {
            x |= 0x0001_0000;
        }
        x |= valid_mask << 8;
        x |= leaf_mask;

        x
    }

    /// Calculates (total size, block info start index)
    const fn calculate_block_sizes(mut side_length: u32) -> (u32, u32) {
        let mut total = 1;
        while side_length > 1 {
            total += side_length.pow(3);
            side_length /= 2;
        }

        let page_header_count = (total / 8192) + 1;
        let block_info_start_index = total + page_header_count;
        let total = total + page_header_count + 2;

        (total, block_info_start_index)
    }
    const BLOCK_SIZE: (u32, u32) = Self::calculate_block_sizes(32);
    fn create_block(&mut self) -> Block {}

    fn find_or_create_block(&mut self, needed_size: u32) -> Block {
        for block in &self.blocks {
            if block.available_node_size >= needed_size {
                return block.clone();
            }
        }

        self.create_block()
    }

    fn append_node(&mut self, node: u32) {
        let block = self.find_or_create_block(1);
    }
}

struct Attachments {
    lookup: Vec<u32>,
    attributes: HashMap<Attributes, Vec<u32>>,
}

impl Attachments {
    fn new() -> Self {
        let mut attributes = HashMap::new();
        attributes.insert(Attributes::ALBEDO, vec![0xFF00FF00, 0xFF000000]);
        Self {
            lookup: vec![
                Self::make_lookup_slot(0, 0b00000010),
                Self::make_lookup_slot(1, 0b00000100),
            ],
            attributes,
        }
    }

    fn make_lookup_slot(raw_attachment_pointer: u32, attribute_mask: u32) -> u32 {
        assert!(attribute_mask & 0xFF == attribute_mask);

        (raw_attachment_pointer << 8) | attribute_mask
    }
}
