use std::{collections::HashMap, ops::Range};

use egui::debug_text::print;
use nalgebra::Vector3;

use crate::common::morton::morton_decode;

use super::voxel::{Attributes, VoxelModelImpl, VoxelModelSchema, VoxelRange};

pub(crate) struct VoxelModelESVO {
    length: u32,

    data: Vec<u32>,
    bucket_lookup: Vec<BucketLookupInfo>,
}

impl VoxelModelESVO {
    pub fn new(length: u32) -> Self {
        assert!(length.is_power_of_two());
        let mut s = VoxelModelESVO {
            length,

            data: Vec::new(),
            bucket_lookup: Vec::new(),
        };

        s.append_node(Self::new_node(0, false, 0, 0));

        s
    }

    pub fn append_node(&mut self, node: u32) {
        let bucket_index = self.get_free_bucket();
        let bucket = &mut self.bucket_lookup[bucket_index as usize];

        self.data[bucket.bucket_free_start as usize] = node;
        bucket.node_size += 1;
        bucket.bucket_free_start += 1;
        if (bucket.bucket_free_start % 8192) == 0 {
            bucket.bucket_free_start += 1;
        }
    }

    pub fn get_free_bucket(&mut self) -> u32 {
        self.bucket_lookup
            .iter()
            .find_map(|info| {
                if info.node_size < info.node_capacity {
                    Some(info.index)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                self.push_empty_block();

                self.bucket_lookup.len() as u32 - 1
            })
    }

    pub fn push_empty_block(&mut self) {
        let bucket_info = BucketLookupInfo::empty(
            self.bucket_lookup.len() as u32,
            self.data.len() as u32,
            Self::node_count_from_length(16),
        );
        self.data
            .resize(self.data.len() + bucket_info.bucket_total_size as usize, 0);
        self.bucket_lookup.push(bucket_info);
        self.data[bucket_info.bucket_info_start as usize] = bucket_info.bucket_node_start;
        // TODO: Write attachment indices
        self.data[bucket_info.bucket_info_start as usize + 1] = 0;

        // Write page headers to point to block info
        let mut i = bucket_info.bucket_absolute_start;
        while i < bucket_info.bucket_info_start {
            if (i % 8192) == 0 {
                self.data[i as usize] = bucket_info.bucket_info_start;
            }

            i = (i + 1).next_multiple_of(8192);
        }
    }

    const fn new_node(pointer: u32, far: bool, valid_mask: u32, leaf_mask: u32) -> u32 {
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

    fn node_count_from_length(length: u32) -> u32 {
        let mut count = 0;
        for i in 0..length.trailing_zeros() {
            count += (length >> i).pow(3);
        }

        count
    }
}

impl VoxelModelImpl for VoxelModelESVO {
    /// Sets a voxel range relative to the current models origin.
    fn set_voxel_range(&mut self, range: VoxelRange) {}

    fn schema(&self) -> VoxelModelSchema {
        VoxelModelSchema::ESVO
    }
}

#[derive(Clone, Copy)]
struct BucketLookupInfo {
    index: u32,
    node_capacity: u32,
    node_size: u32,
    // Pointers relative to start of esvo data
    bucket_absolute_start: u32,
    bucket_node_start: u32,
    bucket_free_start: u32,
    bucket_info_start: u32,
    bucket_total_size: u32,
}

impl BucketLookupInfo {
    pub fn empty(index: u32, mut start_offset: u32, desired_node_count: u32) -> Self {
        let mut left = start_offset;
        let mut i = 0;
        while i < desired_node_count {
            let next_page_header = left.next_multiple_of(8192);
            let nodes_between = (next_page_header - left).min(desired_node_count - i);
            i += nodes_between;
            left += nodes_between + 1;
        }

        let bucket_absolute_start = start_offset;
        if (start_offset % 8192) == 0 {
            start_offset += 1;
        }

        println!("totla size; {:?}", (left + Self::bucket_info_size()) * 4);

        Self {
            index,
            node_capacity: desired_node_count,
            node_size: 0,
            bucket_absolute_start,
            bucket_node_start: start_offset,
            bucket_free_start: start_offset,
            bucket_info_start: left - 1,
            bucket_total_size: left + Self::bucket_info_size(),
        }
    }

    const fn bucket_info_size() -> u32 {
        let mut x = 0;

        // Bucket start index
        x += 1;
        // Albedo attachment index (Absolute in data buffer)
        x += 1;

        x
    }
}
