use std::{collections::HashMap, ops::Deref};

use nalgebra::Vector3;

use super::{
    attachment::{Attachment, AttachmentId, AttachmentInfoMap},
    flat::VoxelModelFlat,
    sft::VoxelModelSFT,
    sft_compressed_gpu::VoxelModelSFTCompressedGpu,
    sft_gpu::VoxelModelSFTGpu,
    voxel::{VoxelModelImpl, VoxelModelImplMethods},
};
use crate::{common::geometry::ray::Ray, voxel::voxel::VoxelModelEditRegion};
use crate::{common::morton, consts};
use crate::{
    common::{color::Color, geometry::ray::RayAABBHitInfo},
    voxel::{
        attachment::{AttachmentMap, BuiltInMaterial},
        rvox_asset::RVOXAsset,
        voxel::{VoxelMaterialData, VoxelModelTrace},
    },
};

#[derive(Copy, Clone)]
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
    pub update_tracker: u32,
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
            update_tracker: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.node_data[0].child_mask == 0
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

    pub fn set_voxel(&mut self, position: Vector3<u32>, material: Option<&VoxelMaterialData>) {
        assert!(self.in_bounds_local(position.cast::<i32>()));
        let height = self.tree_height() - 1;

        let mut curr_node_index = 0;
        let mut curr_child_pos = position.map(|x| (x >> ((height) * 2)) & 3);
        let mut curr_child_index = morton::morton_encode(curr_child_pos);
        for i in 0..height {
            let curr_node = self.node_data[curr_node_index].clone();
            let child_ptr = curr_node.child_ptr;
            let is_present = (curr_node.child_mask & (1 << curr_child_index)) > 0;

            if is_present {
                let child_offset =
                    (curr_node.child_mask & ((1 << curr_child_index) - 1)).count_ones();
                curr_node_index = (curr_node.child_ptr + child_offset) as usize;
                curr_child_pos = position.map(|x| (x >> ((height - i - 1) * 2)) & 3);
                curr_child_index = morton::morton_encode(curr_child_pos);
            } else {
                if material.is_some() {
                    let old_child_count = self.node_data[curr_node_index].child_mask.count_ones();
                    self.node_data[curr_node_index].child_mask |= (1 << curr_child_index);
                    let child_offset = (self.node_data[curr_node_index].child_mask
                        & ((1 << curr_child_index) - 1))
                        .count_ones();
                    let new_child_ptr = self.node_data.len();
                    for i in 0..child_offset {
                        let n = self.node_data[(child_ptr + i) as usize];
                        self.node_data.push(n);
                        for attachment_lookup_data in self.attachment_lookup_data.values_mut() {
                            attachment_lookup_data
                                .push(attachment_lookup_data[(child_ptr + i) as usize].clone());
                        }
                    }

                    self.node_data.push(SFTNodeCompressed::new_empty());
                    for attachment_lookup_data in self.attachment_lookup_data.values_mut() {
                        attachment_lookup_data.push(SFTAttachmentLookupNodeCompressed::new_empty());
                    }

                    for i in child_offset..old_child_count {
                        self.node_data
                            .push(self.node_data[(child_ptr + i) as usize]);
                        for attachment_lookup_data in self.attachment_lookup_data.values_mut() {
                            attachment_lookup_data
                                .push(attachment_lookup_data[(child_ptr + i) as usize].clone());
                        }
                    }
                    assert_eq!(
                        old_child_count as usize + 1,
                        self.node_data.len() - new_child_ptr
                    );
                    self.node_data[curr_node_index].child_ptr = new_child_ptr as u32;
                    curr_node_index = new_child_ptr + child_offset as usize;
                    curr_child_pos = position.map(|x| (x >> ((height - i - 1) * 2)) & 3);
                    curr_child_index = morton::morton_encode(curr_child_pos);
                } else {
                    // Node is already empty.
                    return;
                }
            }
        }

        let curr_node = &mut self.node_data[curr_node_index];
        let child_bit = 1 << curr_child_index;

        let bmat_lookup_node = &mut self
            .attachment_lookup_data
            .get_mut(Attachment::BMAT_ID)
            .unwrap()[curr_node_index];
        let bmat_attachment_data = self
            .attachment_raw_data
            .get_mut(Attachment::BMAT_ID)
            .unwrap();
        let attachment_exists = bmat_lookup_node.attachment_mask & child_bit > 0;
        if material.is_some() {
            let comp_mat = material.unwrap().encode();

            curr_node.child_mask |= child_bit;
            curr_node.leaf_mask |= child_bit;
            if attachment_exists {
                let attachment_offset =
                    (bmat_lookup_node.attachment_mask & (child_bit - 1)).count_ones() as usize;
                let data_ptr = bmat_lookup_node.data_ptr() as usize;
                let start = data_ptr + attachment_offset * Attachment::BMAT.size() as usize;
                let end = start + Attachment::BMAT.size() as usize;
                bmat_attachment_data[start..end]
                    .copy_from_slice(bytemuck::cast_slice(&comp_mat.to_le_bytes()));
            } else {
                bmat_lookup_node.attachment_mask |= child_bit;
                let attachment_offset =
                    (bmat_lookup_node.attachment_mask & (child_bit - 1)).count_ones();
                let attachment_leaf_count = bmat_lookup_node.attachment_mask.count_ones();
                let data_ptr = bmat_attachment_data.len();
                for i in 0..(attachment_offset * Attachment::BMAT.size()) {
                    bmat_attachment_data
                        .push(bmat_attachment_data[(bmat_lookup_node.data_ptr + i) as usize]);
                }
                bmat_attachment_data
                    .extend_from_slice(bytemuck::cast_slice(&comp_mat.to_le_bytes()));
                for i in (attachment_offset * Attachment::BMAT.size())
                    ..((attachment_leaf_count - 1) * Attachment::BMAT.size())
                {
                    bmat_attachment_data
                        .push(bmat_attachment_data[(bmat_lookup_node.data_ptr + i) as usize]);
                }
                bmat_lookup_node.data_ptr = data_ptr as u32;
            }
        } else {
            curr_node.child_mask &= !child_bit;
            curr_node.leaf_mask &= !child_bit;
            if attachment_exists {
                let attachment_offset =
                    (bmat_lookup_node.attachment_mask & (child_bit - 1)).count_ones();
                bmat_lookup_node.attachment_mask &= !child_bit;
                let attachment_count = bmat_lookup_node.attachment_mask.count_ones();

                // Copy all the attachment data after this leaf back one voxel to account for the
                // removed voxel.
                let data_ptr = bmat_lookup_node.data_ptr();
                for i in attachment_offset..attachment_count {
                    let offset = i * Attachment::BMAT.size();
                    for j in 0..Attachment::BMAT.size() {
                        bmat_attachment_data[(data_ptr + offset + j) as usize] =
                            bmat_attachment_data
                                [(data_ptr + offset + Attachment::BMAT.size() + j) as usize];
                    }
                }
            }
        }
    }

    pub fn side_length(&self) -> u32 {
        return self.side_length;
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

    pub fn get_voxel(&self, position: Vector3<u32>) -> Option<VoxelMaterialData> {
        assert!(self.in_bounds_local(position.cast::<i32>()));
        let height = self.tree_height() - 1;

        let mut curr_node_index = 0;
        let mut curr_child_pos = position.map(|x| (x >> ((height) * 2)) & 3);
        let mut curr_child_index = morton::morton_encode(curr_child_pos);
        for i in 0..height {
            let curr_node = self.node_data[curr_node_index].clone();
            let child_ptr = curr_node.child_ptr;
            let is_present = (curr_node.child_mask & (1 << curr_child_index)) > 0;

            if !is_present {
                return None;
            }
            let child_offset = (curr_node.child_mask & ((1 << curr_child_index) - 1)).count_ones();
            curr_node_index = (curr_node.child_ptr + child_offset) as usize;
            curr_child_pos = position.map(|x| (x >> ((height - i - 1) * 2)) & 3);
            curr_child_index = morton::morton_encode(curr_child_pos);
        }

        let curr_node = &self.node_data[curr_node_index];
        let child_bit = 1 << curr_child_index;
        if (curr_node.leaf_mask & child_bit) == 0 {
            return None;
        }

        let bmat_lookup_node = &self
            .attachment_lookup_data
            .get(Attachment::BMAT_ID)
            .unwrap()[curr_node_index];
        let bmat_attachment_data = self.attachment_raw_data.get(Attachment::BMAT_ID).unwrap();
        let attachment_exists = bmat_lookup_node.attachment_mask & child_bit > 0;
        if !attachment_exists {
            return None;
        }
        let attachment_offset =
            (bmat_lookup_node.attachment_mask & (child_bit - 1)).count_ones() as usize;
        let data_ptr = bmat_lookup_node.data_ptr() as usize;
        // Stored little endian in terms of u32s
        let a = bmat_attachment_data
            [data_ptr + attachment_offset * Attachment::BMAT.size() as usize]
            as u64;
        let b = bmat_attachment_data
            [data_ptr + attachment_offset * Attachment::BMAT.size() as usize + 1]
            as u64;
        return Some(VoxelMaterialData::decode((b << 32) | a));
    }
}

impl VoxelModelImpl for VoxelModelSFTCompressed {
    const NAME: &'static str = "SFTCompressed";

    fn get_voxel(&self, position: Vector3<u32>) -> Option<VoxelMaterialData> {
        Self::get_voxel(self, position)
    }

    fn clear(&mut self) {
        self.node_data = vec![SFTNodeCompressed::new_empty()];
        for attachment_lookup_data in self.attachment_lookup_data.values_mut() {
            *attachment_lookup_data = vec![SFTAttachmentLookupNodeCompressed::new_empty()];
        }
        for attachment_raw_data in self.attachment_raw_data.values_mut() {
            attachment_raw_data.clear();
        }
    }

    fn resize_model(&mut self, new_side_length: Vector3<u32>) {
        assert!(
            new_side_length.x == new_side_length.y && new_side_length.y == new_side_length.z,
            "Should check if the side length is valid for the model before resizing."
        );
        let new_side_length = new_side_length.x;
        assert_ne!(
            new_side_length, self.side_length,
            "New side length should be different."
        );

        let mut new_sft = Self::new_empty(new_side_length);
        new_sft.initialize_attachment_buffers(&Attachment::BMAT);

        if new_side_length > self.side_length {
            let offset = ((new_side_length - self.side_length) / 2) as u32;
            for x in 0..self.side_length {
                for y in 0..self.side_length {
                    for z in 0..self.side_length {
                        let pos = Vector3::new(x, y, z);

                        let mat = self.get_voxel(Vector3::new(x, y, z));
                        new_sft.set_voxel(pos + Vector3::new(offset, offset, offset), mat.as_ref());
                    }
                }
            }
        } else {
            // TODO: do this
        }
        new_sft.update_tracker = self.update_tracker + 1;

        *self = new_sft;
    }

    fn trace(
        &self,
        in_ray: &crate::common::geometry::ray::Ray,
        aabb: &crate::common::geometry::aabb::AABB,
    ) -> Option<super::voxel::VoxelModelTrace> {
        let mut ray = in_ray.clone();
        // Early exit if the ray doesn't intersect the bounding box of this model.
        let Some(RayAABBHitInfo {
            t_enter: model_t,
            t_min,
            ..
        }) = ray.intersect_aabb(aabb)
        else {
            return None;
        };
        ray.advance(model_t);

        // DDA through the 4x4x4 nodes at varying step sizes depending on how far we
        // are down the tree. While we DDA we check if the current child exists, if it is
        // a node we push onto the traversal stack of the last node index we were at and
        // decrease our step size by a fourth. Effectively doing DDA on that child node now.
        // If if a leaf we calculate the voxel position dcepending on the leaf node intersection
        // and return with that.
        let local_pos = ray.origin - aabb.min;
        let norm_pos = local_pos.zip_map(&aabb.side_length(), |x, y| (x / y).clamp(0.0, 0.9999));
        // Our scaled position from [0, bounds).
        let dda_pos = norm_pos * self.side_length as f32;

        let height = self.tree_height() - 1;
        let sl = self.side_length;
        let quarter_sl = self.side_length >> 2;
        let unit_grid = ray.dir.map(|x| x.signum() as i32);

        let mut last_mask = t_min.map(|x| if (x - model_t).abs() < 0.0001 { 1 } else { 0 });
        let dir_scaling = aabb.side_length() / (self.side_length as f32 * consts::voxel::VOXEL_METER_LENGTH);
        let norm_dir = ray.dir.component_div(&dir_scaling).normalize();
        let mut curr_ray = Ray::new(dda_pos, norm_dir);
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
            assert!(curr_height <= height);
            i += 1;
            let should_pop = curr_local_grid.x < 0
                || curr_local_grid.y < 0
                || curr_local_grid.z < 0
                || curr_local_grid.x > 3
                || curr_local_grid.y > 3
                || curr_local_grid.z > 3;
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
                let is_child_present = (curr_node.child_mask & (1 << child_index)) > 0;
                if is_child_present {
                    let node_size = quarter_sl >> (curr_height * 2);
                    curr_anchor =
                        curr_anchor.zip_map(&curr_local_grid, |x, y| x + y as u32 * node_size);
                    let global_grid_pos = curr_ray.origin.zip_map(&curr_anchor, |x, y| {
                        (x.floor() as u32).clamp(y, y + node_size - 1)
                    });

                    let is_leaf = (curr_node.leaf_mask & (1 << child_index)) > 0;
                    if is_leaf {
                        let t_scaling = (aabb.max - aabb.min) / (sl as f32);
                        let world_pos_hit = aabb.min + curr_ray.origin.component_mul(&t_scaling);
                        let depth_t = in_ray.origin.metric_distance(&world_pos_hit);
                        let normal = last_mask.component_mul(&ray.dir.map(|x| -x.signum() as i32));
                        return Some(VoxelModelTrace {
                            local_position: global_grid_pos,
                            depth_t,
                            local_normal: normal,
                        });
                    }

                    assert!(curr_node.child_ptr != u32::MAX);
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
                + (curr_local_grid.map(|x| x as u32) + unit_grid.map(|x| x.max(0) as u32))
                    * node_size;
            let next_t = curr_ray.intersect_point(next_point.cast::<f32>());
            let min_t = next_t.min();
            let mask = next_t.map(|x| if x == min_t { 1 } else { 0 });
            last_mask = mask;

            curr_local_grid += unit_grid.component_mul(&mask);
            // Epsilon since sometimes we advance out of bounds but due to fp math it's just barely
            // off, messing up the traversal.
            curr_ray.advance(min_t + 0.0001);
        }

        return None;
    }

    fn set_voxel_range_impl(&mut self, edit: &super::voxel::VoxelModelEdit) {
        let sl = self.side_length;
        let volume = (sl as u64).pow(3);

        let calculate_mask_weight = |prev_voxel: Option<VoxelMaterialData>,
                                     voxel_pos: Vector3<u32>| {
            let mut weight = 1.0;
            for mask in &edit.mask.layers {
                match mask {
                    crate::voxel::voxel::VoxelModelEditMaskLayer::Presence => {
                        if prev_voxel.is_none() {
                            weight = 0.0;
                            break;
                        }
                    }
                    crate::voxel::voxel::VoxelModelEditMaskLayer::Sphere { center, diameter } => {
                        let mut center = center.cast::<f32>();
                        let mut radius = *diameter as f32 / 2.0;
                        if diameter % 2 == 0 {
                            center += Vector3::new(0.5, 0.5, 0.5);
                        }
                        if (Vector3::new(
                            voxel_pos.x as f32,
                            voxel_pos.y as f32,
                            voxel_pos.z as f32,
                        ) - center)
                            .norm()
                            > radius
                        {
                            weight = 0.0;
                            break;
                        }
                    }
                }
            }
            return weight;
        };

        let mut region_min_max = |min: Vector3<u32>, max: Vector3<u32>| {
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        let voxel_pos = Vector3::new(x, y, z);
                        let prev_mat = if let Some(mask_source) = &edit.mask.mask_source {
                            let sample_pos =
                                voxel_pos.cast::<i32>() - mask_source.offset.cast::<i32>();
                            mask_source.source.get_voxel(sample_pos)
                        } else {
                            self.get_voxel(voxel_pos)
                        };
                        let weight = calculate_mask_weight(prev_mat, voxel_pos);
                        if weight == 0.0 {
                            continue;
                        }
                        match &edit.operator {
                            crate::voxel::voxel::VoxelModelEditOperator::Replace(
                                voxel_material_data,
                            ) => {
                                self.update_tracker += 1;
                                self.set_voxel(Vector3::new(x, y, z), voxel_material_data.as_ref());
                            }
                        }
                    }
                }
            }
        };

        match &edit.region {
            VoxelModelEditRegion::Rect { min, max } => {
                region_min_max(*min, *max);
            }
            VoxelModelEditRegion::Intersect(voxel_model_edit_regions) => todo!(),
        }
    }

    fn length(&self) -> Vector3<u32> {
        return Vector3::new(self.side_length, self.side_length, self.side_length);
    }

    fn create_rvox_asset(&self) -> RVOXAsset {
        RVOXAsset {
            sft_compressed: self.clone(),
        }
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

        // Under-estimate with 64 voxel per leaf node. This assumes each voxel perfectly fills
        // up a leaf node but realistically the voxels will be sparse and there are internal
        // nodes.
        let mut reserved_estimate_node_count =
            flat.presence_data.one_bits() / /*leaves per node*/64;
        let mut node_list_rev: Vec<SFTNodeCompressed> =
            Vec::with_capacity(reserved_estimate_node_count);
        let mut attachment_lookup_data: AttachmentMap<Vec<SFTAttachmentLookupNodeCompressed>> =
            AttachmentMap::new();
        for (attachment_id, _) in flat.attachment_map.iter() {
            attachment_lookup_data.insert(
                attachment_id,
                Vec::with_capacity(reserved_estimate_node_count),
            );
        }
        let mut attachment_raw_data: AttachmentMap<Vec<u32>> = AttachmentMap::new();
        for (attachment_id, _) in flat.attachment_map.iter() {
            // Attrachment per leaf node with this attachment.
            let reserve_estimate = flat
                .attachment_presence_data
                .get(attachment_id)
                .unwrap()
                .one_bits();
            // TODO: This estimate is bad since according to heaptrack we leak a lot with this if
            // we don't shrink so try to figure out why the estimate is bad since to me it looks
            // good idk.
            attachment_raw_data.insert(attachment_id, Vec::with_capacity(reserve_estimate));
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
                // Change to enable or disable bmat compression.
                let mut homogenous = false;
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
                                    dst_raw_data.extend(data.iter().rev());
                                    let attachment_ptr = dst_raw_data.len() - 1;

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
                                let data = VoxelMaterialData::Unbaked(id).encode();
                                dst_raw_data.push((data >> 32) as u32);
                                dst_raw_data.push((data & 0xFFFF_FFFF) as u32);

                                let mut curr_lookup_node =
                                new_attachment_map.get_mut(
                                    Attachment::BMAT_ID).expect("Flat voxel builtin attachment is not present in the attachment info map when it should be.");
                                curr_lookup_node.attachment_mask |= child_bit;
                                curr_lookup_node.data_ptr = attachment_ptr;
                            }
                        }
                        SFTFlatNode::Child((child_node, child_lookup_nodes)) => {
                            homogenous = false;
                            let child_bit = 1 << child_index;
                            child_mask |= child_bit;
                            child_ptr = node_list_rev.len() as u32;
                            node_list_rev.push(child_node);
                            for (attachment_id, lookup_node) in child_lookup_nodes.into_iter() {
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
                    update_tracker: 0,
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

            if raw_data_len > 0 {
                for lookup_node in lookup_data.iter_mut() {
                    lookup_node.data_ptr = raw_data_len - 1 - lookup_node.data_ptr;
                }
                raw_data.reverse();
            }

            lookup_data.reverse();
        }

        for (_, attachment_data) in attachment_raw_data.iter_mut() {
            attachment_data.shrink_to_fit();
        }

        return VoxelModelSFTCompressed {
            side_length: length,
            attachment_map: attachment_info_map,
            node_data,
            attachment_lookup_data,
            attachment_raw_data,
            update_tracker: 0,
        };
    }
}
