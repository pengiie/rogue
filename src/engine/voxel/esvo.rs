use std::{
    collections::{btree_map::IterMut, HashMap},
    fmt::Write,
    ops::Range,
};

use downcast::Downcast;
use egui::debug_text::print;
use nalgebra::Vector3;

use crate::common::morton::morton_decode;

use super::voxel::{
    Attachment, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
    VoxelModelImplConcrete, VoxelModelSchema, VoxelRange,
};

#[derive(Clone)]
pub(crate) struct VoxelModelESVO {
    pub length: u32,

    pub data: Vec<u32>,
    pub bucket_lookup: Vec<BucketLookupInfo>,

    pub updates: Option<Vec<VoxelModelESVOUpdate>>,
}

impl VoxelModelESVO {
    pub fn empty(length: u32, track_updates: bool) -> Self {
        assert!(length.is_power_of_two());
        VoxelModelESVO {
            length,

            data: Vec::new(),
            bucket_lookup: Vec::new(),
            updates: if track_updates {
                Some(Vec::new())
            } else {
                None
            },
        }
    }

    pub fn new(length: u32, track_updates: bool) -> Self {
        let mut esvo = Self::empty(length, track_updates);
        esvo.append_node(Self::encode_node(0, false, 0, 0));

        esvo
    }

    pub fn with_nodes(nodes: Vec<u32>, length: u32, track_updates: bool) -> Self {
        let mut esvo = Self::empty(length, track_updates);

        // Start at 1 since i == 0 is a page header.
        let mut last_index = 1;
        let mut added_page_headers = 0;
        for node in nodes {
            // As we write update the child_ptr to account for page headers.
            esvo.append_node(node + (added_page_headers << 17));
            if esvo.data.len() - last_index > 0 {
                added_page_headers += 1;
            }
            last_index = esvo.data.len();
        }

        esvo
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

    pub const fn encode_node(pointer: u32, far: bool, valid_mask: u32, leaf_mask: u32) -> u32 {
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

    pub const fn decode_node(node: u32) -> (u32, bool, u32, u32) {
        let child_ptr = node >> 17;
        let far = if ((node >> 16) & 1) == 1 { true } else { false };
        let value_mask = (node >> 8) & 0xFF;
        let leaf_mask = node & 0xFF;

        (child_ptr, far, value_mask, leaf_mask)
    }

    fn node_count_from_length(length: u32) -> u32 {
        let mut count = 0;
        for i in 0..length.trailing_zeros() {
            count += (length >> i).pow(3);
        }

        count
    }
}

impl VoxelModelImplConcrete for VoxelModelESVO {
    type Gpu = VoxelModelESVOGpu;
}

impl VoxelModelImpl for VoxelModelESVO {
    /// Sets a voxel range relative to the current models origin.
    fn set_voxel_range(&mut self, range: VoxelRange) {}

    fn schema(&self) -> VoxelModelSchema {
        VoxelModelSchema::ESVO
    }

    fn length(&self) -> Vector3<u32> {
        Vector3::new(self.length, self.length, self.length)
    }

    //    fn model_clone(&self) -> Box<dyn VoxelModelImpl> {
    //        Box::new(self.clone())
    //    }
    //
    //    fn take_updates(&mut self) -> Vec<VoxelModelUpdate> {
    //        self.updates.take().map_or(Vec::new(), |updates| {
    //            updates
    //                .into_iter()
    //                .map(|update| VoxelModelUpdate::ESVO(update))
    //                .collect::<Vec<VoxelModelUpdate>>()
    //        })
    //    }
}

impl std::fmt::Debug for VoxelModelESVO {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("ESVO height={}\n", self.length.trailing_zeros()));
        for (i, node) in self.data.iter().enumerate() {
            if i > 20 {
                break;
            }

            if (i % 8192) == 0 {
                f.write_str(&format!("[{}] Page Header, Block info: {}\n", i, node))?
            } else {
                let (child_ptr, far, value_mask, leaf_mask) = VoxelModelESVO::decode_node(*node);
                let value_mask_str = (0..8).fold(String::new(), |mut str, octant| {
                    str.push_str(if (value_mask & (1 << octant)) > 0 {
                        "1"
                    } else {
                        "0"
                    });

                    str
                });
                let leaf_mask_str = (0..8).fold(String::new(), |mut str, octant| {
                    str.push_str(if (leaf_mask & (1 << octant)) > 0 {
                        "1"
                    } else {
                        "0"
                    });

                    str
                });
                f.write_str(&format!(
                    "[{}] Child ptr: {}, Far: {}, Value Mask: {}, Leaf Mask: {}\n",
                    i, child_ptr, far, value_mask_str, leaf_mask_str,
                ))?
            }
        }

        f.write_str("")
    }
}

pub struct VoxelModelESVOGpu {
    data_allocation: Option<Range<u32>>,
    attachment_lookup_allocations: Option<Range<u32>>,
    raw_attachment_allocations: Option<Range<u32>>,

    initialized: bool,
}

impl VoxelModelGpuImpl for VoxelModelESVOGpu {
    fn aggregate_model_info(&self) -> Vec<u32> {
        vec![]
    }

    fn write_gpu_updates(
        &mut self,
        allocator: &mut super::voxel_allocator::VoxelAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        let model = model.downcast_ref::<VoxelModelESVO>().unwrap();

        if !self.initialized {
            // Nothing should be allocated yet.
            assert!(
                self.data_allocation.is_none()
                    && self.attachment_lookup_allocations.is_none()
                    && self.raw_attachment_allocations.is_none()
            );

            let data_allocation_size = 10000;
            self.data_allocation = Some(allocator.allocate(data_allocation_size));
            self.attachment_lookup_allocations = Some(allocator.allocate(data_allocation_size));
            self.raw_attachment_allocations = Some(allocator.allocate(data_allocation_size));
            self.initialized = true;
        }

        todo!("Finish esvo data allocation and writing");
        // todo!("Implementing this in the next commit")
    }
}

impl VoxelModelGpuImplConcrete for VoxelModelESVOGpu {
    fn new() -> Self {
        Self {
            data_allocation: None,
            attachment_lookup_allocations: None,
            raw_attachment_allocations: None,

            initialized: false,
        }
    }
}

#[derive(Clone)]
pub enum VoxelModelESVOUpdate {
    Data {
        updated_region: Range<usize>,
    },
    AttachmentLookup {
        attachment: Attachment,
        updated_region: Range<usize>,
    },
    RawAttachment {
        updated_region: Range<usize>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct BucketLookupInfo {
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

/// Iterates over all the node data given a range of node indices, not including page headers, the
/// iterator then iterates over abstracting away the page headers.
pub struct VoxelModelESVOIterMut<'a> {
    pub esvo_model: &'a mut VoxelModelESVO,
}

impl<'a> Iterator for VoxelModelESVOIterMut<'a> {
    type Item = (&'a mut u32); // (node)

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}
