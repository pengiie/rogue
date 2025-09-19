use std::{collections::HashMap, u64};

use nalgebra::Vector3;

use super::{
    attachment::{Attachment, AttachmentId, AttachmentInfoMap, AttachmentMap},
    flat::VoxelModelFlat,
    sft_compressed::VoxelModelSFTCompressed,
    sft_gpu::VoxelModelSFTGpu,
    thc::VoxelModelTHCCompressed,
    voxel::{
        VoxelModelEdit, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
        VoxelModelImplConcrete, VoxelModelSchema, VoxelModelTrace,
    },
    voxel_allocator::{VoxelDataAllocation, VoxelDataAllocator},
};
use crate::common::geometry::ray::Ray;
use crate::{common::geometry::aabb::AABB, engine::voxel::thc::VoxelModelTHC};
use crate::{
    common::morton::{self, morton_encode, morton_traversal_thc, next_power_of_4},
    consts,
    engine::{graphics::device::GfxDevice, voxel::attachment::BuiltInMaterial},
};

#[derive(Clone)]
pub struct VoxelModelSFTNode {
    pub child_mask: u64,
    pub leaf_mask: u64,
    // Any internal children nodes.
    pub children: Vec<Box<VoxelModelSFTNode>>,
    // AttachmentMap doesn't lazily allcoate so handle that here.
    pub attachment_data: Option<
        Box<
            AttachmentMap<(
                /*attachment_mask*/ u64,
                /*attachment_data*/ Vec<u32>,
            )>,
        >,
    >,
}

impl VoxelModelSFTNode {
    pub fn new_empty() -> Self {
        Self {
            child_mask: 0,
            leaf_mask: 0,
            children: Vec::new(),
            attachment_data: None,
        }
    }

    pub fn new_filled(bmat_compressed: u32) -> Self {
        let mut attachment_data = AttachmentMap::new();
        attachment_data.insert(Attachment::BMAT_ID, (u64::MAX, vec![bmat_compressed; 64]));
        Self {
            child_mask: u64::MAX,
            leaf_mask: u64::MAX,
            children: Vec::new(),
            attachment_data: Some(Box::new(attachment_data)),
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

        let attachment_data = self
            .attachment_data
            .get_or_insert_with(|| Box::new(AttachmentMap::new()));

        let Some((attachment_mask, attachment_data)) = attachment_data.get_mut(attachment_id)
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
        } else {
            for i in 0..attachment_size {
                attachment_data.insert(child_offset + i, data[i]);
            }
        }
        *attachment_mask |= child_bit;
    }
}

/// Sixty-four tree that is pointer based and also sparsely allocated. That means both empty
/// space and attachments which can be compacted will be merged up the tree to save memory.
/// Differs from the THC since THC can only merge air, and this can do materials as well.
#[derive(Clone)]
pub struct VoxelModelSFT {
    pub side_length: u32,
    pub attachment_map: AttachmentInfoMap,
    pub root_node: VoxelModelSFTNode,
    pub update_tracker: u32,
}

impl VoxelModelSFT {
    pub fn new_empty(side_length: u32) -> Self {
        return Self::new_empty_with_attachment_map(side_length, AttachmentInfoMap::new());
    }

    pub fn new_empty_with_attachment_map(
        side_length: u32,
        attachment_map: AttachmentInfoMap,
    ) -> Self {
        assert_eq!(
            next_power_of_4(side_length),
            side_length,
            "Length for a SFT must be a power of 4."
        );
        assert!(side_length >= 4, "Length for a SFT must be atleast 4.");

        Self {
            side_length,
            attachment_map,
            root_node: VoxelModelSFTNode::new_empty(),
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

    // Any homogenous nodes become a leaf of the same material. This can make solid chunks
    // take up very little memory. Also does the same for air. Does not account for normals so gg
    // on that, we can approximate normals on those tho by the voxel face since they will be cubes.
    fn recompress_bmat(&mut self) {}

    pub fn tree_height(&self) -> u32 {
        self.side_length.trailing_zeros() / 2
    }
}

impl VoxelModelImplConcrete for VoxelModelSFT {
    type Gpu = VoxelModelSFTGpu;
}

impl VoxelModelImpl for VoxelModelSFT {
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace> {
        let original_pos = ray.origin;
        let mut ray = ray.clone();
        let Some(model_t) = ray.intersect_aabb(aabb) else {
            return None;
        };
        ray.advance(model_t);

        // Setup DDA with ray in the model's voxel-space.
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
                let is_child_present = (curr_node.child_mask & (1 << child_index)) > 0;
                if is_child_present {
                    let is_leaf_present = (curr_node.leaf_mask & (1 << child_index)) > 0;
                    if is_leaf_present {
                        curr_anchor =
                            curr_anchor.zip_map(&curr_local_grid, |x, y| x + y as u32 * node_size);
                        let global_grid_pos = curr_ray.origin.zip_map(&curr_anchor, |x, y| {
                            (x.floor() as u32).clamp(y, y + node_size - 1)
                        });

                        let t_scaling = (aabb.max - aabb.min) * (1.0 / sl as f32);
                        let world_pos_hit = aabb.min + curr_ray.origin.component_mul(&t_scaling);
                        let depth_t = original_pos.metric_distance(&world_pos_hit);
                        return Some(VoxelModelTrace {
                            local_position: global_grid_pos,
                            depth_t,
                        });
                    } else {
                        let child_offset = ((curr_node.child_mask & !curr_node.leaf_mask)
                            & ((1 << child_index) - 1))
                            .count_ones();
                        let Some(child) = curr_node.children.get(child_offset as usize) else {
                            panic!();
                        };
                        stack.push(curr_node);
                        curr_node = child;
                        curr_height += 1;
                        curr_anchor =
                            curr_anchor.zip_map(&curr_local_grid, |x, y| x + y as u32 * node_size);

                        let global_grid_pos = curr_ray.origin.zip_map(&curr_anchor, |x, y| {
                            (x.floor() as u32).clamp(y, y + node_size - 1)
                        });
                        curr_local_grid = global_grid_pos
                            .map(|x| ((x >> ((height - curr_height) * 2)) & 0b11) as i32);
                        continue;
                    }
                }
            }

            // Advance the ray position with dda at the current height.
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
            // Empty voxel.
            if !other.presence_data.get_bit(i) {
                continue;
            }

            let dst_pos = range.offset + other.get_voxel_position(i);
            // We love rust lifetimes, who doesn't want self to take ownership of every variable
            // inside it :), i wanted to indent my code a ton anyways.
            let (dst_node, dst_child_index) = 'get_or_create_preleaf: {
                let height = self.tree_height();
                let mut traversal = morton_traversal_thc(morton_encode(dst_pos), height);

                let mut curr_node = &mut self.root_node;
                for i in 0..height {
                    // The index of the next child to traverse.
                    let child_index = ((traversal >> (i * 6)) & 0b111111) as u32;

                    // Check if we are on the preleaf node.
                    if i == height - 1 {
                        break 'get_or_create_preleaf (curr_node, child_index);
                    } else {
                        let child_bit = 1 << child_index;
                        let exists_as_leaf = (child_bit & curr_node.leaf_mask) > 0;
                        if exists_as_leaf {
                            // Remove leaf attachment and restructure into a child of this node.
                            let (attachment_mask, attachment_data) = curr_node
                                .attachment_data
                                .as_mut()
                                .unwrap()
                                .get_mut(Attachment::BMAT_ID)
                                .unwrap();
                            let data_offset =
                                (*attachment_mask & (child_bit - 1)).count_ones() as usize;
                            let bmat_compressed = attachment_data.remove(data_offset);
                            let mut new_node = VoxelModelSFTNode::new_filled(bmat_compressed);

                            curr_node.leaf_mask &= !child_bit;
                            *attachment_mask &= !child_bit;

                            let non_leaf_child_mask = curr_node.child_mask & !curr_node.leaf_mask;
                            let child_offset =
                                ((child_bit - 1) & non_leaf_child_mask).count_ones() as usize;
                            curr_node.children.insert(child_offset, Box::new(new_node));
                            curr_node = &mut curr_node.children[child_offset];
                            continue;
                        }

                        let exists_as_internal = (child_bit & curr_node.child_mask) > 0;
                        // Offset into the children array where this child resides.
                        let non_leaf_child_mask = curr_node.child_mask & !curr_node.leaf_mask;
                        let child_offset =
                            ((child_bit - 1) & non_leaf_child_mask).count_ones() as usize;
                        if !exists_as_internal {
                            curr_node.child_mask |= child_bit;
                            curr_node
                                .children
                                .insert(child_offset, Box::new(VoxelModelSFTNode::new_empty()));
                        }
                        curr_node = &mut curr_node.children[child_offset];
                    }
                }

                unreachable!()
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
                        dst_child_index,
                        attachment_id,
                        attachment.size() as usize,
                        src_data,
                    );
                }
            }

            // If voxel exists but there is no attachment data, remove the voxel.
            let dst_child_bit = 1 << dst_child_index;
            if count == 0 {
                // We need to actually remove the attachment data as well
                dst_node.child_mask &= !dst_child_bit;
                dst_node.leaf_mask &= !dst_child_bit;
                if let Some(attachment_data) = &mut dst_node.attachment_data {
                    for (attachment_id, (attachment_mask, data)) in attachment_data.iter_mut() {
                        if (*attachment_mask & dst_child_bit) > 0 {
                            let data_offset = (*attachment_mask & (dst_child_bit - 1)).count_ones();
                            *attachment_mask &= !dst_child_bit;
                            data.remove(data_offset as usize);
                        }
                    }
                }
            } else {
                dst_node.child_mask |= dst_child_bit;
                dst_node.leaf_mask |= dst_child_bit;
            }
        }
    }

    fn schema(&self) -> VoxelModelSchema {
        consts::voxel::MODEL_SFT_SCHEMA
    }

    fn length(&self) -> Vector3<u32> {
        Vector3::new(self.side_length, self.side_length, self.side_length)
    }
}

impl From<&VoxelModelSFTCompressed> for VoxelModelSFT {
    fn from(sft_compressed: &VoxelModelSFTCompressed) -> Self {
        let mut sft = VoxelModelSFT::new_empty(sft_compressed.side_length);
        sft.attachment_map = sft_compressed.attachment_map.clone();

        // Use a pointer because borrow checker wont like this.
        let mut to_process = vec![(
            /*curr_node_index*/ 0usize,
            std::ptr::from_mut(&mut sft.root_node),
        )];
        loop {
            let Some((curr_node_index, curr_sft_node)) = to_process.pop() else {
                break;
            };
            // Safety: This is only borrowed from either the root node or a nodes, children. A node
            // is not visited twice and it's Vec is reserved before any children are pushed,
            // meaning no reallocation should occur that causes dangling pointers.
            let curr_sft_node = unsafe { &mut *curr_sft_node };

            let compressed_node = &sft_compressed.node_data[curr_node_index];
            curr_sft_node.child_mask = compressed_node.child_mask;
            curr_sft_node.leaf_mask = compressed_node.leaf_mask;
            if curr_sft_node.leaf_mask > 0 {
                curr_sft_node.attachment_data =
                    Some(sft_compressed.collect_attachment_data(curr_node_index));
            }
            let non_leaf_child_mask = curr_sft_node.child_mask & !curr_sft_node.leaf_mask;
            curr_sft_node
                .children
                .reserve_exact(non_leaf_child_mask.count_ones() as usize);

            for i in 0..64 {
                let child_bit = 1 << i;
                let is_non_leaf_child = (child_bit & non_leaf_child_mask) > 0;
                if !is_non_leaf_child {
                    continue;
                }

                let child_offset = ((child_bit - 1) & non_leaf_child_mask).count_ones() as usize;
                let child_ptr = compressed_node.child_ptr as usize + child_offset;

                curr_sft_node
                    .children
                    .push(Box::new(VoxelModelSFTNode::new_empty()));
                to_process.push((
                    child_ptr,
                    std::ptr::from_mut(&mut curr_sft_node.children.last_mut().unwrap()),
                ));
            }
            assert_eq!(
                curr_sft_node.children.len(),
                non_leaf_child_mask.count_ones() as usize
            );
        }

        if sft.attachment_map.contains(Attachment::BMAT_ID) {
            sft.recompress_bmat();
        }
        log::info!("From compressed sft to sft, done");

        return sft;
    }
}

impl From<&VoxelModelFlat> for VoxelModelSFT {
    fn from(flat: &VoxelModelFlat) -> Self {
        VoxelModelSFT::from(&VoxelModelSFTCompressed::from(flat))
    }
}

impl From<&VoxelModelTHC> for VoxelModelSFT {
    fn from(thc: &VoxelModelTHC) -> Self {
        VoxelModelSFT::from(&VoxelModelSFTCompressed::from(
            &VoxelModelTHCCompressed::from(thc),
        ))
    }
}

impl From<&VoxelModelTHCCompressed> for VoxelModelSFT {
    fn from(thc_compressed: &VoxelModelTHCCompressed) -> Self {
        VoxelModelSFT::from(&VoxelModelSFTCompressed::from(thc_compressed))
    }
}
