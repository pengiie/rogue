use std::{
    collections::{btree_map::IterMut, HashMap},
    fmt::Write,
    ops::Range,
    u32,
};

use downcast::Downcast;
use egui::debug_text::print;
use log::debug;
use nalgebra::Vector3;
use wgpu::core::device;

use crate::{common::morton::morton_decode, engine::graphics::device::DeviceResource};

use super::{
    voxel::{
        Attachment, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
        VoxelModelImplConcrete, VoxelModelSchema, VoxelRange,
    },
    voxel_allocator::{VoxelAllocator, VoxelDataAllocation},
};

#[derive(Clone)]
pub(crate) struct VoxelModelESVO {
    pub length: u32,

    pub node_data: Vec<u32>,
    pub attachment_lookup_data: HashMap<Attachment, Vec<u32>>,
    pub attachment_raw_data: HashMap<Attachment, Vec<u32>>,
    pub bucket_lookup: Vec<BucketLookupInfo>,

    pub updates: Option<Vec<VoxelModelESVOUpdate>>,
}

impl VoxelModelESVO {
    pub fn empty(length: u32, track_updates: bool) -> Self {
        assert!(length.is_power_of_two());
        VoxelModelESVO {
            length,

            node_data: Vec::new(),
            attachment_lookup_data: HashMap::new(),
            attachment_raw_data: HashMap::new(),
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
        esvo.append_node(Self::encode_node(0, false, 0, 0), None);

        esvo
    }

    pub fn with_nodes(
        nodes: Vec<(u32, Option<HashMap<Attachment, u32>>)>,
        attachment_raw: HashMap<Attachment, Vec<u32>>,
        length: u32,
        track_updates: bool,
    ) -> Self {
        for (i, (node_data, attachment_lookup_data)) in nodes.iter().enumerate() {
            let (child_ptr, far, value_mask, leaf_mask) = VoxelModelESVO::decode_node(*node_data);
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
            debug!(
                "[{}] Child ptr: {}, Far: {}, Value Mask: {}, Leaf Mask: {}, Has Attachment?: {}",
                i,
                child_ptr,
                far,
                value_mask_str,
                leaf_mask_str,
                attachment_lookup_data.is_some()
            );
            if let Some(attachment_lookup_data) = attachment_lookup_data {
                for (attachment, attachment_lookup_data) in attachment_lookup_data {
                    let (raw_data_ptr, attachment_mask) =
                        VoxelModelESVO::decode_attachment_lookup(*attachment_lookup_data);
                    let attachment_mask_str = (0..8).fold(String::new(), |mut str, octant| {
                        str.push_str(if (attachment_mask & (1 << octant)) > 0 {
                            "1"
                        } else {
                            "0"
                        });

                        str
                    });
                    debug!(
                        "[{}] Attachment Lookup [{}] Data ptr: {}, Attachment Mask: {}\n",
                        i,
                        attachment.name(),
                        raw_data_ptr,
                        attachment_mask_str,
                    );
                }
            }
        }

        let mut esvo = Self::empty(length, track_updates);

        // The next index to be written.
        let mut next_index = 1;
        // TODO: This struct padding doesnt actually work because we want the opposite, there may
        // be page headers between this node and where the child_ptr is pointing to which would
        // offset the child pointer but we aren't accounting for that yet so what we need to do is
        // either make a function to calculate the amount of page headers because which is more
        // complicated because we can have bucket infos between or and keep track of the nodes and
        // how many page headers to a node so we know how much to offset references after this, we
        // can compare the perf of both at some point and choose.
        //
        // For now we can just make the bucket size big so we dont have to worry about it.
        let mut added_struct_padding = 0;
        for (node_data, attachment_lookup_data) in nodes {
            assert_eq!(added_struct_padding, 0);
            // We add the offset the node is now at due to padding .
            let updated_child_ptr_node = node_data;
            // We don't have to modify the lookup data raw attachment pointer since the raw
            // attachment will have the same position since we are making a brand new esvo.
            esvo.append_node(updated_child_ptr_node, attachment_lookup_data);

            let next_free_bucket = esvo.get_free_bucket() as usize;
            let next_free = esvo.bucket_lookup[next_free_bucket].bucket_free_start;

            if next_free - next_index > 1 {
                added_struct_padding += next_free - next_index - 1;
            }

            next_index = next_free;
        }
        esvo.attachment_raw_data = attachment_raw;

        esvo
    }

    /// Appends the node to self.node_data and the corresponding mapped one-to-one self.attachment_lookup_data
    pub fn append_node(&mut self, node: u32, leaf_attachments: Option<HashMap<Attachment, u32>>) {
        let bucket_index = self.get_free_bucket();
        let bucket = &mut self.bucket_lookup[bucket_index as usize];

        self.node_data[bucket.bucket_free_start as usize] = node;
        let attachment_lookup_index = bucket.bucket_free_start as usize;
        bucket.node_size += 1;
        bucket.bucket_free_start += 1;
        if (bucket.bucket_free_start % 8192) == 0 {
            bucket.bucket_free_start += 1;
        }
        if let Some(leaf_attachments) = leaf_attachments {
            for (attachment, attachment_lookup_data) in leaf_attachments {
                self.get_attachment_lookup_data(attachment)[attachment_lookup_index] =
                    attachment_lookup_data;
            }
        }
    }

    pub fn get_attachment_lookup_data(&mut self, attachment: Attachment) -> &mut Vec<u32> {
        self.attachment_lookup_data
            .entry(attachment)
            .or_insert_with(|| vec![0; self.node_data.len()])
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
                self.push_empty_bucket();

                self.bucket_lookup.len() as u32 - 1
            })
    }

    pub fn push_empty_bucket(&mut self) {
        let bucket_info = BucketLookupInfo::empty(
            self.bucket_lookup.len() as u32,
            self.node_data.len() as u32,
            32,
        );
        self.node_data.resize(
            self.node_data.len() + bucket_info.bucket_total_size as usize,
            0,
        );
        self.bucket_lookup.push(bucket_info);
        self.node_data[bucket_info.bucket_info_start as usize] = bucket_info.bucket_absolute_start;
        // TODO: Write attachment indices
        self.node_data[bucket_info.bucket_info_start as usize + 1] = 0;

        // Write page headers to point to block info
        let mut i = bucket_info.bucket_absolute_start;
        while i < bucket_info.bucket_info_start {
            if (i % 8192) == 0 {
                self.node_data[i as usize] = bucket_info.bucket_info_start;
            }

            i = (i + 1).next_multiple_of(8192);
        }
    }

    pub const fn encode_node(pointer: u32, far: bool, valid_mask: u32, leaf_mask: u32) -> u32 {
        // Make sure child pointer is only 15 bits.
        assert!(pointer < (1 << 15), "Pointer is too big.");
        assert!(valid_mask < (1 << 8), "valid mask is too big.");
        assert!(leaf_mask < (1 << 8), "leaf mask is too big.");
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

    pub const fn encode_attachment_lookup(
        raw_attachment_index: u32,
        leaf_attachment_mask: u32,
    ) -> u32 {
        // Make sure raw_attachment_data is only 24 bits.
        assert!(raw_attachment_index < (1 << 24));
        assert!(leaf_attachment_mask < (1 << 8));

        (raw_attachment_index << 8) | leaf_attachment_mask
    }

    pub const fn decode_attachment_lookup(data: u32) -> (u32, u32) {
        (data >> 8, data & 0xFF)
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
        for (i, node) in self.node_data.iter().enumerate() {
            if (i % 8192) == 0 {
                f.write_str(&format!("[{}] Page Header, Block info: {}\n", i, node))?
            } else {
                let (child_ptr, far, value_mask, leaf_mask) = VoxelModelESVO::decode_node(*node);
                let value_mask_str = (0..8).rev().fold(String::new(), |mut str, octant| {
                    str.push_str(if (value_mask & (1 << octant)) > 0 {
                        "1"
                    } else {
                        "0"
                    });

                    str
                });
                let leaf_mask_str = (0..8).rev().fold(String::new(), |mut str, octant| {
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

        for (attachment, attachment_lookup_data) in &self.attachment_lookup_data {
            for (i, lookup) in attachment_lookup_data.iter().enumerate() {
                let (raw_data_ptr, attachment_mask) =
                    VoxelModelESVO::decode_attachment_lookup(*lookup);
                let attachment_mask_str = (0..8).rev().fold(String::new(), |mut str, octant| {
                    str.push_str(if (attachment_mask & (1 << octant)) > 0 {
                        "1"
                    } else {
                        "0"
                    });

                    str
                });
                f.write_str(&format!(
                    "Attachment Lookup [{}][{}] Data ptr: {}, Attachment Mask: {}\n",
                    attachment.name(),
                    i,
                    raw_data_ptr,
                    attachment_mask_str,
                ));
            }
        }

        for (attachment, data) in &self.attachment_raw_data {
            f.write_str(&format!("Attachment Raw Data [{}]:\n\t", attachment.name()));
            match attachment.renderable_index() {
                Attachment::ALBEDO_RENDER_INDEX => {
                    for (i, albedo) in data.iter().enumerate() {
                        let (r, g, b, a) = Attachment::decode_albedo(*albedo);
                        f.write_str(&format!("[{}] {} {} {}, ", i, r, g, b));
                    }
                }
                default => {}
            }
        }

        f.write_str("")
    }
}

pub struct VoxelModelESVOGpu {
    data_allocation: Option<VoxelDataAllocation>,
    attachment_lookup_allocations: HashMap<Attachment, VoxelDataAllocation>,
    attachment_raw_allocations: HashMap<Attachment, VoxelDataAllocation>,

    initialized_data: bool,
}

impl VoxelModelGpuImpl for VoxelModelESVOGpu {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        let Some(data_allocation) = &self.data_allocation else {
            return None;
        };

        let albedo_lookup_attachment_ptr = if let Some(lookup_allocation) =
            self.attachment_lookup_allocations.get(&Attachment::ALBEDO)
        {
            lookup_allocation.start_index() >> 2
        } else {
            u32::MAX
        };

        let albedo_raw_attachment_ptr = if let Some(raw_allocation) =
            self.attachment_raw_allocations.get(&Attachment::ALBEDO)
        {
            raw_allocation.start_index() >> 2
        } else {
            u32::MAX
        };

        let info = vec![
            // World data ptr (divide by 4 since 4 bytes in a u32)
            data_allocation.start_index() >> 2,
            // Albedo attachment lookup ptr
            albedo_lookup_attachment_ptr,
            // Albedo attachment raw ptr
            albedo_raw_attachment_ptr,
        ];

        Some(info)
    }

    fn update_gpu_objects(&mut self, allocator: &mut VoxelAllocator, model: &dyn VoxelModelImpl) {
        let model = model.downcast_ref::<VoxelModelESVO>().unwrap();

        if self.data_allocation.is_none() {
            let data_allocation_size = model.node_data.len() as u64 * 4;
            self.data_allocation = Some(
                allocator
                    .allocate(data_allocation_size)
                    .expect("Failed to allocate ESVO node data."),
            );
        }

        for (attachment, data) in &model.attachment_lookup_data {
            if !self.attachment_lookup_allocations.contains_key(attachment) {
                let lookup_data_allocation_size = data.len() as u64 * 4;
                self.attachment_lookup_allocations.insert(
                    attachment.clone(),
                    allocator
                        .allocate(lookup_data_allocation_size)
                        .expect("Failed to allocate ESVO attachment lookup data."),
                );
            }
        }

        for (attachment, data) in &model.attachment_raw_data {
            if !self.attachment_raw_allocations.contains_key(attachment) {
                let raw_data_allocation_size = data.len() as u64 * 4;
                self.attachment_raw_allocations.insert(
                    attachment.clone(),
                    allocator
                        .allocate(raw_data_allocation_size)
                        .expect("Failed to allocate ESVO attachment raw data."),
                );
            }
        }
    }

    fn write_gpu_updates(
        &mut self,
        device: &DeviceResource,
        allocator: &mut VoxelAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        let model = model.downcast_ref::<VoxelModelESVO>().unwrap();

        // If data allocation is some and we haven't initialized yet, expected the attachment data
        // to also be ready.
        if !self.initialized_data && self.data_allocation.is_some() {
            debug!("Writing initial data");

            debug!("Writing node data {:?}", model.node_data.as_slice());
            allocator.write_world_data(
                device,
                self.data_allocation.as_ref().unwrap(),
                bytemuck::cast_slice::<u32, u8>(model.node_data.as_slice()),
            );

            for (attachment, lookup_data) in &model.attachment_lookup_data {
                debug!(
                    "Writing attachment lookup data [{}] {:?}",
                    attachment.name(),
                    lookup_data.as_slice()
                );
                let allocation = self
                    .attachment_lookup_allocations
                    .get(attachment)
                    .expect("Lookup allocation should exist by now.");

                allocator.write_world_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(lookup_data.as_slice()),
                );
            }

            for (attachment, raw_data) in &model.attachment_raw_data {
                debug!(
                    "Writing attachment raw data [{}] {:?}",
                    attachment.name(),
                    raw_data.as_slice()
                );
                let allocation = self
                    .attachment_raw_allocations
                    .get(attachment)
                    .expect("Raw allocation should exist by now.");

                allocator.write_world_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(raw_data.as_slice()),
                );
            }

            self.initialized_data = true;
        }
    }
}

impl VoxelModelGpuImplConcrete for VoxelModelESVOGpu {
    fn new() -> Self {
        Self {
            data_allocation: None,
            attachment_lookup_allocations: HashMap::new(),
            attachment_raw_allocations: HashMap::new(),

            initialized_data: false,
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
    pub fn empty(index: u32, mut start_offset: u32, desired_bucket_size: u32) -> Self {
        assert!(desired_bucket_size.is_power_of_two());

        let node_capacity = desired_bucket_size - Self::bucket_info_size();

        let mut page_header_count = 0;
        let mut left = start_offset;
        let bucket_info_start = start_offset + node_capacity;
        while left < bucket_info_start {
            page_header_count += 1;

            left = (left + 1).next_multiple_of(8192);
        }

        let bucket_absolute_start = start_offset;
        if (start_offset % 8192) == 0 {
            start_offset += 1;
        }

        Self {
            index,
            node_capacity: node_capacity - page_header_count,
            node_size: 0,
            bucket_absolute_start,
            bucket_node_start: start_offset,
            bucket_free_start: start_offset,
            bucket_info_start,
            bucket_total_size: desired_bucket_size,
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
