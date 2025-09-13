use std::{collections::HashMap, ops::Deref};

use nalgebra::Vector3;

use crate::{
    common::morton,
    consts,
    engine::voxel::{
        attachment::{AttachmentMap, BuiltInMaterial},
        voxel::{VoxelModelSchema, VoxelModelTrace},
    },
};
use crate::common::geometry::ray::Ray;
use super::{
    attachment::{Attachment, AttachmentId, AttachmentInfoMap},
    flat::VoxelModelFlat,
    sft::VoxelModelSFT,
    sft_compressed_gpu::VoxelModelSFTCompressedGpu,
    sft_gpu::VoxelModelSFTGpu,
    thc::VoxelModelTHCCompressed,
    voxel::{VoxelModelImpl, VoxelModelImplConcrete, VoxelModelType},
};

#[derive(Clone)]
pub struct SFTNodeCompressed {
    // Left most bit determines if this node is a leaf.
    pub child_ptr: u32,
    pub child_mask: u64,
    pub leaf_mask: u64,
}

impl SFTNodeCompressed {
    pub const U32_SIZE: u64 = 5;
    pub const BYTE_SIZE: u64 = Self::U32_SIZE * 4;

    pub fn new_empty() -> Self {
        Self {
            child_ptr: 0,
            leaf_mask: 0,
            child_mask: 0,
        }
    }

    pub fn has_leaf(&self, child_index: u32) -> bool {
        (self.leaf_mask & (1 << child_index)) > 0
    }

    pub fn has_child(&self, child_index: u32) -> bool {
        (self.child_mask & (1 << child_index)) > 0
    }
}

#[derive(Clone, Debug)]
pub struct SFTAttachmentLookupNodeCompressed {
    pub data_ptr: u32,
    // A mask designating which children have the attachment.
    pub attachment_mask: u64,
}

impl SFTAttachmentLookupNodeCompressed {
    pub const U32_SIZE: u64 = 3;
    pub const BYTE_SIZE: u64 = Self::U32_SIZE * 4;

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

#[derive(Clone)]
pub struct VoxelModelSFTCompressed {
    pub side_length: u32,
    pub attachment_map: AttachmentInfoMap,

    pub node_data: Vec<SFTNodeCompressed>,
    pub attachment_lookup_data: AttachmentMap<Vec<SFTAttachmentLookupNodeCompressed>>,
    pub attachment_raw_data: AttachmentMap<Vec<u32>>,
}

impl VoxelModelSFTCompressed {
    pub fn new_empty(length: u32) -> Self {
        assert_eq!(
            Self::next_power_of_4(length),
            length,
            "Length for a THC must be a power of 4."
        );
        assert!(length >= 4, "Length for a THC must be atleast 4.");
        Self {
            side_length: length,
            node_data: vec![SFTNodeCompressed::new_empty()],
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

    pub fn collect_attachment_data(
        &self,
        node_index: usize,
    ) -> Box<
        AttachmentMap<(
            /*attachment_mask=*/ u64,
            /*attachment_data=*/ Vec<u32>,
        )>,
    > {
        let mut result = Box::new(AttachmentMap::new());
        for (attachment_id, lookup_data) in self.attachment_lookup_data.iter() {
            if lookup_data.len() <= node_index {
                continue;
            }

            let lookup_node = &lookup_data[node_index];
            if lookup_node.attachment_mask == 0 {
                continue;
            }

            let attachment_size = self.attachment_map.get_unchecked(attachment_id).size() as usize;
            let data_ptr = lookup_node.data_ptr() as usize;
            let node_attachment_size =
                lookup_node.attachment_mask.count_ones() as usize * attachment_size;
            let raw_attachment_data = &self.attachment_raw_data.get(attachment_id).unwrap()
                [data_ptr..(data_ptr + node_attachment_size)];
            result.insert(
                attachment_id,
                (lookup_node.attachment_mask, raw_attachment_data.to_vec()),
            );
        }

        return result;
    }

    // If not existing already, will intialize the attachment buffers and register to the
    // attachment map.
    pub fn initialize_attachment_buffers(&mut self, attachment: &Attachment) {
        self.attachment_map
            .insert(attachment.id(), attachment.clone());

        if !self.attachment_lookup_data.contains(attachment.id()) {
            self.attachment_lookup_data.insert(
                attachment.id(),
                vec![SFTAttachmentLookupNodeCompressed::new_empty(); self.node_data.len()],
            );
            self.attachment_raw_data.insert(attachment.id(), Vec::new());
        }
    }

    pub fn tree_height(&self) -> u32 {
        self.side_length.trailing_zeros() / 2
    }
}

impl VoxelModelImplConcrete for VoxelModelSFTCompressed {
    type Gpu = VoxelModelSFTCompressedGpu;

    fn model_type() -> Option<VoxelModelType> {
        Some(VoxelModelType::SFTCompressed)
    }
}

impl VoxelModelImpl for VoxelModelSFTCompressed {
    fn trace(
        &self,
        ray: &crate::common::geometry::ray::Ray,
        aabb: &crate::common::geometry::aabb::AABB,
    ) -> Option<super::voxel::VoxelModelTrace> {
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

                    let is_leaf = (curr_node.leaf_mask & (1 << child_index)) > 0;
                    if is_leaf {
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
                    curr_node_index = (curr_node.child_ptr + child_offset) as usize;

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

    fn set_voxel_range_impl(&mut self, range: &super::voxel::VoxelModelEdit) {
        unimplemented!("Use VoxelModelSFT for editing.");
    }

    fn schema(&self) -> super::voxel::VoxelModelSchema {
        return consts::voxel::MODEL_SFT_COMPRESSED_SCHEMA;
    }

    fn length(&self) -> Vector3<u32> {
        return Vector3::new(self.side_length, self.side_length, self.side_length);
    }
}

impl From<&VoxelModelSFT> for VoxelModelSFTCompressed {
    fn from(sft: &VoxelModelSFT) -> Self {
        let mut compressed = VoxelModelSFTCompressed::new_empty(sft.side_length);
        for (_, attachment) in sft.attachment_map.iter() {
            compressed.initialize_attachment_buffers(attachment);
        }

        let mut stack = vec![(
            &sft.root_node,
            /*curr_compressed_node*/ 0,
            /*curr_child_iter*/ 0,
        )];
        compressed.node_data.push(SFTNodeCompressed::new_empty());
        while !stack.is_empty() {
            let (curr_node, compressed_node_idx, curr_child_iter) = stack.last_mut().unwrap();
            // Allocate space for this nodes children, and initialize
            // this nodes info along with the attachment data for this node's leaves.
            if *curr_child_iter == 0 {
                let children_allocation_index = compressed.node_data.len();
                // This is already allocated since the root node always exist and we allocate
                // room any children with the resize below.
                compressed.node_data[*compressed_node_idx as usize] = SFTNodeCompressed {
                    child_ptr: children_allocation_index as u32,
                    child_mask: curr_node.child_mask,
                    leaf_mask: curr_node.leaf_mask,
                };
                compressed.node_data.resize(
                    children_allocation_index + curr_node.children.len(),
                    SFTNodeCompressed::new_empty(),
                );

                // Set attachment info for the current node.
                if let Some(attachment_data) = &curr_node.attachment_data {
                    for (attachment_id, (attachment_mask, attachment_data)) in
                        attachment_data.iter()
                    {
                        if attachment_mask.count_ones() as usize != attachment_data.len() {
                            assert_eq!(
                                attachment_mask.count_ones() as usize,
                                attachment_data.len()
                            );
                        }
                        let compressed_raw_data = compressed
                            .attachment_raw_data
                            .get_mut(attachment_id)
                            .unwrap();
                        let raw_data_ptr = compressed_raw_data.len();
                        compressed_raw_data.extend_from_slice(&attachment_data);

                        let compressed_lookup_data = compressed
                            .attachment_lookup_data
                            .get_mut(attachment_id)
                            .unwrap();
                        compressed_lookup_data.resize(
                            (*compressed_node_idx as usize + 1).max(compressed_lookup_data.len()),
                            SFTAttachmentLookupNodeCompressed::new_empty(),
                        );
                        compressed_lookup_data[*compressed_node_idx] =
                            SFTAttachmentLookupNodeCompressed {
                                data_ptr: raw_data_ptr as u32,
                                attachment_mask: *attachment_mask,
                            };
                    }
                }
            }

            if *curr_child_iter == curr_node.children.len() {
                stack.pop();
                continue;
            }

            let curr_compressed_node = &compressed.node_data[*compressed_node_idx];
            let compressed_child_ptr = curr_compressed_node.child_ptr as usize + *curr_child_iter;
            let next_child_node_ptr = &curr_node.children[*curr_child_iter];
            *curr_child_iter += 1;
            stack.push((next_child_node_ptr, compressed_child_ptr, 0));
        }

        for (attachment_id, lookup_data) in compressed.attachment_lookup_data.iter_mut() {
            lookup_data.resize(
                compressed.node_data.len(),
                SFTAttachmentLookupNodeCompressed::new_empty(),
            );
        }

        return compressed;
    }
}

impl From<&VoxelModelTHCCompressed> for VoxelModelSFTCompressed {
    fn from(thc: &VoxelModelTHCCompressed) -> Self {
        let mut node_data = thc
            .node_data
            .iter()
            .map(|node| {
                let leaf_mask = if node.is_leaf_node() {
                    node.child_mask
                } else {
                    0
                };

                SFTNodeCompressed {
                    child_ptr: node.child_ptr(),
                    child_mask: node.child_mask,
                    leaf_mask,
                }
            })
            .collect::<Vec<_>>();

        let mut attachment_lookup_data = AttachmentMap::new();
        for (attachment_id, thc_lookup_data) in thc.attachment_lookup_data.iter() {
            let sft_lookup_data = thc_lookup_data
                .iter()
                .map(|node| SFTAttachmentLookupNodeCompressed {
                    data_ptr: node.data_ptr(),
                    attachment_mask: node.attachment_mask,
                })
                .collect();
            attachment_lookup_data.insert(attachment_id, sft_lookup_data);
        }
        let attachment_raw_data = thc.attachment_raw_data.clone();

        return VoxelModelSFTCompressed {
            side_length: thc.side_length,
            attachment_map: thc.attachment_map.clone(),

            node_data,
            attachment_lookup_data,
            attachment_raw_data,
        };
    }
}

#[derive(Clone)]
enum SFTFlatNode {
    Empty,
    Leaf(
        (
            /*voxel_morton*/ u64,
            /*builtin_material_index*/ u32,
        ),
    ),
    Child(
        (
            SFTNodeCompressed,
            AttachmentMap<SFTAttachmentLookupNodeCompressed>,
        ),
    ),
}

impl SFTFlatNode {
    pub const NULL_MATERIAL_INDEX: u32 = u32::MAX;

    pub fn is_empty(&self) -> bool {
        matches!(self, SFTFlatNode::Empty)
    }

    pub fn is_present(&self) -> bool {
        matches!(self, SFTFlatNode::Leaf(_))
    }
}

impl From<&VoxelModelFlat> for VoxelModelSFTCompressed {
    fn from(flat: &VoxelModelFlat) -> Self {
        let length = flat
            .side_length()
            .map(|x| VoxelModelSFTCompressed::next_power_of_4(x))
            .max()
            .max(4);
        let volume = (length as u64).pow(3);
        // With just the root node being a height of 1, since log4(4) == log2(4) / 2 == 1.
        let height = length.trailing_zeros() / 2;

        let mut levels: Vec<Vec<SFTFlatNode>> =
            (0..=height).map(|_| Vec::new()).collect::<Vec<_>>();

        let attachment_info_map = flat.attachment_map.clone();

        let mut node_list_rev: Vec<SFTNodeCompressed> = Vec::new();
        let mut attachment_lookup_data: AttachmentMap<Vec<SFTAttachmentLookupNodeCompressed>> =
            AttachmentMap::new();
        for (attachment_id, _) in flat.attachment_map.iter() {
            attachment_lookup_data.insert(attachment_id, Vec::new());
        }
        let mut attachment_raw_data: AttachmentMap<Vec<u32>> = AttachmentMap::new();
        for (attachment_id, _) in flat.attachment_map.iter() {
            attachment_raw_data.insert(attachment_id, Vec::new());
        }

        for i in 0..volume {
            let pos = morton::morton_decode(i);
            if !flat.in_bounds(pos) || !flat.get_voxel(pos).exists() {
                levels[height as usize].push(SFTFlatNode::Empty);
            } else {
                let builtin_material_index = flat
                    .get_voxel(pos)
                    .get_attachment_data()
                    .find(|(id, data)| *id == Attachment::BMAT_ID)
                    .map(|(id, data)| data[0])
                    .unwrap_or(SFTFlatNode::NULL_MATERIAL_INDEX);
                levels[height as usize].push(SFTFlatNode::Leaf((i, builtin_material_index)));
            }

            for h in (1..=height).rev() {
                let curr_level = &mut levels[h as usize];
                if curr_level.len() != 64 {
                    break;
                }

                let mut first_bt_index = None;
                let mut homogenous = true;
                let mut child_mask = 0;
                let mut leaf_mask = 0;
                let mut child_ptr = u32::MAX;
                let mut new_attachment_map: AttachmentMap<SFTAttachmentLookupNodeCompressed> =
                    AttachmentMap::new();
                for (attachment_id, _) in flat.attachment_map.iter() {
                    new_attachment_map.insert(
                        attachment_id,
                        SFTAttachmentLookupNodeCompressed::new_empty(),
                    );
                }
                // TODO: Don't drain so we don't have to reverse the raw attachment vec as well.
                for (child_index, node) in curr_level.drain(..).enumerate().rev() {
                    match node {
                        SFTFlatNode::Empty => {
                            // We check if the node is all air down below via the child_mask.
                            homogenous = false;
                        }
                        SFTFlatNode::Leaf((morton, id)) => {
                            let child_bit = 1 << child_index;
                            child_mask |= child_bit;
                            leaf_mask |= child_bit;
                            if homogenous {
                                if id != SFTFlatNode::NULL_MATERIAL_INDEX {
                                    let first_id = first_bt_index.get_or_insert(id);
                                    if *first_id != id {
                                        homogenous = false;
                                    }
                                } else {
                                    homogenous = false;
                                }
                            }

                            if h == height {
                                for (attachment_id, data) in flat
                                    .get_voxel(morton::morton_decode(morton))
                                    .get_attachment_data()
                                {
                                    let dst_raw_data =
                                        attachment_raw_data.get_mut(attachment_id).unwrap();
                                    let attachment_ptr = dst_raw_data.len();
                                    dst_raw_data.extend(data.iter().rev());

                                    let mut curr_lookup_node =
                                    new_attachment_map.get_mut(
                                        attachment_id).expect("Flat voxel attachment is not present in the attachment info map.");
                                    curr_lookup_node.attachment_mask |= child_bit;
                                    curr_lookup_node.data_ptr = attachment_ptr as u32;
                                }
                            } else {
                                assert!(id != SFTFlatNode::NULL_MATERIAL_INDEX);
                                let dst_raw_data =
                                    attachment_raw_data.get_mut(Attachment::BMAT_ID).unwrap();
                                let attachment_ptr = dst_raw_data.len() as u32;
                                dst_raw_data.push(BuiltInMaterial::new(id as u16).encode());

                                let mut curr_lookup_node =
                                new_attachment_map.get_mut(
                                    Attachment::BMAT_ID).expect("Flat voxel builtin attachment is not present in the attachment info map when it should be.");
                                curr_lookup_node.attachment_mask |= child_bit;
                                curr_lookup_node.data_ptr = attachment_ptr;
                            }
                        }
                        SFTFlatNode::Child((child_node, child_lookup_node)) => {
                            homogenous = false;
                            let child_bit = 1 << child_index;
                            child_mask |= child_bit;
                            child_ptr = node_list_rev.len() as u32;
                            node_list_rev.push(child_node);
                            for (attachment_id, lookup_node) in child_lookup_node.into_iter() {
                                attachment_lookup_data
                                    .get_mut(attachment_id)
                                    .unwrap()
                                    .push(lookup_node);
                            }
                        }
                    }
                }

                if child_mask == 0 {
                    // All children are empty, so this node is also empty.
                    levels[(h - 1) as usize].push(SFTFlatNode::Empty);
                } else if first_bt_index.is_some() && homogenous {
                    // All children are the same builtin material, so this node is a leaf.
                    levels[(h - 1) as usize].push(SFTFlatNode::Leaf((0, first_bt_index.unwrap())));
                } else {
                    // Node is heterogenous with air and solid voxels so create an internal child.
                    let new_node = SFTNodeCompressed {
                        child_ptr,
                        child_mask,
                        leaf_mask,
                    };

                    levels[(h - 1) as usize]
                        .push(SFTFlatNode::Child((new_node, new_attachment_map)));
                }
            }
        }

        let (root_node, root_attachment_lookup) = match levels[0].pop().unwrap() {
            SFTFlatNode::Empty => {
                return VoxelModelSFTCompressed::new_empty(length);
            }
            SFTFlatNode::Leaf((morton, builtin_index)) => {
                let mut attachment_lookup_data = AttachmentMap::new();
                attachment_lookup_data.insert(
                    Attachment::BMAT_ID,
                    vec![SFTAttachmentLookupNodeCompressed {
                        data_ptr: 0,
                        attachment_mask: u64::MAX,
                    }],
                );
                let mut attachment_raw_data = AttachmentMap::new();
                attachment_raw_data.insert(
                    Attachment::BMAT_ID,
                    vec![BuiltInMaterial::new(builtin_index as u16).encode(); 64],
                );
                return VoxelModelSFTCompressed {
                    side_length: length,
                    attachment_map: attachment_info_map,
                    node_data: vec![SFTNodeCompressed {
                        child_ptr: u32::MAX,
                        child_mask: u64::MAX,
                        leaf_mask: u64::MAX,
                    }],
                    attachment_lookup_data,
                    attachment_raw_data,
                };
            }
            SFTFlatNode::Child(nodes) => nodes,
        };

        // Reverse the lists since we want the root node to be first.
        // TODO: Figure out if we actually want to support editing, if we don't, then we don't
        // really gotta do all of this.
        node_list_rev.push(root_node);
        for (attachment_id, lookup_node) in root_attachment_lookup.iter() {
            attachment_lookup_data
                .get_mut(attachment_id)
                .unwrap()
                .push(lookup_node.clone());
        }

        let node_data_len = node_list_rev.len() as u32;
        assert!(node_data_len < 0x8000_0000);
        let mut node_data = node_list_rev
            .into_iter()
            .map(|mut node| {
                if node.child_ptr == u32::MAX {
                    return node;
                }
                node.child_ptr = node_data_len - 1 - node.child_ptr;
                return node;
            })
            .collect::<Vec<_>>();
        node_data.reverse();

        for (attachment_id, lookup_data) in attachment_lookup_data.iter_mut() {
            let raw_data = attachment_raw_data.get_mut(attachment_id).unwrap();
            let raw_data_len = raw_data.len() as u32;
            for lookup_node in lookup_data.iter_mut() {
                if raw_data_len == 0 {
                    assert!(raw_data_len == 1);
                }
                lookup_node.data_ptr = raw_data_len - 1 - lookup_node.data_ptr;
            }

            lookup_data.reverse();
            raw_data.reverse();
        }

        log::info!("Finished SFT compressed");
        return VoxelModelSFTCompressed {
            side_length: length,
            attachment_map: attachment_info_map,
            node_data,
            attachment_lookup_data,
            attachment_raw_data,
        };
    }
}
