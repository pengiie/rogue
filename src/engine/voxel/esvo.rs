use std::{
    borrow::BorrowMut,
    collections::{btree_map::IterMut, HashMap},
    fmt::{Display, Pointer, Write},
    ops::{DerefMut, Range, Rem},
    u32,
};

use bytemuck::{Pod, Zeroable};
use downcast::Downcast;
use egui::debug_text::print;
use log::{debug, error, warn};
use nalgebra::{ComplexField, Vector3};

use crate::{
    common::morton::{morton_decode, morton_encode},
    consts,
    engine::graphics::{
        device::{DeviceResource, GfxDevice},
        gpu_allocator::{Allocation, GpuBufferAllocator},
    },
};

use super::{
    attachment::{Attachment, AttachmentId, AttachmentInfoMap, AttachmentMap},
    voxel::{
        VoxelData, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
        VoxelModelImplConcrete, VoxelModelSchema, VoxelModelTrace,
    },
};

#[derive(Clone)]
pub(crate) struct VoxelModelESVO {
    pub length: u32,
    pub root_node_index: u32,

    pub node_data: Vec<VoxelModelESVONode>,
    // Maps index identically to node_data, including page headers and block infos.
    pub node_metadata_data: Vec<ESVONodeMetadata>,

    pub attachment_lookup_data: HashMap<AttachmentId, Vec<VoxelModelESVOAttachmentLookupNode>>,
    pub attachment_raw_data: HashMap<AttachmentId, Vec<u32>>,
    pub attachment_map: AttachmentInfoMap,
    pub bucket_metadata: Vec<BucketMetadata>,

    pub updates: Option<Vec<VoxelModelESVOUpdate>>,
}

impl VoxelModelESVO {
    const PAGE_SIZE: u32 = 1 << 13; // 8192
    const DEFAULT_BUCKET_SIZE: u32 = 1 << 14;
    const DEFAULT_CHILD_CAPACITY: u32 = 8;
    const MAX_RAW_INDEX: u32 = (1 << 16) - 1;

    pub fn empty(length: u32, track_updates: bool) -> Self {
        assert!(length.is_power_of_two());
        let mut s = VoxelModelESVO {
            length,
            root_node_index: 0,

            node_data: Vec::new(),
            node_metadata_data: Vec::new(),
            attachment_lookup_data: HashMap::new(),
            attachment_raw_data: HashMap::new(),
            attachment_map: AttachmentMap::new(),
            bucket_metadata: Vec::new(),
            updates: track_updates.then_some(Vec::new()),
        };

        s.root_node_index = s.allocate_node_data(0, 1, Self::MAX_RAW_INDEX);
        s.node_metadata_data[s.root_node_index as usize] =
            ESVONodeMetadata::Some(ESVONodeMetadataData {
                parent_index: 0,
                children_capacity: 0,
            });

        s
    }

    /// Allocates children for the node at parent_index, handling far pointers and assigning the
    /// parent child pointer and node metadata promotion automatically. Allocation size is exactly of child_count.
    ///
    /// Returns the index into the beginning of the child allocation, skipping far pointers.
    pub fn allocate_node_children(&mut self, parent_index: u32, child_count: u32) -> u32 {
        let new_children_ptr =
            self.allocate_node_data(parent_index, child_count, Self::MAX_RAW_INDEX);
        //debug!(
        //    "allocated node data at: {} {}",
        //    parent_index, new_children_ptr
        //);

        let relative_child_ptr = new_children_ptr - parent_index;
        self.node_data[parent_index as usize].set_relative_ptr(relative_child_ptr);
        //debug!(
        //    "Settings parent {} child ptr to {}",
        //    parent_index, relative_child_ptr
        //);

        let metadata = &mut self.node_metadata_data[parent_index as usize];
        match metadata {
            ESVONodeMetadata::Some(m) => {
                m.children_capacity = child_count;
            }
            ESVONodeMetadata::EmptyChild(pi) => {
                let pi = *pi;
                *metadata = ESVONodeMetadata::Some(ESVONodeMetadataData {
                    parent_index: pi,
                    children_capacity: child_count,
                });
            }
            ESVONodeMetadata::Free => panic!("Can't allocate if the parent_index is free."),
        };

        for new_child_metadata in &mut self.node_metadata_data
            [new_children_ptr as usize..(new_children_ptr + child_count) as usize]
        {
            *new_child_metadata = ESVONodeMetadata::EmptyChild(parent_index);
        }

        new_children_ptr
    }

    pub fn get_node_mut(&mut self, node_index: u32) -> &mut VoxelModelESVONode {
        &mut self.node_data[node_index as usize]
    }

    // Allocates a child node which itself has a child capacity initially of 0, with the parent
    // node at `parent_index`.
    pub fn allocate_node_child(&mut self, parent_index: u32, default_child_capacity: u32) -> u32 {
        let parent_node = &self.node_data[parent_index as usize];
        if parent_node.is_far() {
            todo!("Deal with far pointer case.");
        }

        let current_child_capacity = self.node_metadata_data[parent_index as usize]
            .unwrap()
            .children_capacity;

        let parent_relative_child_ptr = parent_node.relative_ptr();
        let parent_children_index = parent_index + parent_relative_child_ptr;
        let mut child_slot_index: Option<u32> = None;
        for i in parent_children_index..(parent_children_index + current_child_capacity) {
            if self.node_metadata_data[i as usize].is_empty_child() {
                // We found an open child slot for the parents child capactity.
                child_slot_index = Some(i);
                break;
            }
        }

        let child_slot_index = child_slot_index.unwrap_or_else(|| {
            // Re-allocate child capacity for the parent node so we can allocate this node as
            // the parent's child.
            let new_child_capacity = if current_child_capacity == 0 {
                default_child_capacity
            } else {
                (current_child_capacity * 2).min(8)
            };

            let new_children_ptr = self.allocate_node_data(parent_index, new_child_capacity, Self::MAX_RAW_INDEX);
            if current_child_capacity != 0 {
                assert!(current_child_capacity < 8);

                let copy_len = current_child_capacity as usize;
                unsafe {
                    let src_ptr = self
                        .node_data
                        .as_ptr()
                        .offset(parent_children_index as isize);
                    let dst_ptr = self.node_data.as_ptr().offset(new_children_ptr as isize)
                        as *mut VoxelModelESVONode;
                    dst_ptr.copy_from_nonoverlapping(src_ptr, copy_len);
               todo!("recursively reallocate any children of the chilren we just moved since the relative pointers will now no longer be correct."); 
                }

                for old_child in &mut self.node_metadata_data[parent_children_index as usize
                    ..(parent_children_index + current_child_capacity) as usize]
                {
                    //*old_child = ESVONodeMetadata::Free;
                }
            }

            let relative_child_ptr = new_children_ptr - parent_index;
            self.node_data[parent_index as usize].set_relative_ptr(relative_child_ptr);
            self.node_metadata_data[parent_index as usize]
                .unwrap_mut()
                .children_capacity = new_child_capacity;

            for new_child_metadata in &mut self.node_metadata_data
                [new_children_ptr as usize..(new_children_ptr + new_child_capacity) as usize]
            {
                *new_child_metadata = ESVONodeMetadata::EmptyChild(parent_index);
            }

            new_children_ptr
        });

        self.node_metadata_data[child_slot_index as usize] =
            ESVONodeMetadata::Some(ESVONodeMetadataData {
                parent_index,
                children_capacity: 0,
            });

        child_slot_index
    }

    // Returns the u32 index where the allocation starts.
    pub fn allocate_node_data(
        &mut self,
        after_index: u32,
        size: u32,
        maximum_distance: u32,
    ) -> u32 {
        assert!(
            size > 0 && size <= 8,
            "Size can only be between 1-8 and we got {}",
            size
        );

        let free_bucket_index = self
            .bucket_metadata
            .iter()
            .find_map(|info| {
                if info.node_size + size <= info.node_capacity
                    && info.bucket_free_start > after_index
                {
                    Some(info.metadata_index)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                let new_bucket_size = Self::DEFAULT_BUCKET_SIZE;
                let maximum_possible_page_header_count = 1 + new_bucket_size / Self::PAGE_SIZE;
                assert!(
                    new_bucket_size
                        >= size + self.bucket_info_size() + maximum_possible_page_header_count,
                    "Newly created bucket isn't big enough to allocate {} contiguous nodes.",
                    size
                );
                assert!(
                    new_bucket_size >= Self::PAGE_SIZE,
                    "Bucket must be large than page sizes so a page header can exist for a bucket."
                );
                self.push_empty_bucket(new_bucket_size);

                self.bucket_metadata.len() as u32 - 1
            });
        let free_bucket = self
            .bucket_metadata
            .get_mut(free_bucket_index as usize)
            .unwrap();

        // We can always assume that `bucket_free_start` is never a multiple of `Self::PAGE_SIZE`
        // since we mantain that on `BucketMetadata` creation and maintain that here.
        let mut start_index = free_bucket.bucket_free_start;
        let end_index = start_index + size - 1;
        let end_page_pre_padding = end_index % Self::PAGE_SIZE;
        // If the remainder of the start index is larger then the end, then that means the
        if (start_index % Self::PAGE_SIZE > end_page_pre_padding) {
            assert!(Self::PAGE_SIZE > size, "The page size must be larger than the max allocation 
                size of 8 since we must be able to guarantee contiguous data without a page header in the way.");

            // This will position the start index right after the page header was encounted.
            // The one in the following 2 lines is the account for the page header.
            let new_start_index = end_index - end_page_pre_padding + 1;
            free_bucket.node_capacity -= new_start_index - start_index - 1;
            start_index = new_start_index;
        }

        free_bucket.bucket_free_start = start_index + size;
        free_bucket.node_capacity -= size;

        start_index
    }

    pub fn get_attachment_lookup_node_mut(
        &mut self,
        attachment_id: AttachmentId,
        index: u32,
    ) -> &mut VoxelModelESVOAttachmentLookupNode {
        assert!(self.attachment_map.contains(attachment_id));

        let lookup_data = self.get_attachment_lookup_data_mut(attachment_id);

        &mut lookup_data[index as usize]
    }

    // Returns the u32 index where the allocation starts.
    pub fn allocate_raw_attachment_data(&mut self, attachment_id: AttachmentId, size: u32) -> u32 {
        assert!(size > 0 && size <= 8);
        assert!(self.attachment_map.contains(attachment_id));

        let raw_attachment_data = self
            .attachment_raw_data
            .entry(attachment_id)
            .or_insert(Vec::new());

        let mut start_index = raw_attachment_data.len() as u32;
        raw_attachment_data.resize(raw_attachment_data.len() + size as usize, 0);

        start_index
    }

    pub fn resize_raw_attachment_data(&mut self, attachment_id: AttachmentId, new_len: u32) {
        assert!(self.attachment_map.contains(attachment_id));
        let raw_attachment_data = self
            .attachment_raw_data
            .get_mut(&attachment_id)
            .expect("Can't shrink a non existant buffer.");
        raw_attachment_data.resize(new_len as usize, 0);
    }

    /// Will get or create the child node for this parent index, if the child doesn't exist and
    /// this node doesn't have any children allocated, it will use allocate `default_child_capacity` children for the parent index.
    /// This will return None if the child of this node at the specified octant exists as a leaf
    /// already.
    pub fn get_or_create_child_node_index(
        &mut self,
        parent_index: u32,
        octant: u32,
        default_child_capacity: u32,
    ) -> Option<u32> {
        let (relative_ptr, is_far, valid_mask, leaf_mask) =
            self.node_data[parent_index as usize].decode();

        let has_child = (valid_mask & (1 << octant)) > 0;
        let is_leaf = (leaf_mask & (1 << octant)) > 0;

        if is_leaf {
            return None;
        }

        let mut allocated_child_ptr = if !has_child {
            let mut child_index = self.allocate_node_child(parent_index, default_child_capacity);

            // Clears the bits at the octants bit and to the right so that only octants that should
            // come after this octant should be present if they exist.
            let mut must_swap = valid_mask & !((1 << (octant + 1)) - 1) > 0;
            if must_swap {
                todo!("swap ordering since octants of a higher norm must be after this one due to implicit child ordering");
            }

            self.node_data[parent_index as usize].set_valid_mask(valid_mask | (1 << octant));

            child_index
        } else {
            let child_offset = (valid_mask & ((1 << octant) - 1)).count_ones();
            parent_index + relative_ptr + child_offset
        };

        Some(allocated_child_ptr)
    }

    pub fn in_bounds(&self, position: Vector3<u32>) -> bool {
        !(position.x >= self.length || position.y >= self.length || position.z >= self.length)
    }

    pub fn height(&self) -> u32 {
        self.length.trailing_zeros()
    }

    pub fn get_voxel_mut(&mut self, position: Vector3<u32>) -> VoxelModelESVOVoxelAccessMut<'_> {
        assert!(self.in_bounds(position));

        VoxelModelESVOVoxelAccessMut {
            esvo_model: self,
            position,
        }
    }

    pub fn root_node_index(&self) -> u32 {
        self.root_node_index
    }

    pub fn get_attachment_lookup_data_mut(
        &mut self,
        attachment_id: AttachmentId,
    ) -> &mut Vec<VoxelModelESVOAttachmentLookupNode> {
        assert!(self.attachment_map.contains(attachment_id));
        self.attachment_lookup_data
            .entry(attachment_id)
            .or_insert_with(|| vec![VoxelModelESVOAttachmentLookupNode(0); self.node_data.len()])
    }

    pub fn get_attachment_raw_data_mut(&mut self, attachment_id: AttachmentId) -> &mut Vec<u32> {
        assert!(self.attachment_map.contains(attachment_id));
        self.attachment_raw_data
            .entry(attachment_id)
            .or_insert(Vec::new())
    }

    // Bucket size must be a power of 2 so that it can be optimally allocated in the world data
    // buffer.
    pub fn push_empty_bucket(&mut self, bucket_size: u32) {
        assert!(bucket_size.is_power_of_two());

        let start_offset = self.node_data.len() as u32;
        assert!(
            (start_offset.max(1)).is_power_of_two(),
            "Somehow the start offset isn't a power of 2, start offset: {}",
            start_offset
        );

        let bucket_metadata = BucketMetadata::empty(
            self.bucket_metadata.len() as u32,
            start_offset,
            bucket_size,
            self.bucket_info_size(),
        );

        // Resize node data and node metadata data.
        // TODO: Resize attachment lookup buffers.
        self.node_data.resize(
            self.node_data.len() + bucket_metadata.bucket_total_size as usize,
            VoxelModelESVONode::encode_node(0, false, 0, 0),
        );
        self.node_metadata_data.resize(
            self.node_metadata_data.len() + bucket_metadata.bucket_total_size as usize,
            ESVONodeMetadata::Free,
        );
        assert_eq!(self.node_data.len(), self.node_metadata_data.len());
        for (_, attachment_lookup_data) in &mut self.attachment_lookup_data {
            attachment_lookup_data.resize(
                attachment_lookup_data.len() + bucket_metadata.bucket_total_size as usize,
                VoxelModelESVOAttachmentLookupNode(0),
            );

            assert_eq!(self.node_data.len(), attachment_lookup_data.len());
        }

        self.bucket_metadata.push(bucket_metadata);
        // Writing index 0 of bucket info.
        self.node_data[bucket_metadata.bucket_info_start as usize] =
            VoxelModelESVONode::encode_data(bucket_metadata.bucket_absolute_start);
        assert_eq!(
            (bucket_metadata.bucket_absolute_start + bucket_metadata.bucket_total_size)
                - bucket_metadata.bucket_info_start,
            self.bucket_info_size(),
            "Expected size between {} and {} to be {}, but it was not.",
            bucket_metadata.bucket_info_start,
            bucket_metadata.bucket_total_size,
            self.bucket_info_size(),
        );

        // Write page headers to point to block info
        for i in (bucket_metadata.bucket_absolute_start..bucket_metadata.bucket_info_start)
            .step_by(Self::PAGE_SIZE as usize)
        {
            self.node_data[i as usize] =
                VoxelModelESVONode::encode_data(bucket_metadata.bucket_info_start);
        }
    }

    fn bucket_info_size(&self) -> u32 {
        1
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

pub struct VoxelModelESVOVoxelAccessMut<'a> {
    pub esvo_model: &'a mut VoxelModelESVO,
    pub position: Vector3<u32>,
}

impl<'a> VoxelModelESVOVoxelAccessMut<'a> {
    /// Returns (parent_node_index, leaf_octant).
    /// Done lazily so we don't create nodes if we don't have to.
    pub fn get_or_create_leaf_node(&mut self) -> (u32, u32) {
        let morton = morton_encode(self.position);
        let traversal = (0..self.esvo_model.height())
            .map(|i| ((morton >> (i * 3)) & 7) as u32)
            .rev();

        let mut parent_node_index = self.esvo_model.root_node_index;
        let mut octant = 0;
        'traversal_loop: for (i, traversal) in traversal.enumerate() {
            if i as u32 == self.esvo_model.height() - 1 {
                // parent_node_index is equal to the leaf nodes parent.
                let parent_node_data = &mut self.esvo_model.node_data[parent_node_index as usize];
                parent_node_data.set_leaf_mask(parent_node_data.leaf_mask() | (1 << traversal));
                parent_node_data.set_valid_mask(parent_node_data.valid_mask() | (1 << traversal));

                octant = traversal;
            } else {
                match self.esvo_model.get_or_create_child_node_index(
                    parent_node_index,
                    traversal,
                    VoxelModelESVO::DEFAULT_CHILD_CAPACITY,
                ) {
                    Some(child_ptr) => {
                        parent_node_index = child_ptr;
                    }
                    None => {
                        panic!("Node in here already exists as a leaf node with data on the non-leaf layer, figure out behavior here. index {}", i);
                    }
                }
            }
        }

        (parent_node_index, octant)
    }

    pub fn set_data(&mut self, voxel_data: &VoxelData) {
        let (parent_node_index, leaf_octant) = self.get_or_create_leaf_node();

        for (attachment, data) in voxel_data
            .iter()
            .map(|(attachment_id, data)| {
                (
                    self.esvo_model
                        .attachment_map
                        .get_unchecked(*attachment_id)
                        .clone(),
                    data,
                )
            })
            .collect::<Vec<_>>()
        {
            self.set_attachment_data(parent_node_index, leaf_octant, &attachment, data);
        }
    }

    pub fn set_attachment_data(
        &mut self,
        parent_node_index: u32,
        leaf_octant: u32,
        attachment: &Attachment,
        data: &[u32],
    ) {
        let old_attachment_mask = self
            .parent_node_lookup_mut(attachment.id(), parent_node_index)
            .attachment_mask();

        let new_attachment_mask = old_attachment_mask | (1 << leaf_octant);
        self.parent_node_lookup_mut(attachment.id(), parent_node_index)
            .set_attachment_mask(new_attachment_mask);

        let raw_attachment_index = if old_attachment_mask != new_attachment_mask {
            // Entry for this octant for the corresponding attachment type doesn't exist yet.
            //todo!("Add attachment lookup metadata so we can properly grow raw attachment child data and track how many children for raw attachment data are allocated for a node");
            let has_raw_data = old_attachment_mask > 0;
            if !has_raw_data {
                // No entries for any children in the parent node exist so we must allocate
                // some data in the attachment's raw data buffer. We only do this now since we
                // want to be lazily allocated for voxel attachment data.

                // TODO: Make a function that determines this per attachment so that stuff
                // like PT material allocates a higher default size than emmisiveness.
                const DEFAULT_RAW_ATTACHMENT_ALLOCATION_SIZE: u32 = 8;
                let allocated_raw_ptr = self.esvo_model.allocate_raw_attachment_data(
                    attachment.id(),
                    DEFAULT_RAW_ATTACHMENT_ALLOCATION_SIZE,
                );

                self.parent_node_lookup_mut(attachment.id(), parent_node_index)
                    .set_raw_index(allocated_raw_ptr);
            }

            let leaf_attachment_index = self
                .parent_node_lookup_mut(attachment.id(), parent_node_index)
                .raw_index()
                + old_attachment_mask.count_ones();

            let mut must_swap = false;
            for j in (leaf_octant + 1)..8 {
                if (old_attachment_mask & (1 << j)) > 0 {
                    must_swap = true;
                    break;
                }
            }
            if must_swap {
                todo!("swap ordering since octants of a higher norm must be after this one due to implicit child ordering in the attachment raw data");
            }

            leaf_attachment_index
        } else {
            panic!(
                "attachment already exists at this node and octant {} {}",
                old_attachment_mask, new_attachment_mask
            );
            let child_offset = (old_attachment_mask & ((1 << leaf_octant) - 1)).count_ones();
            self.parent_node_lookup_mut(attachment.id(), parent_node_index)
                .raw_index()
                + child_offset
        };
        let raw_attachment_index = (raw_attachment_index * attachment.size()) as usize;
        let raw_attachment_data = self.esvo_model.get_attachment_raw_data_mut(attachment.id());
        raw_attachment_data
            [raw_attachment_index..(raw_attachment_index + attachment.size() as usize)]
            .copy_from_slice(data);
    }

    fn parent_node_lookup_mut(
        &mut self,
        attachment_id: AttachmentId,
        index: u32,
    ) -> &mut VoxelModelESVOAttachmentLookupNode {
        self.esvo_model
            .get_attachment_lookup_node_mut(attachment_id, index)
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct VoxelModelESVOAttachmentLookupNode(pub u32);

impl VoxelModelESVOAttachmentLookupNode {
    pub const RELATIVE_PTR_BITS: u32 = 15;
    pub const fn encode_lookup(raw_index: u32, attachment_mask: u32) -> Self {
        assert!(raw_index < (1 << 15), "raw index is too big.");
        assert!(attachment_mask < (1 << 8), "attachment mask is too big.");

        Self((raw_index << 8) | attachment_mask)
    }

    pub const fn raw_index(&self) -> u32 {
        self.0 >> 8
    }

    pub fn set_raw_index(&mut self, raw_index: u32) {
        assert!(
            raw_index < (1 << Self::RELATIVE_PTR_BITS),
            "raw index {} is too big.",
            raw_index
        );

        self.0 = (self.0 & 0x0000_00FF) | (raw_index << 8);
    }

    pub const fn attachment_mask(&self) -> u32 {
        self.0 & 0xFF
    }

    pub fn set_attachment_mask(&mut self, attachment_mask: u32) {
        assert!(attachment_mask < (1 << 8), "attachment mask is too big.");

        self.0 = (self.0 & 0xFFFF_FF00) | attachment_mask;
    }

    pub const fn decode(&self) -> (u32, u32) {
        (self.0 >> 8, self.attachment_mask())
    }
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct VoxelModelESVONode(pub u32);

impl Display for VoxelModelESVONode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl VoxelModelESVONode {
    pub const fn encode_data(data: u32) -> Self {
        Self(data)
    }

    pub const fn encode_node(pointer: u32, far: bool, valid_mask: u32, leaf_mask: u32) -> Self {
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

        Self(x)
    }

    pub fn is_far(&self) -> bool {
        ((self.0 >> 16) & 1) > 0
    }

    pub fn relative_ptr(&self) -> u32 {
        self.0 >> 17
    }

    pub fn set_relative_ptr(&mut self, ptr: u32) {
        self.0 &= 0x0001_FFFF;
        self.0 |= ptr << 17;
    }

    pub fn set_leaf_mask(&mut self, leaf_mask: u32) {
        self.0 &= 0xFFFF_FF00;
        self.0 |= leaf_mask;
    }

    pub fn leaf_mask(&self) -> u32 {
        self.0 & 0xFF
    }

    pub fn set_valid_mask(&mut self, valid_mask: u32) {
        self.0 &= 0xFFFF_00FF;
        self.0 |= valid_mask << 8;
    }

    pub fn valid_mask(&self) -> u32 {
        (self.0 >> 8) & 0xFF
    }

    /// (relative child pointer OR relative far pointer pointer,
    ///  bool if the first argument is a far pointer poiner,
    ///  value_mask,
    ///  leaf_mask)
    pub const fn decode(&self) -> (u32, bool, u32, u32) {
        let child_ptr = self.0 >> 17;
        let far = if ((self.0 >> 16) & 1) == 1 {
            true
        } else {
            false
        };
        let value_mask = (self.0 >> 8) & 0xFF;
        let leaf_mask = self.0 & 0xFF;

        (child_ptr, far, value_mask, leaf_mask)
    }
}

#[derive(Clone, Copy, Debug)]
struct ESVONodeMetadataData {
    pub parent_index: u32,
    pub children_capacity: u32,
}

#[derive(Clone, Copy, Debug)]
enum ESVONodeMetadata {
    /// Is allocated and has some node living here.
    Some(ESVONodeMetadataData),
    /// Is allocated and assigned to some parent node but is empty, holds the parent index.
    EmptyChild(u32),
    Free,
}

impl ESVONodeMetadata {
    pub fn expect(&self, message: &str) -> &ESVONodeMetadataData {
        match self {
            ESVONodeMetadata::Some(v) => v,
            _ => panic!("{}", message),
        }
    }

    pub fn unwrap(&self) -> &ESVONodeMetadataData {
        match self {
            ESVONodeMetadata::Some(v) => v,
            _ => panic!("Couldn't unwrap"),
        }
    }

    pub fn unwrap_mut(&mut self) -> &mut ESVONodeMetadataData {
        match self {
            ESVONodeMetadata::Some(v) => v,
            _ => panic!("Couldn't unwrap"),
        }
    }

    pub fn is_some(&self) -> bool {
        match self {
            ESVONodeMetadata::Some(_) => true,
            _ => false,
        }
    }
    pub fn is_empty_child(&self) -> bool {
        match self {
            ESVONodeMetadata::EmptyChild(_) => true,
            _ => false,
        }
    }
}

impl VoxelModelImplConcrete for VoxelModelESVO {
    type Gpu = VoxelModelESVOGpu;
}

impl VoxelModelImpl for VoxelModelESVO {
    fn trace(
        &self,
        ray: &crate::common::ray::Ray,
        aabb: &crate::common::aabb::AABB,
    ) -> Option<VoxelModelTrace> {
        todo!()
    }

    fn set_voxel_range_impl(&mut self, range: &super::voxel::VoxelModelEdit) {
        todo!()
    }

    fn schema(&self) -> VoxelModelSchema {
        consts::voxel::MODEL_ESVO_SCHEMA
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
        const ARRAY_LINE_LIMIT: usize = 50;
        f.write_str(&format!("ESVO height={}\n", self.length.trailing_zeros()));
        for (i, node) in self.node_data.iter().enumerate() {
            if i > ARRAY_LINE_LIMIT {
                break;
            }

            if (i % 8192) == 0 {
                f.write_str(&format!("[{}] Page Header, Block info: {}\n", i, node))?
            } else {
                let (child_ptr, far, value_mask, leaf_mask) = node.decode();
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
                if i > ARRAY_LINE_LIMIT {
                    break;
                }
                let (raw_data_ptr, attachment_mask) = lookup.decode();
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
                    attachment, i, raw_data_ptr, attachment_mask_str,
                ));
            }
        }

        for (attachment, data) in &self.attachment_raw_data {
            f.write_str(&format!(
                "Attachment Raw Data [{}]:\n\t",
                self.attachment_map.name(*attachment)
            ));
            match *attachment {
                Attachment::PTMATERIAL_ID => {
                    for (i, material) in data.iter().enumerate() {
                        if i > ARRAY_LINE_LIMIT {
                            break;
                        }
                        let material = Attachment::decode_ptmaterial(material);
                        f.write_str(&format!("[{}] {:?}, ", i, material));
                    }
                }
                Attachment::NORMAL_ID => {
                    for (i, normal) in data.iter().enumerate() {
                        if i > ARRAY_LINE_LIMIT {
                            break;
                        }
                        let normal = Attachment::decode_normal(*normal);
                        f.write_str(&format!("[{}] {} {} {}, ", i, normal.x, normal.y, normal.z));
                    }
                }
                Attachment::EMMISIVE_ID => {
                    for (i, emmisive) in data.iter().enumerate() {
                        if i > ARRAY_LINE_LIMIT {
                            break;
                        }
                        let emmissiveness = Attachment::decode_emissive(*emmisive);
                        f.write_str(&format!("[{}] {}", i, emmissiveness));
                    }
                }
                default => {}
            }
            f.write_str("\n");
        }

        f.write_str("")
    }
}

pub struct VoxelModelESVOGpu {
    data_allocation: Option<Allocation>,
    attachment_lookup_allocations: HashMap<AttachmentId, Allocation>,
    attachment_raw_allocations: HashMap<AttachmentId, Allocation>,

    initialized_data: bool,
}

impl VoxelModelGpuImpl for VoxelModelESVOGpu {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        let Some(data_allocation) = &self.data_allocation else {
            return None;
        };
        if self.attachment_lookup_allocations.is_empty()
            || self.attachment_raw_allocations.is_empty()
        {
            return None;
        }

        let mut attachment_lookup_indices =
            vec![u32::MAX; Attachment::MAX_ATTACHMENT_ID as usize + 1];
        for (attachment, lookup_allocation) in &self.attachment_lookup_allocations {
            if *attachment > Attachment::MAX_ATTACHMENT_ID {
                continue;
            }

            attachment_lookup_indices[*attachment as usize] =
                lookup_allocation.start_index_stride_dword() as u32
        }
        let mut attachment_raw_indices = vec![u32::MAX; Attachment::MAX_ATTACHMENT_ID as usize + 1];
        for (attachment, raw_allocation) in &self.attachment_raw_allocations {
            if *attachment > Attachment::MAX_ATTACHMENT_ID {
                continue;
            }

            attachment_raw_indices[*attachment as usize] =
                raw_allocation.start_index_stride_dword() as u32;
        }

        let mut info = vec![
            // World data ptr (divide by 4 since 4 bytes in a u32)
            data_allocation.start_index_stride_dword() as u32,
        ];
        info.append(&mut attachment_lookup_indices);
        info.append(&mut attachment_raw_indices);

        Some(info)
    }

    fn update_gpu_objects(
        &mut self,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool {
        let model = model.downcast_ref::<VoxelModelESVO>().unwrap();
        let mut did_allocate = false;

        if self.data_allocation.is_none() {
            let data_allocation_size = model.node_data.len() as u64 * 4;
            self.data_allocation = Some(
                allocator
                    .allocate(data_allocation_size)
                    .expect("Failed to allocate ESVO node data."),
            );
            did_allocate = true;
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
                did_allocate = true;
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
                did_allocate = true;
            }
        }

        return did_allocate;
    }

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        let model = model.downcast_ref::<VoxelModelESVO>().unwrap();

        // If data allocation is some and we haven't initialized yet, expected the attachment data
        // to also be ready.
        if !self.initialized_data && self.data_allocation.is_some() {
            // debug!("Writing ESVO voxel model initial data");

            // debug!("Writing node data {:?}", model.node_data.as_slice());
            allocator.write_allocation_data(
                device,
                self.data_allocation.as_ref().unwrap(),
                bytemuck::cast_slice::<VoxelModelESVONode, u8>(model.node_data.as_slice()),
            );

            for (attachment, lookup_data) in &model.attachment_lookup_data {
                // debug!(
                //     "Writing attachment lookup data [{}] {:?}",
                //     attachment.name(),
                //     lookup_data.as_slice()
                // );
                let allocation = self
                    .attachment_lookup_allocations
                    .get(attachment)
                    .expect("Lookup allocation should exist by now.");

                allocator.write_allocation_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<VoxelModelESVOAttachmentLookupNode, u8>(
                        lookup_data.as_slice(),
                    ),
                );
            }

            for (attachment, raw_data) in &model.attachment_raw_data {
                // debug!(
                //     "Writing attachment raw data [{}] Len: {:?}",
                //     attachment,
                //     raw_data.as_slice().len()
                // );
                let allocation = self
                    .attachment_raw_allocations
                    .get(attachment)
                    .expect("Raw allocation should exist by now.");

                allocator.write_allocation_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(raw_data.as_slice()),
                );
            }

            self.initialized_data = true;
            return;
        }

        // If we are here, we are now incrementally updating the gpu buffer data given updates sent
        // from the voxel model of memory slices that have changed.
        if let Some(updates) = &model.updates {
            if !updates.is_empty() {
                todo!("Process GPU updates")
            }
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
pub struct BucketMetadata {
    metadata_index: u32,
    node_capacity: u32,
    node_size: u32,

    // Pointers relative to start of esvo data, aka. 0 == node_data[0].
    bucket_absolute_start: u32,
    bucket_node_start: u32,
    bucket_free_start: u32,
    bucket_info_start: u32,
    bucket_total_size: u32,
}

impl BucketMetadata {
    pub fn empty(
        metadata_index: u32,
        mut start_offset: u32,
        desired_bucket_size: u32,
        bucket_info_size: u32,
    ) -> Self {
        assert!(desired_bucket_size.is_power_of_two());
        assert!(
            (start_offset % VoxelModelESVO::PAGE_SIZE) == 0,
            "Bucket should always start with a page header."
        );

        let bucket_info_start = start_offset + desired_bucket_size - bucket_info_size;
        let mut page_header_count = (start_offset..bucket_info_start)
            .step_by(VoxelModelESVO::PAGE_SIZE as usize)
            .count() as u32;
        let node_capacity = desired_bucket_size - bucket_info_size - page_header_count;

        Self {
            metadata_index,
            node_capacity,
            node_size: 0,
            bucket_absolute_start: start_offset,
            bucket_node_start: start_offset + 1,
            bucket_free_start: start_offset + 1,
            bucket_info_start,
            bucket_total_size: desired_bucket_size,
        }
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
