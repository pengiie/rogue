use core::panic;
use std::{
    array,
    borrow::Borrow,
    collections::HashMap,
    fmt::{Pointer, Write},
    ops::BitOrAssign,
    u32,
};

use bytemuck::Pod;
use log::debug;
use nalgebra::Vector3;

use crate::{
    common::{aabb::AABB, bitset::Bitset, morton::morton_decode, ray::Ray},
    consts,
    engine::graphics::{
        device::{DeviceResource, GfxDevice},
        gpu_allocator::{Allocation, GpuBufferAllocator},
    },
};

use super::{
    attachment::{Attachment, AttachmentId, AttachmentMap},
    esvo::{VoxelModelESVO, VoxelModelESVONode},
    voxel::{
        VoxelData, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl,
        VoxelModelImplConcrete, VoxelModelRange, VoxelModelSchema,
    },
};

/// A float 1D array representing a 3D voxel region.
#[derive(Clone)]
pub struct VoxelModelFlat {
    pub attachment_data: HashMap<AttachmentId, Vec<u32>>,
    pub attachment_presence_data: HashMap<AttachmentId, Bitset>,
    pub attachment_map: AttachmentMap,
    pub presence_data: Bitset,
    side_length: Vector3<u32>,
    volume: usize,
    update_tracker: u32,
}

impl VoxelModelFlat {
    pub fn new(
        presence_data: Bitset,
        attachment_data: HashMap<AttachmentId, Vec<u32>>,
        attachment_presence_data: HashMap<AttachmentId, Bitset>,
        attachment_map: AttachmentMap,
        length: Vector3<u32>,
    ) -> Self {
        let volume = length.product() as usize;

        // Ensure all data fits the expected volume.
        assert_eq!(presence_data.bits(), volume as usize);
        for (attachment_id, attachment) in attachment_map.iter() {
            assert_eq!(
                attachment_presence_data.get(attachment_id).unwrap().bits(),
                volume as usize
            );
            assert_eq!(
                attachment_data.get(attachment_id).unwrap().len(),
                attachment.size() as usize * volume
            );
        }

        Self {
            presence_data,
            attachment_data,
            attachment_presence_data,
            attachment_map: AttachmentMap::new(),
            side_length: length,
            volume,
            update_tracker: 0,
        }
    }

    pub fn new_empty(length: Vector3<u32>) -> Self {
        let volume = length.x * length.y * length.z;
        Self::new(
            Bitset::new(volume as usize),
            HashMap::new(),
            HashMap::new(),
            AttachmentMap::new(),
            length,
        )
    }

    pub fn is_empty(&self) -> bool {
        self.attachment_presence_data.is_empty()
    }

    pub fn get_voxel_index(&self, position: Vector3<u32>) -> usize {
        position.x as usize
            + position.y as usize * self.side_length.x as usize
            + position.z as usize * (self.side_length.x as usize * self.side_length.y as usize)
    }

    pub fn get_voxel_position(&self, index: usize) -> Vector3<u32> {
        Vector3::new(
            (index % self.side_length.x as usize) as u32,
            ((index / self.side_length.x as usize) % self.side_length.y as usize) as u32,
            (index / (self.side_length.x as usize * self.side_length.y as usize)) as u32,
        )
    }

    pub fn get_voxel(&self, position: Vector3<u32>) -> VoxelModelFlatVoxelAccess<'_> {
        let index = self.get_voxel_index(position);
        VoxelModelFlatVoxelAccess {
            flat_model: self,
            index,
        }
    }

    pub fn get_voxel_mut(&mut self, position: Vector3<u32>) -> VoxelModelFlatVoxelAccessMut<'_> {
        let index = self.get_voxel_index(position);
        VoxelModelFlatVoxelAccessMut {
            flat_model: self,
            index,
        }
    }

    pub fn remove_voxel(&mut self, position: Vector3<u32>) {
        let index = self.get_voxel_index(position);
        self.presence_data.set_bit(index, false);
        for (_, data) in &mut self.attachment_presence_data {
            data.set_bit(index, false);
        }
    }

    pub fn in_bounds(&self, position: Vector3<u32>) -> bool {
        !(position.x >= self.side_length.x
            || position.y >= self.side_length.y
            || position.z >= self.side_length.z)
    }

    pub fn side_length(&self) -> &Vector3<u32> {
        &self.side_length
    }

    pub fn volume(&self) -> usize {
        self.volume
    }

    pub fn xyz_iter(&self) -> VoxelModelFlatXYZIter {
        VoxelModelFlatXYZIter {
            flat_model: self,
            i: 0,
        }
    }

    pub fn xyz_iter_mut(&mut self) -> VoxelModelFlatXYZIterMut {
        VoxelModelFlatXYZIterMut {
            flat_model: self,
            i: 0,
            volume: self.volume,
            phantom: std::marker::PhantomData::default(),
        }
    }

    // Creates a rect with with given attributes for each voxel.
    // TODO: pub fn rect_filled(length: Vector3<u32>, voxel_data: VoxelData) -> Self {}
}

impl std::fmt::Debug for VoxelModelFlat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut nodes_str = String::new();
        nodes_str.push_str(&format!(
            "length: {}x{}x{}\n",
            self.side_length.x, self.side_length.y, self.side_length.z
        ));
        if (self.side_length.x * self.side_length.y * self.side_length.z > 256) {
            nodes_str.push_str("Flat model is too big to print out to stdout.\n");
        } else {
            nodes_str.push_str("presence: \n");
            for y in 0..self.side_length.x {
                nodes_str.push_str(&format!("Y: {}\n", y));
                for z in 0..self.side_length.y {
                    let mut row = String::new();
                    for x in 0..self.side_length.z {
                        let voxel = self.get_voxel(Vector3::new(x, y, z));
                        let char = if !voxel.exists() { '0' } else { '1' };
                        row.push(char);
                    }
                    row.push_str("\n");
                    nodes_str.push_str(&row);
                }
            }
        }

        f.write_fmt(format_args!("VoxelFlat {{\n{}}}", nodes_str))
    }
}

pub struct VoxelModelFlatXYZIter<'a> {
    flat_model: &'a VoxelModelFlat,
    i: usize,
}

impl<'a> Iterator for VoxelModelFlatXYZIter<'a> {
    type Item = (Vector3<u32>, &'a VoxelModelFlat);

    fn next(&mut self) -> Option<Self::Item> {
        if self.i < self.flat_model.volume() {
            let position = self.flat_model.get_voxel_position(self.i);

            self.i += 1;

            Some((position, self.flat_model))
        } else {
            None
        }
    }
}

pub struct VoxelModelFlatXYZIterMut<'a> {
    flat_model: *mut VoxelModelFlat,
    i: usize,
    volume: usize,
    phantom: std::marker::PhantomData<&'a mut VoxelModelFlat>,
}

impl<'a> Iterator for VoxelModelFlatXYZIterMut<'a> {
    type Item = (Vector3<u32>, VoxelModelFlatVoxelAccessMut<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.i < self.volume {
            // Safe since we don't access the same x,y,z value twice, so we only have one mutable
            // reference.
            unsafe {
                let index = self.i;
                let position = self.flat_model.as_ref().unwrap().get_voxel_position(index);

                self.i += 1;

                let flat_model = self.flat_model.as_mut().unwrap();
                Some((position, VoxelModelFlatVoxelAccessMut { flat_model, index }))
            }
        } else {
            None
        }
    }
}

pub struct VoxelModelFlatVoxelAccess<'a> {
    pub flat_model: &'a VoxelModelFlat,
    pub index: usize,
}

impl<'a> VoxelModelFlatVoxelAccess<'a> {
    pub fn exists(&self) -> bool {
        self.flat_model.presence_data.get_bit(self.index)
    }

    pub fn get_attachment_data(&self) -> impl Iterator<Item = (u8, &[u32])> {
        self.flat_model
            .attachment_data
            .iter()
            .filter_map(|(attachment, data)| {
                let exists = self
                    .flat_model
                    .attachment_presence_data
                    .get(attachment)
                    .expect(
                        "If raw attachment data exists then it's presence data should also exist.",
                    )
                    .get_bit(self.index);
                if !exists {
                    return None;
                }

                let attachment = self.flat_model.attachment_map.get_attachment(*attachment);
                let i = attachment.size() as usize * self.index;
                let data = &data[i..(i + attachment.size() as usize)];
                Some((attachment.id(), data))
            })
    }
}

pub struct VoxelModelFlatVoxelAccessMut<'a> {
    pub flat_model: &'a mut VoxelModelFlat,
    pub index: usize,
}

impl<'a> VoxelModelFlatVoxelAccessMut<'a> {
    /// Expects attachment id to be registered and present, and hashmaps to be populated.
    pub fn set_attachment_id(&mut self, attachment_id: u8, value: &[u32]) {
        let attachment = self.flat_model.attachment_map.get_attachment(attachment_id);
        self.flat_model.presence_data.set_bit(self.index, true);
        self.flat_model
            .attachment_presence_data
            .get_mut(&attachment_id)
            .unwrap()
            .set_bit(self.index, true);
        let initial_offset = self.index * attachment.size() as usize;
        self.flat_model
            .attachment_data
            .get_mut(&attachment_id)
            .unwrap()[initial_offset..(initial_offset + attachment.size() as usize)]
            .copy_from_slice(value);
    }
    pub fn set_attachment<T: Pod>(&mut self, attachment: Attachment, value: Option<T>) {
        self.flat_model
            .presence_data
            .set_bit(self.index, value.is_some());

        if let Some(value) = value {
            self.flat_model
                .attachment_map
                .register_attachment(&attachment);

            // If the attachment presence data doesn't exist, that means the raw array also doesn't
            // exist so initialize both.
            if !self
                .flat_model
                .attachment_presence_data
                .contains_key(&attachment.id())
            {
                self.flat_model
                    .attachment_presence_data
                    .insert(attachment.id(), Bitset::new(self.flat_model.volume));
                self.flat_model.attachment_data.insert(
                    attachment.id(),
                    vec![0u32; attachment.size() as usize * self.flat_model.volume()],
                );
            }

            // Mark attachment presence.
            self.flat_model
                .attachment_presence_data
                .get_mut(&attachment.id())
                .unwrap()
                .set_bit(self.index, true);

            assert_eq!(std::mem::size_of::<T>(), attachment.size() as usize * 4);
            let mut attachment_data = self
                .flat_model
                .attachment_data
                .get_mut(&attachment.id())
                .unwrap()
                .as_mut_slice();

            let value = bytemuck::cast_slice::<u8, u32>(bytemuck::bytes_of(&value));
            let initial_offset = self.index * attachment.size() as usize;
            for i in 0..attachment.size() as usize {
                attachment_data[initial_offset + i] = value[i];
            }
        } else {
            if let Some(attachment_presence_data) = self
                .flat_model
                .attachment_presence_data
                .get_mut(&attachment.id())
            {
                // Unmark attachment presence if it exists.
                attachment_presence_data.set_bit(self.index, false);

                // If there are no attachments related to this voxel, then clear its general
                // presence since it holds no data.
                let mut should_clear = true;
                for (attachment_presence) in self.flat_model.attachment_presence_data.values() {
                    if attachment_presence.get_bit(self.index) {
                        should_clear = false;
                        break;
                    }
                }

                if should_clear {
                    self.flat_model.presence_data.set_bit(self.index, false);
                }
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct FlatESVONode {
    child_ptr: u32,
    info: u32,
}

impl FlatESVONode {
    pub fn zero() -> Self {
        FlatESVONode {
            child_ptr: u32::MAX,
            info: 0,
        }
    }

    pub fn into_esvo_node(&self, child_ptr: u32, far: bool) -> VoxelModelESVONode {
        VoxelModelESVONode::encode_node(child_ptr, far, self.valid_mask(), self.leaf_mask())
    }

    pub fn empty() -> Self {
        FlatESVONode {
            child_ptr: u32::MAX,
            info: 0x8000_0000,
        }
    }

    pub fn set_empty(&mut self, empty: bool) {
        if empty {
            self.info |= 0x8000_0000;
        } else {
            self.info &= 0x7FFF_FFFF;
        }
    }

    pub fn child_node_count(&self) -> u32 {
        self.valid_mask().count_ones() - self.leaf_mask().count_ones()
    }

    pub fn is_empty(&self) -> bool {
        (self.info >> 31) > 0
    }

    pub fn set_leaf_mask(&mut self, leaf_mask: u32) {
        self.info &= 0xFFFF_FF00;
        self.info |= leaf_mask;
    }

    pub fn leaf_mask(&self) -> u32 {
        self.info & 0xFF
    }

    pub fn valid_mask(&self) -> u32 {
        (self.info >> 8) & 0xFF
    }

    pub fn node_mask(&self) -> u32 {
        self.valid_mask() & !self.leaf_mask()
    }

    pub fn set_valid_mask(&mut self, valid_mask: u32) {
        self.info &= 0xFFFF_00FF;
        self.info |= valid_mask << 8;
    }
}

impl From<VoxelModelFlat> for VoxelModelESVO {
    fn from(flat: VoxelModelFlat) -> Self {
        From::from(&flat)
    }
}

impl From<&VoxelModelFlat> for VoxelModelESVO {
    fn from(flat: &VoxelModelFlat) -> Self {
        let length = flat
            .side_length()
            .map(|x| x.next_power_of_two())
            .max()
            .max(2);
        let mut esvo = VoxelModelESVO::empty(length, true);
        esvo.attachment_map = flat.attachment_map.clone();

        // With just the root node being a height of 1, since log2(2) == 1.
        let height = length.trailing_zeros() as usize;
        let mut levels = (0..=height)
            .map(|_| Vec::new())
            .collect::<Vec<Vec<FlatESVONode>>>();
        let mut node_list: Vec<FlatESVONode> = Vec::new();

        for i in 0..esvo.volume() {
            let pos = morton_decode(i as u64);

            if flat.in_bounds(pos) {
                let flat_voxel = flat.get_voxel(pos);

                if flat_voxel.exists() {
                    levels[height as usize].push(FlatESVONode::zero())
                } else {
                    levels[height as usize].push(FlatESVONode::empty())
                }

                // Try and pop a level if it is full recursively.
                for h in (1..=height).rev() {
                    if levels[h as usize].len() != 8 {
                        break;
                    }
                    let mut child_mask = 0;
                    let mut child_ptr = u32::MAX;
                    for octant in 0..8usize {
                        if levels[h][octant].is_empty() {
                            continue;
                        }
                        child_mask |= 1 << octant;
                        if h == height {
                            continue;
                        }
                        if child_ptr == u32::MAX {
                            child_ptr = node_list.len() as u32;
                        }
                        node_list.push(levels[h][octant].clone());
                    }
                    levels[h].clear();

                    if child_mask == 0 {
                        levels[h - 1].push(FlatESVONode::empty());
                        continue;
                    }
                    //debug!("STACKING OUT LEVEL {}", h);

                    // Since we are on the leaf layer, we don't have any nodes to push.
                    let mut meta_node = FlatESVONode::zero();
                    meta_node.set_valid_mask(child_mask);
                    if h < height {
                        meta_node.child_ptr = child_ptr;
                    } else {
                        meta_node.set_leaf_mask(child_mask);
                    }
                    //debug!("height: {}, pushed: {}", h, child_mask);
                    levels[h - 1].push(meta_node);
                }
            }
        }

        node_list.push(levels[0][0].clone());

        let mut to_process = Vec::new();
        //debug!("ROOT node index is {}", node_list.len() - 1);
        let root_node = node_list.last().unwrap();
        to_process.push((root_node, 1, 0));

        let mut attachment_info: HashMap<
            AttachmentId,
            (u32, u32), /* (attachment_mask, raw_attachment_ptr) */
        > = HashMap::new();
        while !to_process.is_empty() {
            let (curr_flat_node, curr_esvo_node_index, traversal) = to_process.pop().unwrap();

            // We can override the entire node data since we allocate the children right after.
            let curr_esvo_node = esvo.get_node_mut(curr_esvo_node_index);
            curr_esvo_node.0 = curr_flat_node.info & 0xFFFF;
            //debug!(
            //    "Addressing current esvo index {} with traversal {}",
            //    curr_esvo_node_index, traversal
            //);

            let children_allocation_esvo_ptr = if curr_flat_node.child_ptr == u32::MAX {
                0
            } else {
                let child_count = curr_flat_node.child_node_count();
                assert!(
                    child_count != 0,
                    "Child count should not be 0 here, leaf count: {}",
                    curr_flat_node.leaf_mask().count_ones()
                );
                esvo.allocate_node_children(curr_esvo_node_index, curr_flat_node.child_node_count())
            };

            attachment_info.clear();
            for octant in 0..8 {
                let octant_bit = 1 << octant;
                if (curr_flat_node.valid_mask() & octant_bit) == 0 {
                    continue;
                }

                let octant_traversal = (traversal << 3) | octant;
                // Skip leaf voxels since we don't spawn nodes for them.
                if (curr_flat_node.leaf_mask() & octant_bit) > 0 {
                    // Parse as a child node, collect the attachment data from flat and translate
                    // into esvo.
                    let position = morton_decode(octant_traversal);
                    assert!(flat.in_bounds(position));

                    let flat_voxel = flat.get_voxel(position);
                    for (attachment_id, attachment_data) in flat_voxel.get_attachment_data() {
                        let (attachment_mask, attachment_ptr) = attachment_info
                            .entry(attachment_id)
                            .or_insert((0, u32::MAX));

                        *attachment_mask |= octant_bit;
                        if *attachment_ptr == u32::MAX {
                            *attachment_ptr = esvo.allocate_raw_attachment_data(attachment_id, 8);
                        }

                        let attachment_offset =
                            (*attachment_mask as u32 & (octant_bit - 1)).count_ones();
                        let attachment = flat.attachment_map.get_attachment(attachment_id);
                        let child_attachment_ptr =
                            *attachment_ptr + attachment_offset * attachment.size();
                        let esvo_raw_attachment_range = child_attachment_ptr as usize
                            ..(child_attachment_ptr + attachment.size()) as usize;
                        esvo.attachment_raw_data.get_mut(&attachment_id).unwrap()
                            [esvo_raw_attachment_range]
                            .copy_from_slice(attachment_data);
                    }
                } else {
                    // Parse as another esvo node.
                    let child_offset =
                        ((curr_flat_node.node_mask()) & (octant_bit - 1)).count_ones();
                    let child_flat_node =
                        &node_list[(curr_flat_node.child_ptr + child_offset) as usize];
                    //debug!(
                    //    "pushing child index base {}  with res {} {} {}",
                    //    children_allocation_esvo_ptr,
                    //    children_allocation_esvo_ptr + child_offset as u32,
                    //    child_offset,
                    //    curr_flat_node.node_mask()
                    //);
                    //debug!(
                    //    "Current flat node child ptr is {}",
                    //    curr_flat_node.child_ptr
                    //);
                    to_process.push((
                        child_flat_node,
                        children_allocation_esvo_ptr + child_offset as u32,
                        octant_traversal,
                    ));
                }
            }

            for (attachment_id, (attachment_mask, attachment_ptr)) in &attachment_info {
                let lookup_node =
                    esvo.get_attachment_lookup_node_mut(*attachment_id, curr_esvo_node_index);
                lookup_node.set_attachment_mask(*attachment_mask);
                lookup_node.set_raw_index(*attachment_ptr);
                let attachment = flat.attachment_map.get_attachment(*attachment_id);
                let used_raw_size = attachment_mask.count_ones() * attachment.size();
                esvo.resize_raw_attachment_data(*attachment_id, attachment_ptr + used_raw_size);
            }
        }
        //debug!("FLAT to ESVO finale: {:?}", esvo);

        esvo
    }
}

impl VoxelModelImplConcrete for VoxelModelFlat {
    type Gpu = VoxelModelFlatGpu;
}

impl VoxelModelImpl for VoxelModelFlat {
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<Vector3<u32>> {
        let mut ray = ray.clone();
        let Some(model_t) = ray.intersect_aabb(aabb) else {
            return None;
        };
        ray.advance(model_t);

        let mut dda = ray.begin_dda(aabb, self.side_length);
        while (dda.in_bounds()) {
            let curr_grid_pos = dda.curr_grid_pos().map(|x| x as u32);
            let voxel = self.get_voxel(curr_grid_pos);
            if (voxel.exists()) {
                return Some(curr_grid_pos);
            }
            dda.step();
        }

        return None;
    }

    fn set_voxel_range_impl(&mut self, range: &VoxelModelRange) {
        self.update_tracker += 1;
        let other = &range.data;
        self.attachment_map.inherit_other(&other.attachment_map);
        for (attachment_id, attachment) in other.attachment_map.iter() {
            if !self.attachment_presence_data.contains_key(attachment_id) {
                assert!(!self.attachment_data.contains_key(attachment_id));
                self.attachment_presence_data
                    .insert(*attachment_id, Bitset::new(self.volume));
                self.attachment_data.insert(
                    attachment.id(),
                    vec![0u32; attachment.size() as usize * self.volume()],
                );
            }
        }

        for i in 0..other.volume {
            let src_voxel = VoxelModelFlatVoxelAccess {
                flat_model: other,
                index: i,
            };
            let dst_pos = range.offset + other.get_voxel_position(i);
            if src_voxel.exists() {
                let mut dst_voxel = self.get_voxel_mut(dst_pos);
                for (attachment_id, data) in src_voxel.get_attachment_data() {
                    dst_voxel.set_attachment_id(attachment_id, data);
                }
            } else {
                self.remove_voxel(dst_pos);
            }
        }
    }

    fn schema(&self) -> VoxelModelSchema {
        consts::voxel::MODEL_FLAT_SCHEMA
    }

    fn length(&self) -> Vector3<u32> {
        self.side_length
    }
}

pub struct VoxelModelFlatGpu {
    flat_length: Vector3<u32>,
    voxel_presence_allocation: Option<Allocation>,
    voxel_attachment_presence_allocations: HashMap<AttachmentId, Allocation>,
    voxel_attachment_data_allocations: HashMap<AttachmentId, Allocation>,

    initialized_data: bool,
    update_tracker: u32,
}

impl VoxelModelFlatGpu {
    pub fn on_model_update(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) {
    }
}

impl VoxelModelGpuImplConcrete for VoxelModelFlatGpu {
    fn new() -> Self {
        VoxelModelFlatGpu {
            flat_length: Vector3::zeros(),
            voxel_presence_allocation: None,
            voxel_attachment_presence_allocations: HashMap::new(),
            voxel_attachment_data_allocations: HashMap::new(),

            initialized_data: false,
            update_tracker: 0,
        }
    }
}

impl VoxelModelGpuImpl for VoxelModelFlatGpu {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        let Some(voxel_presence_allocation) = &self.voxel_presence_allocation else {
            return None;
        };
        if self.voxel_attachment_data_allocations.is_empty()
            || self.voxel_attachment_presence_allocations.is_empty()
        {
            return None;
        }

        let mut attachment_presence_indices =
            vec![u32::MAX; Attachment::MAX_ATTACHMENT_ID as usize + 1];
        for (attachment, allocation) in &self.voxel_attachment_presence_allocations {
            if *attachment > Attachment::MAX_ATTACHMENT_ID {
                continue;
            }

            attachment_presence_indices[*attachment as usize] =
                allocation.start_index_stride_dword() as u32
        }
        let mut attachment_data_indices =
            vec![u32::MAX; Attachment::MAX_ATTACHMENT_ID as usize + 1];
        for (attachment, allocation) in &self.voxel_attachment_data_allocations {
            if *attachment > Attachment::MAX_ATTACHMENT_ID {
                continue;
            }

            attachment_data_indices[*attachment as usize] =
                allocation.start_index_stride_dword() as u32;
        }

        let mut info = vec![
            // Flat length
            self.flat_length.x,
            self.flat_length.y,
            self.flat_length.z,
            // World data ptr (divide by 4 since 4 bytes in a u32)
            voxel_presence_allocation.start_index_stride_dword() as u32,
        ];
        info.append(&mut attachment_presence_indices);
        info.append(&mut attachment_data_indices);

        Some(info)
    }

    fn update_gpu_objects(
        &mut self,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool {
        let model = model.downcast_ref::<VoxelModelFlat>().unwrap();
        let mut did_allocate = false;

        if self.voxel_presence_allocation.is_none() {
            let presence_allocation_size = model.presence_data.data().len() * 4;
            self.voxel_presence_allocation = Some(
                allocator
                    .allocate(presence_allocation_size as u64)
                    .expect("Failed to allocate flat voxel presence data."),
            );
            did_allocate = true;
        }

        if self.update_tracker != model.update_tracker || !self.initialized_data {
            for (attachment_id, presence_bitset) in &model.attachment_presence_data {
                let req_presence_allocation_size = presence_bitset.data().len() as u64 * 4;
                match self
                    .voxel_attachment_presence_allocations
                    .entry(*attachment_id)
                {
                    std::collections::hash_map::Entry::Occupied(mut e) => {
                        let old_allocation = e.get();
                        if old_allocation.length_bytes() < req_presence_allocation_size {
                            let new_allocation = allocator
                                .reallocate(e.get(), req_presence_allocation_size)
                                .expect("Failed to reallocate flat attachment presence data.");

                            // If we reallocate we may keep the same pointer, so we need to
                            // only update our allocation info if the pointer is different.
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
                                .allocate(req_presence_allocation_size as u64)
                                .expect("Failed to allocate flat attachment presence data."),
                        );
                        did_allocate = true;
                    }
                }
            }

            for (attachment_id, attachment_data) in &model.attachment_data {
                let req_data_allocation_size = attachment_data.len() as u64 * 4;
                match self.voxel_attachment_data_allocations.entry(*attachment_id) {
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
        }

        return did_allocate;
    }

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        let model = model.downcast_ref::<VoxelModelFlat>().unwrap();

        // Update the gpu model if we haven't initialized or the model has been updated.
        if (!self.initialized_data || self.update_tracker != model.update_tracker)
            && self.voxel_presence_allocation.is_some()
        {
            self.flat_length = model.side_length;

            allocator.write_allocation_data(
                device,
                self.voxel_presence_allocation.as_ref().unwrap(),
                bytemuck::cast_slice::<u32, u8>(model.presence_data.data()),
            );

            for (attachment_id, presence_data) in &model.attachment_presence_data {
                let allocation = self
                    .voxel_attachment_presence_allocations
                    .get(attachment_id)
                    .expect(
                    "Voxel attachment presence allocation should've been allocated at this point.",
                );

                allocator.write_allocation_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(presence_data.data()),
                );
            }

            for (attachment_id, attachment_data) in &model.attachment_data {
                let allocation = self
                    .voxel_attachment_data_allocations
                    .get(attachment_id)
                    .expect(
                    "Voxel attachment presence allocation should've been allocated at this point.",
                );

                allocator.write_allocation_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(attachment_data),
                );
            }

            if self.initialized_data {
                debug!(
                    "Flat model updating from update {} to {}.",
                    self.update_tracker, model.update_tracker
                );
            }
            self.update_tracker = model.update_tracker;
            self.initialized_data = true;
            return;
        }
    }
}
