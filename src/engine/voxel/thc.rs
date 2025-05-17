use std::{collections::HashMap, time::Duration};

use log::debug;
use nalgebra::Vector3;
use petgraph::matrix_graph::Zero;

use crate::{
    common::{
        bitset::Bitset,
        color::Color,
        morton::{self, morton_decode},
    },
    consts,
    engine::{
        graphics::{
            device::GfxDevice,
            gpu_allocator::{Allocation, GpuBufferAllocator},
        },
        voxel::attachment::{self, PTMaterial},
    },
};

use super::{
    attachment::{Attachment, AttachmentId, AttachmentInfoMap, AttachmentMap},
    flat::VoxelModelFlat,
    voxel::{
        VoxelModelEdit, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
        VoxelModelImplConcrete, VoxelModelTrace, VoxelModelType,
    },
};

// Tetrahexacontree, aka., 64-tree. Essentially an octree where each node is
// two octree nodes squashed together, resulting in 64 children in each node.
#[derive(Clone)]
pub struct VoxelModelTHC {
    pub side_length: u32,
    pub node_data: Vec<THCNode>,
    pub attachment_lookup_data: AttachmentMap<Vec<THCAttachmentLookupNode>>,
    pub attachment_raw_data: AttachmentMap<Vec<u32>>,
    pub attachment_map: AttachmentInfoMap,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct THCNode {
    // Left most bit determines if this node is a leaf.
    pub child_ptr: u32,
    pub child_mask: u64,
}

impl std::fmt::Debug for THCNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut child_mask = String::with_capacity(64);
        for i in 0usize..64 {
            child_mask.insert(
                i,
                if (self.child_mask & (1 << i)) > 0 {
                    '1'
                } else {
                    '0'
                },
            );
        }
        f.debug_struct("THCNode")
            .field("child_ptr", &self.child_ptr)
            .field("child_mask", &child_mask)
            .finish()
    }
}

impl THCNode {
    pub fn new_empty() -> Self {
        Self {
            child_ptr: 0,
            child_mask: 0,
        }
    }

    pub fn is_leaf_node(&self) -> bool {
        (self.child_ptr >> 31) > 0
    }

    pub fn child_ptr(&self) -> u32 {
        self.child_ptr & 0x7FFF_FFFF
    }

    pub fn has_child(&self, child_index: u32) -> bool {
        (self.child_mask & (1 << child_index)) > 0
    }
}

#[derive(Clone)]
pub struct THCAttachmentLookupNode {
    pub data_ptr: u32,
    // A mask designating which children have the attachment.
    pub attachment_mask: u64,
}

impl THCAttachmentLookupNode {
    pub fn data_ptr(&self) -> u32 {
        self.data_ptr
    }

    pub fn has_child(&self, child_index: u32) -> bool {
        (self.attachment_mask & (1 << child_index)) > 0
    }
}

impl VoxelModelTHC {
    pub fn new_empty(length: u32) -> Self {
        assert_eq!(
            Self::next_power_of_4(length),
            length,
            "Length for a THC must be a power of 4."
        );
        assert!(length >= 4, "Length for a THC must be atleast 4.");
        Self {
            side_length: length,
            node_data: vec![THCNode::new_empty()],
            attachment_lookup_data: AttachmentMap::new(),
            attachment_raw_data: AttachmentMap::new(),
            attachment_map: AttachmentMap::new(),
        }
    }

    pub fn next_power_of_4(x: u32) -> u32 {
        let x = x.next_power_of_two();
        if (x.trailing_zeros() % 2 == 0) {
            return x;
        }
        return x << 1;
    }

    pub fn tree_height(&self) -> u32 {
        self.side_length.trailing_zeros() / 2
    }

    pub fn get_or_create_preleaf(
        &mut self,
        local_position: Vector3<u32>,
    ) -> (
        /*idx of preleaf*/ usize,
        /*index into child mask*/ u32,
    ) {
        let mut traversal = morton::morton_traversal(
            morton::morton_encode(local_position),
            self.side_length.trailing_zeros(),
        );

        let mut curr_height = 0;
        let mut curr_node = 0;
        loop {
            let curr_idx = traversal & 0b111111;
            if curr_height + 1 == self.tree_height() {
                return (curr_node, curr_idx as u32);
            } else {
                let n = &self.node_data[curr_node];
                let is_child_present = (n.child_mask & (1 << curr_idx)) > 0;
                if is_child_present {
                } else {
                }
            }

            curr_height += 1;
            traversal <<= 6;
        }
    }

    pub fn set_voxel_attachment(
        &mut self,
        local_position: Vec<u32>,
        attachment_id: AttachmentId,
        data: Option<Vec<u32>>,
    ) {
    }
}

impl VoxelModelImplConcrete for VoxelModelTHC {
    type Gpu = VoxelModelTHCGpu;

    fn model_type() -> Option<VoxelModelType> {
        Some(VoxelModelType::THC)
    }
}

impl VoxelModelImpl for VoxelModelTHC {
    fn trace(
        &self,
        ray: &crate::common::ray::Ray,
        aabb: &crate::common::aabb::AABB,
    ) -> Option<VoxelModelTrace> {
        return None;
        //todo!()
    }

    fn set_voxel_range_impl(&mut self, range: &VoxelModelEdit) {
        todo!();
    }

    fn schema(&self) -> super::voxel::VoxelModelSchema {
        consts::voxel::MODEL_THC_SCHEMA
    }

    fn length(&self) -> nalgebra::Vector3<u32> {
        Vector3::new(self.side_length, self.side_length, self.side_length)
    }
}

pub struct VoxelModelTHCGpu {
    // Model side length in voxels.
    side_length: u32,
    nodes_allocation: Option<Allocation>,
    attachment_lookup_allocations: HashMap<AttachmentId, Allocation>,
    attachment_raw_allocations: HashMap<AttachmentId, Allocation>,

    initialized_model_data: bool,
}

impl VoxelModelTHCGpu {}

impl VoxelModelGpuImplConcrete for VoxelModelTHCGpu {
    fn new() -> Self {
        Self {
            side_length: 0,
            nodes_allocation: None,
            attachment_lookup_allocations: HashMap::new(),
            attachment_raw_allocations: HashMap::new(),

            initialized_model_data: false,
        }
    }
}

impl VoxelModelGpuImpl for VoxelModelTHCGpu {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        let Some(data_allocation) = &self.nodes_allocation else {
            return None;
        };
        if self.attachment_lookup_allocations.is_empty()
            || self.attachment_raw_allocations.is_empty()
        {
            log::info!("no attachments");
            return None;
        }
        if self.side_length == 0 {
            log::info!("no length");
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

            // debug!(
            //     "Uploading indices {}",
            //     raw_allocation.start_index_stride_dword() as u32
            // );
            attachment_raw_indices[*attachment as usize] =
                raw_allocation.start_index_stride_dword() as u32;
        }

        let mut info = vec![
            self.side_length,
            // Node ptr (divide by 4 since 4 bytes in a u32)
            data_allocation.start_index_stride_dword() as u32,
        ];
        info.append(&mut attachment_lookup_indices);
        info.append(&mut attachment_raw_indices);

        Some(info)
    }

    fn update_gpu_objects(
        &mut self,
        allocator: &mut crate::engine::graphics::gpu_allocator::GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool {
        let model = model.downcast_ref::<VoxelModelTHC>().unwrap();
        let mut did_allocate = false;

        if self.nodes_allocation.is_none() {
            let nodes_allocation_size = model.node_data.len() as u64 * 12;
            self.nodes_allocation = Some(
                allocator
                    .allocate(nodes_allocation_size)
                    .expect("Failed to allocate THC node data."),
            );
            did_allocate = true;
        }

        for (attachment, data) in model.attachment_lookup_data.iter() {
            if !self.attachment_lookup_allocations.contains_key(&attachment) {
                let lookup_data_allocation_size = data.len() as u64 * 12;
                self.attachment_lookup_allocations.insert(
                    attachment.clone(),
                    allocator
                        .allocate(lookup_data_allocation_size)
                        .expect("Failed to allocate ESVO attachment lookup data."),
                );
                did_allocate = true;
            }
        }

        for (attachment, data) in model.attachment_raw_data.iter() {
            if !self.attachment_raw_allocations.contains_key(&attachment) {
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

        // Add implicit normal attachment.
        if !model.attachment_lookup_data.contains(Attachment::NORMAL_ID)
            && model
                .attachment_lookup_data
                .contains(Attachment::PTMATERIAL_ID)
        {
            if model.attachment_map.contains(Attachment::NORMAL_ID) {
                assert!(
                    model.attachment_map.get_unchecked(Attachment::NORMAL_ID)
                        == &Attachment::NORMAL
                );
            }

            let ptmaterial_data_size = model
                .attachment_raw_data
                .get(Attachment::PTMATERIAL_ID)
                .unwrap()
                .len() as u64;
            let req_data_allocation_size = (ptmaterial_data_size
                / Attachment::PTMATERIAL.size() as u64)
                * Attachment::NORMAL.size() as u64
                * 4;
            match self.attachment_raw_allocations.entry(Attachment::NORMAL_ID) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    let old_allocation = e.get();
                    if old_allocation.length_bytes() < req_data_allocation_size {
                        let new_allocation = allocator
                            .reallocate(e.get(), req_data_allocation_size)
                            .expect("Failed to reallocate flat attachment raw data.");

                        if old_allocation.start_index_stride_bytes()
                            != new_allocation.start_index_stride_bytes()
                        {
                            did_allocate = true;
                        }
                        e.insert(new_allocation);
                    }
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(
                        allocator
                            .allocate(req_data_allocation_size as u64)
                            .expect("Failed to allocate flat attachment raw data."),
                    );
                    did_allocate = true;
                }
            }
        }

        if self.side_length != model.side_length {
            self.side_length = model.side_length;
            // We don't technically allocate anything if this changes, however we
            // return true so the model info entry is updated.
            did_allocate = true;
        }

        return did_allocate;
    }

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        let model = model.downcast_ref::<VoxelModelTHC>().unwrap();

        // If data allocation is some and we haven't initialized yet, expected the attachment data
        // to also be ready.
        if !self.initialized_model_data && self.nodes_allocation.is_some() {
            {
                let mut node_data_packed = Vec::with_capacity(model.node_data.len() * 3);
                for node in &model.node_data {
                    node_data_packed.push(node.child_ptr);
                    // Little endian.
                    node_data_packed.push((node.child_mask & 0xFFFF_FFFF) as u32);
                    node_data_packed.push((node.child_mask >> 32) as u32);
                }

                let node_data_bytes = bytemuck::cast_slice::<u32, u8>(&node_data_packed);
                allocator.write_allocation_data(
                    device,
                    self.nodes_allocation.as_ref().unwrap(),
                    node_data_bytes,
                );
            }

            for (attachment_id, lookup_data) in model.attachment_lookup_data.iter() {
                let allocation = self
                    .attachment_lookup_allocations
                    .get(&attachment_id)
                    .expect("Lookup allocation should exist by now.");

                let mut lookup_data_packed = Vec::with_capacity(lookup_data.len() * 3);
                for lookup in lookup_data {
                    lookup_data_packed.push(lookup.data_ptr);
                    // Little endian.
                    lookup_data_packed.push((lookup.attachment_mask & 0xFFFF_FFFF) as u32);
                    lookup_data_packed.push((lookup.attachment_mask >> 32) as u32);
                }
                let lookup_data_bytes = bytemuck::cast_slice::<u32, u8>(&lookup_data_packed);
                allocator.write_allocation_data(device, allocation, lookup_data_bytes);
            }

            for (attachment, raw_data) in model.attachment_raw_data.iter() {
                let allocation = self
                    .attachment_raw_allocations
                    .get(&attachment)
                    .expect("Raw allocation should exist by now.");
                //debug!("raw data {:?}", &raw_data[0..32]);

                allocator.write_allocation_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(raw_data.as_slice()),
                );
            }

            self.initialized_model_data = true;
            return;
        }

        // If we are here, we are now incrementally updating the gpu buffer data given updates sent
        // from the voxel model of memory slices that have changed.
        //if let Some(updates) = &model.updates {
        //    if !updates.is_empty() {
        //        todo!("Process GPU updates")
        //    }
        //}
    }
}

impl From<VoxelModelFlat> for VoxelModelTHC {
    fn from(value: VoxelModelFlat) -> Self {
        From::<&VoxelModelFlat>::from(&value)
    }
}

impl From<&VoxelModelFlat> for VoxelModelTHC {
    fn from(flat: &VoxelModelFlat) -> Self {
        let length = flat
            .side_length()
            .map(|x| VoxelModelTHC::next_power_of_4(x))
            .max()
            .max(4);
        let volume = (length as u64).pow(3);
        // With just the root node being a height of 1, since log4(4) == log2(4) / 2 == 1.
        let height = length.trailing_zeros() / 2;

        let mut levels: Vec<Vec<Option<THCNode>>> =
            (0..=height).map(|_| Vec::new()).collect::<Vec<_>>();
        let mut node_list_rev: Vec<THCNode> = Vec::new();
        for i in 0..volume {
            let pos = morton_decode(i);
            if !flat.in_bounds(pos) || !flat.get_voxel(pos).exists() {
                levels[height as usize].push(None);
            } else {
                levels[height as usize].push(Some(THCNode::new_empty()));
            }

            for h in (1..=height).rev() {
                let curr_level = &mut levels[h as usize];
                if curr_level.len() != 64 {
                    break;
                }

                // Ensure we push nodes in reverse order and store the child pointer since we reverse the lis
                let mut child_mask = 0u64;
                let mut child_ptr = u32::MAX;
                for (morton, node) in curr_level.drain(..).enumerate().rev() {
                    let Some(node) = node else {
                        continue;
                    };
                    child_mask |= 1 << morton;

                    // Don't push leaf layer.
                    if h == height {
                        continue;
                    }
                    child_ptr = node_list_rev.len() as u32;
                    node_list_rev.push(node.clone());
                }

                if child_mask != 0 {
                    let child_ptr = (child_ptr != u32::MAX)
                        .then_some(child_ptr)
                        .unwrap_or(0x8000_0000);
                    levels[h as usize - 1].push(Some(THCNode {
                        child_ptr,
                        child_mask,
                    }));
                } else {
                    levels[h as usize - 1].push(None);
                }
            }
        }
        let root_node = levels[0][0].clone().unwrap_or(THCNode::new_empty());
        if root_node.child_mask == 0 {
            return VoxelModelTHC::new_empty(length);
        }
        node_list_rev.push(root_node);

        // Flip the list around so the root node is first.
        let node_data_len = node_list_rev.len() as u32;
        assert!(node_data_len < 0x8000_0000);
        let mut node_data = node_list_rev
            .into_iter()
            .map(|mut node| {
                node.child_ptr = (node.child_ptr & 0x8000_0000)
                    | (node_data_len - 1 - (node.child_ptr & 0x7FFF_FFFF));
                if node.child_ptr == 0x8000_0000 {
                    node.child_mask = node.child_mask.reverse_bits();
                }
                node
            })
            .collect::<Vec<_>>();
        node_data.reverse();

        // Allocated up here to prevent reallocation in the while loop below.
        let mut attachment_lookup: HashMap<AttachmentId, (Option<u32>, u64)> = HashMap::new();

        let mut attachment_lookup_data = AttachmentMap::new();
        let mut attachment_raw_data = AttachmentMap::new();
        for (present_attachment, _) in flat.attachment_presence_data.iter() {
            attachment_lookup.insert(present_attachment, (None, 0));
            attachment_lookup_data.insert(
                present_attachment,
                vec![
                    THCAttachmentLookupNode {
                        data_ptr: 0,
                        attachment_mask: 0
                    };
                    node_data_len as usize
                ],
            );
            attachment_raw_data.insert(present_attachment, Vec::new());
        }

        let mut to_process = vec![(
            0,
            node_data.first().unwrap(),
            /*morton_traversal=*/ 0u64,
        )];
        while !to_process.is_empty() {
            let (curr_node_index, curr_node, curr_morton_traversal) = to_process.pop().unwrap();

            // Process internal node.
            let is_leaf = curr_node.child_ptr >> 31 == 1;
            if !is_leaf {
                for child in 0..64usize {
                    let child_bit = 1u64 << (63 - child);
                    let is_present = (curr_node.child_mask & child_bit) > 0;
                    if !is_present {
                        continue;
                    }

                    let child_offset = (curr_node.child_mask & (child_bit - 1)).count_ones();
                    let child_index = curr_node.child_ptr + child_offset;
                    let child_morton_traversal = (curr_morton_traversal << 6) | (63 - child) as u64;
                    to_process.push((
                        child_index as usize,
                        &node_data[child_index as usize],
                        child_morton_traversal,
                    ));
                }

                continue;
            }
            if curr_node.child_mask == 0 {
                continue;
            }

            for (_, (raw_ptr, attachment_mask)) in &mut attachment_lookup {
                *raw_ptr = None;
                *attachment_mask = 0u64;
            }
            for child in 0..64usize {
                let child_bit = 1u64 << child;
                let is_voxel_present = (curr_node.child_mask & child_bit) > 0;
                if !is_voxel_present {
                    continue;
                }

                // Append the voxels flat data from the
                let voxel_morton = curr_morton_traversal << 6 | child as u64;
                let voxel_pos = morton_decode(voxel_morton);
                for (attachment_id, presence_bitset) in flat.attachment_presence_data.iter() {
                    let flat_voxel_index = flat.get_voxel_index(voxel_pos);
                    //debug!("voxel pos {:?}", voxel_pos);
                    let is_attachment_present = presence_bitset.get_bit(flat_voxel_index);
                    if !is_attachment_present {
                        //debug!("attachment {} not present", attachment.name());
                        //debug!("bitset is {:?}", presence_bitset.data());
                        continue;
                    }

                    let attachment = flat.attachment_map.get_unchecked(attachment_id);
                    let (attachment_raw_ptr, attachment_mask) =
                        attachment_lookup.get_mut(&attachment_id).unwrap();
                    if attachment_raw_ptr.is_none() {
                        *attachment_raw_ptr =
                            Some(attachment_raw_data.get(attachment_id).unwrap().len() as u32);
                    }
                    *attachment_mask |= child_bit;

                    // Write voxexl attachment data.
                    let flat_raw_attachment_data =
                        flat.attachment_data.get(attachment_id).unwrap().as_slice();
                    let voxel_raw_attachment_data_start =
                        (flat_voxel_index * attachment.size() as usize);
                    let voxel_raw_attachment_data = &flat_raw_attachment_data
                        [voxel_raw_attachment_data_start
                            ..(voxel_raw_attachment_data_start + attachment.size() as usize)];
                    attachment_raw_data
                        .get_mut(attachment_id)
                        .unwrap()
                        .extend_from_slice(voxel_raw_attachment_data);
                    //attachment_raw_data
                    //    .get_mut(&attachment.id())
                    //    .unwrap()
                    //    .extend_from_slice(&[Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                    //        Color::new_srgb(
                    //            voxel_pos.x as f32 / 64.0,
                    //            voxel_pos.y as f32 / 64.0,
                    //            voxel_pos.z as f32 / 64.0,
                    //        ),
                    //    ))]);
                }
            }

            // Update attachment lookup nodes.
            for (attachment_id, (raw_ptr, attachment_mask)) in &mut attachment_lookup {
                let Some(raw_ptr) = raw_ptr else {
                    continue;
                };
                //debug!(
                //    "raw attachment ptr for {} and morton {} and node idxx {}",
                //    raw_ptr, curr_morton_traversal, curr_node_index
                //);

                //debug!("Settings index {} for raw ptr {}", curr_node_index, raw_ptr);
                attachment_lookup_data.get_mut(*attachment_id).unwrap()[curr_node_index] =
                    THCAttachmentLookupNode {
                        data_ptr: *raw_ptr,
                        attachment_mask: *attachment_mask,
                    };
            }
        }

        VoxelModelTHC {
            side_length: length,
            node_data,
            attachment_lookup_data,
            attachment_raw_data,
            attachment_map: flat.attachment_map.clone(),
        }
    }
}
