use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    time::Duration,
};

use log::debug;
use nalgebra::Vector3;
use petgraph::matrix_graph::Zero;

use crate::{
    common::{
        aabb::AABB,
        bitset::Bitset,
        color::Color,
        morton::{
            self, morton_decode, morton_encode, morton_traversal_octree, morton_traversal_thc,
        },
        ray::Ray,
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
        VoxelMaterialSet, VoxelModelEdit, VoxelModelGpuImpl, VoxelModelGpuImplConcrete,
        VoxelModelImpl, VoxelModelImplConcrete, VoxelModelSchema, VoxelModelTrace, VoxelModelType,
    },
    voxel_world::{VoxelDataAllocation, VoxelDataAllocator},
};

#[derive(Clone)]
pub struct VoxelModelTHC {
    side_length: u32,
    attachment_map: AttachmentInfoMap,
    root_node: Box<VoxelModelTHCNode>,
    update_tracker: u32,
}

#[derive(Clone)]
pub enum VoxelModelTHCNode {
    Internal {
        children: [Option<Box<VoxelModelTHCNode>>; 64],
    },
    Preleaf {
        leaf_mask: u64,
        attachment_data: AttachmentMap<(
            /*attachment_mask*/ u64,
            /*attachment_data*/ Vec<u32>,
        )>,
    },
}

impl VoxelModelTHCNode {
    pub fn new_empty_internal() -> Self {
        Self::Internal {
            children: [const { None }; 64],
        }
    }

    pub fn new_empty_preleaf() -> Self {
        Self::Preleaf {
            leaf_mask: 0,
            attachment_data: AttachmentMap::new(),
        }
    }

    pub fn set_attachment(
        &mut self,
        child_idx: u32,
        attachment_id: u8,
        attachment_size: usize,
        data: &[u32],
    ) {
        assert_eq!(attachment_size, data.len());
        match self {
            VoxelModelTHCNode::Internal { children } => panic!(),
            VoxelModelTHCNode::Preleaf {
                leaf_mask,
                attachment_data,
            } => {
                let Some((attachment_mask, attachment_data)) =
                    attachment_data.get_mut(attachment_id)
                else {
                    attachment_data.insert(attachment_id, ((1 << child_idx), data.to_vec()));
                    return;
                };
                let child_bit = (1 << child_idx);
                let child_offset =
                    (*attachment_mask & (child_bit - 1)).count_ones() as usize * attachment_size;
                if (*attachment_mask & child_bit) > 0 {
                    // Overwrite existing attachment.
                    for i in 0..attachment_size {
                        attachment_data[child_offset + i] = data[i];
                    }
                    return;
                }

                for i in 0..attachment_size {
                    attachment_data.insert(child_offset, data[i]);
                }
                *attachment_mask |= (1 << child_idx);
            }
        }
    }
}

impl VoxelModelTHC {
    pub fn new_empty(side_length: u32) -> Self {
        assert_eq!(
            next_power_of_4(side_length),
            side_length,
            "Length for a THC must be a power of 4."
        );
        assert!(side_length >= 4, "Length for a THC must be atleast 4.");

        let root_node = if side_length == 4 {
            VoxelModelTHCNode::new_empty_preleaf()
        } else {
            VoxelModelTHCNode::new_empty_internal()
        };
        Self {
            side_length,
            attachment_map: AttachmentInfoMap::new(),
            root_node: Box::new(root_node),
            update_tracker: 0,
        }
    }

    pub fn in_bounds_local(&self, local_position: Vector3<i32>) -> bool {
        return local_position.x >= 0
            && local_position.y >= 0
            && local_position.z >= 0
            && local_position.x < self.side_length as i32
            && local_position.y < self.side_length as i32
            && local_position.z < self.side_length as i32;
    }

    pub fn tree_height(&self) -> u32 {
        self.side_length.trailing_zeros() / 2
    }

    pub fn get_or_create_preleaf(
        &mut self,
        local_voxel_pos: Vector3<u32>,
    ) -> (&mut VoxelModelTHCNode, /*child_idx=*/ u32) {
        let height = self.tree_height();
        let mut traversal = morton_traversal_thc(morton_encode(local_voxel_pos), height);
        let mut curr_node = &mut self.root_node;
        for i in 0..height {
            let index = ((traversal >> (i * 6)) & 0b111111) as u32;
            if i == height - 1 {
                return (curr_node, index);
            } else {
                let mut new_node;
                match curr_node.deref_mut() {
                    VoxelModelTHCNode::Internal { children } => {
                        new_node = children[index as usize].get_or_insert_with(|| {
                            if i < height.saturating_sub(2) {
                                Box::new(VoxelModelTHCNode::new_empty_internal())
                            } else {
                                Box::new(VoxelModelTHCNode::new_empty_preleaf())
                            }
                        });
                    }
                    VoxelModelTHCNode::Preleaf { .. } => unreachable!(),
                }
                curr_node = new_node;
            }
        }

        panic!();
    }
}

impl From<&VoxelModelTHC> for VoxelModelTHCCompressed {
    fn from(thc: &VoxelModelTHC) -> Self {
        let mut compressed = VoxelModelTHCCompressed::new_empty(thc.side_length);
        for (_, attachment) in thc.attachment_map.iter() {
            compressed.initialize_attachment_buffers(attachment);
        }

        let mut stack = vec![(
            &thc.root_node,
            /*curr_compressed_node*/ 0,
            /*curr_child_iter*/ 0,
        )];
        compressed.node_data.push(THCNodeCompressed::new_empty());
        while !stack.is_empty() {
            let curr_node_index = stack.len() - 1;
            let (curr_node, compressed_node_idx, curr_child_iter) = &stack[curr_node_index];

            let compressed_node_idx = *compressed_node_idx;
            let curr_child_iter = *curr_child_iter;

            match (*curr_node).deref() {
                VoxelModelTHCNode::Internal { children } => {
                    if curr_child_iter == 64 {
                        stack.pop();
                        continue;
                    }

                    if curr_child_iter == 0 {
                        let mut child_mask = 0u64;
                        let children_allocation_index = compressed.node_data.len();
                        for i in 0..64 {
                            if children[i].is_none() {
                                continue;
                            }
                            child_mask |= (1 << i);
                        }
                        compressed.node_data[compressed_node_idx] = THCNodeCompressed {
                            child_ptr: children_allocation_index as u32,
                            child_mask,
                        };
                        compressed.node_data.resize(
                            children_allocation_index + child_mask.count_ones() as usize,
                            THCNodeCompressed::new_empty(),
                        );
                    }
                    if let Some(next_child) = &children[curr_child_iter] {
                        let curr_compressed_node = &compressed.node_data[compressed_node_idx];
                        assert!(curr_compressed_node.child_ptr() != 0);

                        let child_offset = (curr_compressed_node.child_mask
                            & ((1 << curr_child_iter) - 1))
                            .count_ones();
                        stack.push((
                            next_child,
                            (curr_compressed_node.child_ptr() + child_offset) as usize,
                            0,
                        ));
                    }

                    let (_, _, curr_child_iter) = &mut stack[curr_node_index];
                    *curr_child_iter += 1;
                }
                VoxelModelTHCNode::Preleaf {
                    leaf_mask,
                    attachment_data,
                } => {
                    compressed.node_data[compressed_node_idx] = THCNodeCompressed {
                        child_ptr: 0x8000_0000,
                        child_mask: *leaf_mask,
                    };
                    for (attachment_id, (src_attachment_mask, src_attachment_data)) in
                        attachment_data.iter()
                    {
                        let lookup_nodes = compressed
                            .attachment_lookup_data
                            .get_mut(attachment_id)
                            .unwrap();
                        if lookup_nodes.len() < compressed.node_data.len() {
                            lookup_nodes.resize(
                                compressed.node_data.len(),
                                THCAttachmentLookupNodeCompressed::new_empty(),
                            );
                        }
                        let dst_attachment_data = compressed
                            .attachment_raw_data
                            .get_mut(attachment_id)
                            .unwrap();
                        lookup_nodes[compressed_node_idx] = THCAttachmentLookupNodeCompressed {
                            data_ptr: dst_attachment_data.len() as u32,
                            attachment_mask: *src_attachment_mask,
                        };
                        dst_attachment_data.extend_from_slice(src_attachment_data);
                    }
                    stack.pop();
                    continue;
                }
            }
        }

        if thc.side_length == 16 {
            log::info!("Result is node data:");
            for node in &compressed.node_data {
                log::info!("{:?}", node);
            }
        }

        return compressed;
    }
}

impl From<&VoxelModelTHCCompressed> for VoxelModelTHC {
    fn from(compressed: &VoxelModelTHCCompressed) -> Self {
        let mut root_node = Box::new(if compressed.node_data[0].is_leaf_node() {
            VoxelModelTHCNode::new_empty_preleaf()
        } else {
            VoxelModelTHCNode::new_empty_internal()
        });
        let mut to_process = vec![(0, 0u64, 0usize, &mut root_node)];
        while let Some((curr_height, traversal, compressed_node_index, node)) = to_process.pop() {
            let compressed_node = &compressed.node_data[compressed_node_index];
            match node.deref_mut() {
                VoxelModelTHCNode::Internal { children } => {
                    assert!(!compressed_node.is_leaf_node());
                    for (i, child) in children.iter_mut().enumerate() {
                        let child_bit = (1u64 << i);
                        let is_present = (compressed_node.child_mask & child_bit) > 0;
                        if !is_present {
                            continue;
                        }

                        *child = Some(Box::new(if curr_height >= compressed.tree_height() - 2 {
                            VoxelModelTHCNode::new_empty_preleaf()
                        } else {
                            VoxelModelTHCNode::new_empty_internal()
                        }));
                        let compressed_child_offset =
                            (compressed_node.child_mask & (child_bit - 1)).count_ones() as usize;
                        to_process.push((
                            curr_height + 1,
                            (traversal << 6) | i as u64,
                            compressed_node.child_ptr() as usize + compressed_child_offset,
                            child.as_mut().unwrap(),
                        ));
                    }
                }
                VoxelModelTHCNode::Preleaf {
                    leaf_mask,
                    attachment_data,
                } => {
                    assert!(compressed_node.is_leaf_node());
                    *leaf_mask = compressed_node.child_mask;
                    for (attachment_id, lookup_nodes) in compressed.attachment_lookup_data.iter() {
                        // Lookup node may not be present since we don't always resize lookup buffer to save space.
                        let Some(THCAttachmentLookupNodeCompressed {
                            data_ptr,
                            attachment_mask,
                        }) = lookup_nodes.get(compressed_node_index)
                        else {
                            continue;
                        };

                        let data_ptr = *data_ptr as usize;
                        attachment_data.insert(
                            attachment_id,
                            (
                                *attachment_mask,
                                compressed.attachment_raw_data.get(attachment_id).unwrap()
                                    [data_ptr..(data_ptr + attachment_mask.count_ones() as usize)]
                                    .to_vec(),
                            ),
                        );
                    }
                }
            }
        }

        return VoxelModelTHC {
            side_length: compressed.side_length,
            root_node,
            attachment_map: compressed.attachment_map.clone(),
            update_tracker: 0,
        };
    }
}

impl VoxelModelImpl for VoxelModelTHC {
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace> {
        let original_pos = ray.origin;
        let mut ray = ray.clone();
        let Some(model_t) = ray.intersect_aabb(aabb) else {
            return None;
        };
        ray.advance(model_t);

        let local_pos = ray.origin - aabb.min;
        let norm_pos = local_pos.zip_map(&aabb.side_length(), |x, y| (x / y).clamp(0.0, 0.9999));
        // Our scaled position from [0, bounds).
        let dda_pos = norm_pos * self.side_length as f32;

        let height = self.tree_height() - 1;
        let sl = self.side_length;
        let quarter_sl = self.side_length >> 2;
        let unit_grid = ray.dir.map(|x| x.signum() as i32);

        let mut curr_ray = Ray::new(dda_pos, ray.dir);
        let mut curr_node = &self.root_node;
        let mut curr_height = 0;
        let mut curr_local_grid = curr_ray
            .origin
            .map(|x| (x.floor() as u32 >> (height * 2)) as i32);
        let mut curr_anchor = Vector3::<u32>::zeros();
        // Don't include the leaf layer in the height.
        let mut stack = Vec::new();
        let mut i = 0;
        while self.in_bounds_local(curr_ray.origin.map(|x| x.floor() as i32)) && (i < 2000) {
            i += 1;
            let should_pop = !(curr_local_grid.x >= 0
                && curr_local_grid.y >= 0
                && curr_local_grid.z >= 0
                && curr_local_grid.x <= 3
                && curr_local_grid.y <= 3
                && curr_local_grid.z <= 3);
            if should_pop {
                if curr_height == 0 {
                    break;
                }
                curr_node = stack.pop().unwrap();
                curr_height -= 1;
                curr_local_grid =
                    curr_anchor.map(|x| ((x >> ((height - curr_height) * 2)) & 3) as i32);
                curr_anchor = curr_anchor.map(|x| {
                    (x >> ((height - curr_height + 1) * 2)) << ((height - curr_height + 1) * 2)
                });
            } else {
                let child_index = morton::morton_encode(curr_local_grid.map(|x| x as u32));
                let node_size = quarter_sl >> (curr_height * 2);

                match curr_node.deref() {
                    VoxelModelTHCNode::Internal { children } => {
                        if let Some(child) = &children[child_index as usize] {
                            stack.push(curr_node);
                            curr_node = child;
                            curr_height += 1;
                            curr_anchor = curr_anchor
                                .zip_map(&curr_local_grid, |x, y| x + y as u32 * node_size);

                            let global_grid_pos = curr_ray.origin.zip_map(&curr_anchor, |x, y| {
                                (x.floor() as u32).clamp(y, y + node_size - 1)
                            });
                            curr_local_grid = global_grid_pos
                                .map(|x| ((x >> ((height - curr_height) * 2)) & 0b11) as i32);
                            continue;
                        }
                    }
                    VoxelModelTHCNode::Preleaf {
                        leaf_mask,
                        attachment_data,
                    } => {
                        let is_leaf_present = (leaf_mask & (1 << child_index)) > 0;
                        if is_leaf_present {
                            curr_anchor = curr_anchor
                                .zip_map(&curr_local_grid, |x, y| x + y as u32 * node_size);
                            let global_grid_pos = curr_ray.origin.zip_map(&curr_anchor, |x, y| {
                                (x.floor() as u32).clamp(y, y + node_size - 1)
                            });

                            let t_scaling = (aabb.max - aabb.min) * (1.0 / sl as f32);
                            let world_pos_hit =
                                aabb.min + curr_ray.origin.component_mul(&t_scaling);
                            let depth_t = original_pos.metric_distance(&world_pos_hit);
                            return Some(VoxelModelTrace {
                                local_position: global_grid_pos,
                                depth_t,
                            });
                        }
                    }
                }
            }

            let node_size = quarter_sl >> (curr_height * 2);
            let next_point = curr_anchor
                + curr_local_grid.map(|x| x as u32) * node_size
                + unit_grid.map(|x| x.max(0) as u32) * node_size;
            let next_t = curr_ray.intersect_point(next_point.cast::<f32>());
            let min_t = next_t.min();
            let mask = next_t.map(|x| if x == min_t { 1 } else { 0 });

            curr_local_grid += unit_grid.component_mul(&mask);
            curr_ray.advance(min_t + 0.0001);
        }

        return None;
    }

    fn set_voxel_range_impl(&mut self, range: &VoxelModelEdit) {
        self.update_tracker += 1;
        let other = &range.data.flat;
        self.attachment_map.inherit_other(&other.attachment_map);

        for i in 0..other.volume {
            if !other.presence_data.get_bit(i) {
                continue;
            }

            let dst_pos = range.offset + other.get_voxel_position(i);
            let (dst_node, dst_index) = 'get_or_create_preleaf: {
                let height = self.tree_height();
                let mut traversal = morton_traversal_thc(morton_encode(dst_pos), height);
                let mut curr_node = &mut self.root_node;
                for i in 0..height {
                    let index = (traversal & 0b111111) as u32;
                    if i == height - 1 {
                        break 'get_or_create_preleaf (curr_node, index);
                    } else {
                        let mut new_node;
                        match curr_node.deref_mut() {
                            VoxelModelTHCNode::Internal { children } => {
                                new_node = children[index as usize].get_or_insert_with(|| {
                                    if i < height.saturating_sub(2) {
                                        Box::new(VoxelModelTHCNode::new_empty_internal())
                                    } else {
                                        Box::new(VoxelModelTHCNode::new_empty_preleaf())
                                    }
                                });
                            }
                            VoxelModelTHCNode::Preleaf { .. } => unreachable!(),
                        }
                        curr_node = new_node;
                        traversal >>= 6;
                    }
                }

                panic!();
            };

            let mut count = 0u32;
            for (attachment_id, presence_data) in other.attachment_presence_data.iter() {
                if presence_data.get_bit(i) {
                    count += 1;
                    let attachment = self.attachment_map.get_unchecked(attachment_id);
                    let src_offset = i * attachment.size() as usize;
                    let src_data = &other.attachment_data.get(attachment_id).unwrap()
                        [src_offset..(src_offset + attachment.size() as usize)];
                    dst_node.set_attachment(
                        dst_index,
                        attachment_id,
                        attachment.size() as usize,
                        src_data,
                    );
                }
            }

            if count == 0 {
                // Optimize voxel removal to be per node for faster removals.
                match dst_node.deref_mut() {
                    VoxelModelTHCNode::Internal { children } => unreachable!(),
                    VoxelModelTHCNode::Preleaf {
                        leaf_mask,
                        attachment_data,
                    } => {
                        let dst_bit = 1 << dst_index;
                        *leaf_mask &= !dst_bit;
                        for (attachment_mask, data) in attachment_data.values_mut() {
                            if (*attachment_mask & dst_bit) > 0 {
                                let data_offset = (*attachment_mask & (dst_bit - 1)).count_ones();
                                *attachment_mask &= !dst_bit;
                                data.remove(data_offset as usize);
                            }
                        }
                    }
                }
            } else {
                match dst_node.deref_mut() {
                    VoxelModelTHCNode::Internal { children } => unreachable!(),
                    VoxelModelTHCNode::Preleaf {
                        leaf_mask,
                        attachment_data,
                    } => {
                        *leaf_mask |= (1 << dst_index);
                    }
                }
            }
        }
    }

    fn schema(&self) -> super::voxel::VoxelModelSchema {
        consts::voxel::MODEL_THC_SCHEMA
    }

    fn length(&self) -> Vector3<u32> {
        Vector3::new(self.side_length, self.side_length, self.side_length)
    }
}

impl VoxelModelImplConcrete for VoxelModelTHC {
    type Gpu = VoxelModelTHCGpu;

    fn model_type() -> Option<VoxelModelType> {
        Some(VoxelModelType::THC)
    }
}

pub struct VoxelModelTHCGpu {
    compressed_model: Option<VoxelModelTHCCompressed>,
    compressed_model_gpu: VoxelModelTHCCompressedGpu,

    initialized_data: bool,
    update_tracker: u32,
}

impl VoxelModelGpuImplConcrete for VoxelModelTHCGpu {
    fn new() -> Self {
        Self {
            compressed_model: None,
            compressed_model_gpu: VoxelModelTHCCompressedGpu::new(),

            initialized_data: false,
            update_tracker: 0,
        }
    }
}

impl VoxelModelGpuImpl for VoxelModelTHCGpu {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        self.compressed_model_gpu.aggregate_model_info()
    }

    fn update_gpu_objects(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool {
        let model = model.downcast_ref::<VoxelModelTHC>().unwrap();

        let mut did_allocate = false;
        if self.update_tracker != model.update_tracker || !self.initialized_data {
            self.initialized_data = true;
            self.update_tracker = model.update_tracker;
            let compressed_model = VoxelModelTHCCompressed::from(model);
            if self.compressed_model.is_some() {
                self.compressed_model_gpu.dealloc(allocator);
            }

            self.compressed_model = Some(compressed_model);
            self.compressed_model_gpu = VoxelModelTHCCompressedGpu::new();
        }

        if let Some(compressed_model) = &self.compressed_model {
            did_allocate =
                self.compressed_model_gpu
                    .update_gpu_objects(device, allocator, compressed_model);
        }

        return did_allocate;
    }

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        if let Some(compressed_model) = &self.compressed_model {
            self.compressed_model_gpu
                .write_gpu_updates(device, allocator, compressed_model);
        };
    }
}

// Tetrahexacontree, aka., 64-tree. Essentially an octree where each node is
// two octree nodes squashed together, resulting in 64 children in each node.
#[derive(Clone)]
pub struct VoxelModelTHCCompressed {
    pub side_length: u32,
    pub node_data: Vec<THCNodeCompressed>,
    pub attachment_lookup_data: AttachmentMap<Vec<THCAttachmentLookupNodeCompressed>>,
    pub attachment_raw_data: AttachmentMap<Vec<u32>>,
    pub attachment_map: AttachmentInfoMap,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct THCNodeCompressed {
    // Left most bit determines if this node is a leaf.
    pub child_ptr: u32,
    pub child_mask: u64,
}

impl std::fmt::Debug for THCNodeCompressed {
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

impl THCNodeCompressed {
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
pub struct THCAttachmentLookupNodeCompressed {
    pub data_ptr: u32,
    // A mask designating which children have the attachment.
    pub attachment_mask: u64,
}

impl THCAttachmentLookupNodeCompressed {
    pub const fn new_empty() -> Self {
        Self {
            data_ptr: 0,
            attachment_mask: 0,
        }
    }

    pub fn data_ptr(&self) -> u32 {
        self.data_ptr
    }

    pub fn has_child(&self, child_index: u32) -> bool {
        (self.attachment_mask & (1 << child_index)) > 0
    }
}

pub fn next_power_of_4(x: u32) -> u32 {
    let x = x.next_power_of_two();
    if (x.trailing_zeros() % 2 == 0) {
        return x;
    }
    return x << 1;
}

impl VoxelModelTHCCompressed {
    pub fn new_empty(length: u32) -> Self {
        assert_eq!(
            Self::next_power_of_4(length),
            length,
            "Length for a THC must be a power of 4."
        );
        assert!(length >= 4, "Length for a THC must be atleast 4.");
        Self {
            side_length: length,
            node_data: vec![THCNodeCompressed::new_empty()],
            attachment_lookup_data: AttachmentMap::new(),
            attachment_raw_data: AttachmentMap::new(),
            attachment_map: AttachmentMap::new(),
        }
    }

    pub fn in_bounds_local(&self, local_position: Vector3<i32>) -> bool {
        return local_position.x >= 0
            && local_position.y >= 0
            && local_position.z >= 0
            && local_position.x < self.side_length as i32
            && local_position.y < self.side_length as i32
            && local_position.z < self.side_length as i32;
    }

    pub fn next_power_of_4(x: u32) -> u32 {
        let x = x.next_power_of_two();
        if (x.trailing_zeros() % 2 == 0) {
            return x;
        }
        return x << 1;
    }

    // If not existing already, will intialize the attachment buffers and register to the
    // attachment map.
    pub fn initialize_attachment_buffers(&mut self, attachment: &Attachment) {
        self.attachment_map
            .insert(attachment.id(), attachment.clone());

        if !self.attachment_lookup_data.contains(attachment.id()) {
            self.attachment_lookup_data.insert(
                attachment.id(),
                vec![THCAttachmentLookupNodeCompressed::new_empty(); self.node_data.len()],
            );
            self.attachment_raw_data.insert(attachment.id(), Vec::new());
        }
    }

    pub fn tree_height(&self) -> u32 {
        self.side_length.trailing_zeros() / 2
    }
}

impl VoxelModelImplConcrete for VoxelModelTHCCompressed {
    type Gpu = VoxelModelTHCCompressedGpu;

    fn model_type() -> Option<VoxelModelType> {
        Some(VoxelModelType::THCCompressed)
    }
}

impl VoxelModelImpl for VoxelModelTHCCompressed {
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace> {
        let mut ray = ray.clone();
        let Some(model_t) = ray.intersect_aabb(aabb) else {
            return None;
        };
        ray.advance(model_t);

        let local_pos = ray.origin - aabb.min;
        let norm_pos = local_pos.zip_map(&aabb.side_length(), |x, y| (x / y).clamp(0.0, 0.9999));
        // Our scaled position from [0, bounds).
        let dda_pos = norm_pos * self.side_length as f32;

        let height = self.tree_height() - 1;
        let sl = self.side_length;
        let quarter_sl = self.side_length >> 2;
        let unit_grid = ray.dir.map(|x| x.signum() as i32);

        let mut curr_ray = Ray::new(dda_pos, ray.dir);
        let mut curr_node_index = 0;
        let mut curr_height = 0;
        let mut curr_local_grid = curr_ray
            .origin
            .map(|x| (x.floor() as u32 >> (height * 2)) as i32);
        let mut curr_anchor = Vector3::<u32>::zeros();
        // Don't include the leaf layer in the height.
        let mut stack = Vec::new();
        let mut i = 0;
        while self.in_bounds_local(curr_ray.origin.map(|x| x.floor() as i32)) && (i < 2000) {
            i += 1;
            let should_pop = !(curr_local_grid.x >= 0
                && curr_local_grid.y >= 0
                && curr_local_grid.z >= 0
                && curr_local_grid.x <= 3
                && curr_local_grid.y <= 3
                && curr_local_grid.z <= 3);
            if should_pop {
                if curr_height == 0 {
                    break;
                }
                curr_node_index = stack.pop().unwrap();
                curr_height -= 1;
                curr_local_grid =
                    curr_anchor.map(|x| ((x >> ((height - curr_height) * 2)) & 3) as i32);
                curr_anchor = curr_anchor.map(|x| {
                    (x >> ((height - curr_height + 1) * 2)) << ((height - curr_height + 1) * 2)
                });
            } else {
                let child_index = morton::morton_encode(curr_local_grid.map(|x| x as u32));
                let curr_node = &self.node_data[curr_node_index];
                let is_present = (curr_node.child_mask & (1 << child_index)) > 0;
                if is_present {
                    let node_size = quarter_sl >> (curr_height * 2);
                    curr_anchor =
                        curr_anchor.zip_map(&curr_local_grid, |x, y| x + y as u32 * node_size);
                    let global_grid_pos = curr_ray.origin.zip_map(&curr_anchor, |x, y| {
                        (x.floor() as u32).clamp(y, y + node_size - 1)
                    });

                    if curr_node.is_leaf_node() {
                        let t_scaling = (aabb.max - aabb.min) * (1.0 / sl as f32);
                        let world_pos_hit = aabb.min + curr_ray.origin.component_mul(&t_scaling);
                        let depth_t = ray.origin.metric_distance(&world_pos_hit);
                        return Some(VoxelModelTrace {
                            local_position: global_grid_pos,
                            depth_t,
                        });
                    }
                    let child_offset =
                        (curr_node.child_mask & ((1 << child_index) - 1)).count_ones();
                    stack.push(curr_node_index);
                    curr_node_index = (curr_node.child_ptr() + child_offset) as usize;

                    curr_height += 1;
                    curr_local_grid = global_grid_pos
                        .map(|x| ((x >> ((height - curr_height) * 2)) & 0b11) as i32);
                    continue;
                }
            }

            let node_size = quarter_sl >> (curr_height * 2);
            let next_point = curr_anchor
                + curr_local_grid.map(|x| x as u32) * node_size
                + unit_grid.map(|x| x.max(0) as u32);
            let next_t = curr_ray.intersect_point(next_point.cast::<f32>());
            let min_t = next_t.min();
            let mask = next_t.map(|x| if x == min_t { 1 } else { 0 });

            curr_local_grid += unit_grid.component_mul(&mask);
            // Epsilon since sometimes we advance out of bounds but due to fp math it's just barely
            // off, messing up the traversal.
            curr_ray.advance(min_t + 0.0001);
        }

        return None;
    }

    fn set_voxel_range_impl(&mut self, range: &VoxelModelEdit) {}

    fn schema(&self) -> super::voxel::VoxelModelSchema {
        consts::voxel::MODEL_THC_COMPRESSED_SCHEMA
    }

    fn length(&self) -> nalgebra::Vector3<u32> {
        Vector3::new(self.side_length, self.side_length, self.side_length)
    }
}

pub struct VoxelModelTHCCompressedGpu {
    // Model side length in voxels.
    side_length: u32,
    nodes_allocation: Option<VoxelDataAllocation>,
    attachment_lookup_allocations: HashMap<AttachmentId, VoxelDataAllocation>,
    attachment_raw_allocations: HashMap<AttachmentId, VoxelDataAllocation>,

    initialized_model_data: bool,
}

impl VoxelModelGpuImplConcrete for VoxelModelTHCCompressedGpu {
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

impl VoxelModelTHCCompressedGpu {
    pub fn dealloc(&mut self, allocator: &mut VoxelDataAllocator) {
        if let Some(nodes_alloc) = self.nodes_allocation.take() {
            allocator.free(&nodes_alloc);
        }
        for (_, alloc) in self.attachment_lookup_allocations.drain() {
            allocator.free(&alloc);
        }
        for (_, alloc) in self.attachment_raw_allocations.drain() {
            allocator.free(&alloc);
        }
    }
}

impl VoxelModelGpuImpl for VoxelModelTHCCompressedGpu {
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

            attachment_lookup_indices[*attachment as usize] = lookup_allocation.ptr_gpu();
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
            attachment_raw_indices[*attachment as usize] = raw_allocation.ptr_gpu();
        }

        let mut info = vec![
            self.side_length,
            // Node ptr (divide by 4 since 4 bytes in a u32)
            data_allocation.ptr_gpu(),
        ];
        info.append(&mut attachment_lookup_indices);
        info.append(&mut attachment_raw_indices);

        Some(info)
    }

    fn update_gpu_objects(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool {
        let model = model.downcast_ref::<VoxelModelTHCCompressed>().unwrap();
        let mut did_allocate = false;

        if self.nodes_allocation.is_none() {
            let nodes_allocation_size = model.node_data.len() as u64 * 12;
            self.nodes_allocation = Some(
                allocator
                    .allocate(device, nodes_allocation_size)
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
                        .allocate(device, lookup_data_allocation_size)
                        .expect("Failed to allocate THC attachment lookup data."),
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
                        .allocate(device, raw_data_allocation_size)
                        .expect("Failed to allocate THC attachment raw data."),
                );
                did_allocate = true;
            }
        }

        // Add implicit normal attachment.
        if !model.attachment_raw_data.contains(Attachment::NORMAL_ID)
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
                            .reallocate(device, e.get(), req_data_allocation_size)
                            .expect("Failed to reallocate thc attachment raw data.");

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
                            .allocate(device, req_data_allocation_size as u64)
                            .expect("Failed to allocate thc attachment raw data."),
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
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        let model = model.downcast_ref::<VoxelModelTHCCompressed>().unwrap();

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
        VoxelModelTHC::from(&VoxelModelTHCCompressed::from(flat))
    }
}

impl From<VoxelModelFlat> for VoxelModelTHCCompressed {
    fn from(value: VoxelModelFlat) -> Self {
        From::<&VoxelModelFlat>::from(&value)
    }
}

impl From<&VoxelModelFlat> for VoxelModelTHCCompressed {
    fn from(flat: &VoxelModelFlat) -> Self {
        let length = flat
            .side_length()
            .map(|x| VoxelModelTHCCompressed::next_power_of_4(x))
            .max()
            .max(4);
        let volume = (length as u64).pow(3);
        // With just the root node being a height of 1, since log4(4) == log2(4) / 2 == 1.
        let height = length.trailing_zeros() / 2;

        let mut levels: Vec<Vec<Option<THCNodeCompressed>>> =
            (0..=height).map(|_| Vec::new()).collect::<Vec<_>>();
        let mut node_list_rev: Vec<THCNodeCompressed> = Vec::new();
        for i in 0..volume {
            let pos = morton_decode(i);
            if !flat.in_bounds(pos) || !flat.get_voxel(pos).exists() {
                levels[height as usize].push(None);
            } else {
                levels[height as usize].push(Some(THCNodeCompressed::new_empty()));
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
                    levels[h as usize - 1].push(Some(THCNodeCompressed {
                        child_ptr,
                        child_mask,
                    }));
                } else {
                    levels[h as usize - 1].push(None);
                }
            }
        }
        let root_node = levels[0][0]
            .clone()
            .unwrap_or(THCNodeCompressed::new_empty());
        if root_node.child_mask == 0 {
            return VoxelModelTHCCompressed::new_empty(length);
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
                    THCAttachmentLookupNodeCompressed {
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
                    THCAttachmentLookupNodeCompressed {
                        data_ptr: *raw_ptr,
                        attachment_mask: *attachment_mask,
                    };
            }
        }

        VoxelModelTHCCompressed {
            side_length: length,
            node_data,
            attachment_lookup_data,
            attachment_raw_data,
            attachment_map: flat.attachment_map.clone(),
        }
    }
}
