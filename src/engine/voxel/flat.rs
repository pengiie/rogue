use core::panic;
use std::{
    collections::HashMap,
    fmt::{Pointer, Write},
};

use bitflags::Flags;
use bytemuck::Pod;
use log::debug;
use nalgebra::Vector3;

use crate::{
    common::{bitset::Bitset, morton::morton_decode},
    engine::graphics::device::DeviceResource,
};

use super::{
    attachment::{Attachment, AttachmentId, AttachmentMap},
    esvo::{VoxelModelESVO, VoxelModelESVONode},
    voxel::{
        VoxelData, VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelGpuNone, VoxelModelImpl,
        VoxelModelImplConcrete, VoxelModelSchema,
    },
    voxel_allocator::{VoxelAllocator, VoxelDataAllocation},
    voxel_constants,
};

/// A float 1D array representing a 3D voxel region.
#[derive(Clone)]
pub struct VoxelModelFlat {
    pub attachment_data: HashMap<Attachment, Vec<u32>>,
    pub attachment_presence_data: HashMap<Attachment, Bitset>,
    pub attachment_map: AttachmentMap,
    pub presence_data: Bitset,
    length: Vector3<u32>,
    volume: usize,
}

impl VoxelModelFlat {
    pub fn new(
        presence_data: Bitset,
        attachment_data: HashMap<Attachment, Vec<u32>>,
        attachment_presence_data: HashMap<Attachment, Bitset>,
        length: Vector3<u32>,
    ) -> Self {
        let volume = length.product() as usize;

        // Ensure all data fits the expected volume.
        assert_eq!(presence_data.bits(), volume as usize);
        for (attachment, data) in &attachment_data {
            assert_eq!(attachment.size() as usize * volume as usize, data.len());
        }
        for (attachment, data) in &attachment_presence_data {
            assert_eq!(data.bits(), volume as usize);
        }
        Self {
            presence_data,
            attachment_data,
            attachment_presence_data,
            attachment_map: AttachmentMap::new(),
            length,
            volume,
        }
    }

    pub fn new_empty(length: Vector3<u32>) -> Self {
        let volume = length.x * length.y * length.z;
        Self::new(
            Bitset::new(volume as usize),
            HashMap::new(),
            HashMap::new(),
            length,
        )
    }

    pub fn get_voxel_index(&self, position: Vector3<u32>) -> usize {
        position.x as usize
            + position.y as usize * self.length.x as usize
            + position.z as usize * (self.length.x as usize * self.length.y as usize)
    }

    pub fn get_voxel_position(&self, index: usize) -> Vector3<u32> {
        Vector3::new(
            (index % self.length.x as usize) as u32,
            ((index / self.length.x as usize) % self.length.y as usize) as u32,
            (index / (self.length.x as usize * self.length.y as usize)) as u32,
        )
    }

    pub fn get_voxel(&self, position: Vector3<u32>) -> VoxelModelFlatVoxelAccess<'_> {
        let index = self.get_voxel_index(position);
        VoxelModelFlatVoxelAccess {
            flat_model: self,
            index,
        }
    }

    pub fn in_bounds(&self, position: Vector3<u32>) -> bool {
        !(position.x >= self.length.x || position.y >= self.length.y || position.z >= self.length.z)
    }

    pub fn length(&self) -> &Vector3<u32> {
        &self.length
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
            self.length.x, self.length.y, self.length.z
        ));
        if (self.length.x * self.length.y * self.length.z > 256) {
            nodes_str.push_str("Flat model is too big to print out to stdout.\n");
        } else {
            nodes_str.push_str("presence: \n");
            for y in 0..self.length.x {
                nodes_str.push_str(&format!("Y: {}\n", y));
                for z in 0..self.length.y {
                    let mut row = String::new();
                    for x in 0..self.length.z {
                        let voxel = self.get_voxel(Vector3::new(x, y, z));
                        let char = if voxel.is_empty() { '0' } else { '1' };
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
    pub fn is_empty(&self) -> bool {
        !self.flat_model.presence_data.get_bit(self.index)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, &[u32])> {
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
                .contains_key(&attachment)
            {
                self.flat_model
                    .attachment_presence_data
                    .insert(attachment.clone(), Bitset::new(self.flat_model.volume));
                self.flat_model.attachment_data.insert(
                    attachment.clone(),
                    vec![0u32; attachment.size() as usize * self.flat_model.volume()],
                );
            }

            // Mark attachment presence.
            self.flat_model
                .attachment_presence_data
                .get_mut(&attachment)
                .unwrap()
                .set_bit(self.index, true);

            assert_eq!(std::mem::size_of::<T>(), attachment.size() as usize * 4);
            let mut attachment_data = self
                .flat_model
                .attachment_data
                .get_mut(&attachment)
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
                .get_mut(&attachment)
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
enum FlatESVONode {
    // TODO: allow internal nodes to also hold interpolated attachment data if applicable
    // (interpolation of the attachment depending on the type of attachment)
    NonLeaf {
        node_data: u32,
        attachment_lookup_node_data: Option<HashMap<Attachment, u32>>,
    },
    Leaf {
        attachment_data: HashMap<Attachment, Vec<u32>>,
    },
}

impl From<&VoxelModelFlat> for VoxelModelESVO {
    fn from(flat: &VoxelModelFlat) -> Self {
        let length = flat.length().map(|x| x.next_power_of_two()).max().max(2);
        let mut esvo = VoxelModelESVO::empty(length, true);
        esvo.attachment_map = flat.attachment_map.clone();

        // TODO: Then we have super easy flat -> esvo conversion, however it would probably
        // be better to allocate the node directly so we know capacity then write the data
        // for them so that way we don't have to regrow nodes when we already have the known
        // size so it would be "faster", but no premature opimization so not yet :)
        for i in 0..esvo.volume() {
            let pos = morton_decode(i as u64);

            if flat.in_bounds(pos) {
                let flat_voxel = flat.get_voxel(pos);
                if !flat_voxel.is_empty() {
                    let mut esvo_voxel = esvo.get_voxel_mut(pos);
                    let (parent_node_index, leaf_octant) = esvo_voxel.get_or_create_leaf_node();

                    for (attachment, data) in flat_voxel.iter().map(|(attachment_id, data)| {
                        (flat.attachment_map.get_attachment(attachment_id), data)
                    }) {
                        esvo_voxel.set_attachment_data(
                            parent_node_index,
                            leaf_octant,
                            attachment,
                            data,
                        );
                    }
                }
            }
        }

        esvo
    }
}

impl VoxelModelImplConcrete for VoxelModelFlat {
    type Gpu = VoxelModelFlatGpu;
}

impl VoxelModelImpl for VoxelModelFlat {
    fn set_voxel_range_impl(&mut self, range: super::voxel::VoxelRange) {
        todo!()
    }

    fn schema(&self) -> VoxelModelSchema {
        voxel_constants::MODEL_FLAT_SCHEMA
    }

    fn length(&self) -> Vector3<u32> {
        self.length
    }
}

pub struct VoxelModelFlatGpu {
    flat_length: Vector3<u32>,
    voxel_presence_allocation: Option<VoxelDataAllocation>,
    voxel_attachment_presence_allocations: HashMap<AttachmentId, VoxelDataAllocation>,
    voxel_attachment_data_allocations: HashMap<AttachmentId, VoxelDataAllocation>,

    initialized_data: bool,
}

impl VoxelModelGpuImplConcrete for VoxelModelFlatGpu {
    fn new() -> Self {
        VoxelModelFlatGpu {
            flat_length: Vector3::zeros(),
            voxel_presence_allocation: None,
            voxel_attachment_presence_allocations: HashMap::new(),
            voxel_attachment_data_allocations: HashMap::new(),
            initialized_data: false,
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

            attachment_presence_indices[*attachment as usize] = allocation.start_index_stride_u32()
        }
        let mut attachment_data_indices =
            vec![u32::MAX; Attachment::MAX_ATTACHMENT_ID as usize + 1];
        for (attachment, allocation) in &self.voxel_attachment_data_allocations {
            if *attachment > Attachment::MAX_ATTACHMENT_ID {
                continue;
            }

            attachment_data_indices[*attachment as usize] = allocation.start_index_stride_u32();
        }

        let mut info = vec![
            // Flat length
            self.flat_length.x,
            self.flat_length.y,
            self.flat_length.z,
            // World data ptr (divide by 4 since 4 bytes in a u32)
            voxel_presence_allocation.start_index_stride_u32(),
        ];
        info.append(&mut attachment_presence_indices);
        info.append(&mut attachment_data_indices);

        Some(info)
    }

    fn update_gpu_objects(&mut self, allocator: &mut VoxelAllocator, model: &dyn VoxelModelImpl) {
        let model = model.downcast_ref::<VoxelModelFlat>().unwrap();

        if self.voxel_presence_allocation.is_none() {
            let presence_allocation_size = model.presence_data.data().len() * 4;
            self.voxel_presence_allocation = Some(
                allocator
                    .allocate(presence_allocation_size as u64)
                    .expect("Failed to allocate flat voxel presence data."),
            );
        }

        for (attachment_id, presence_bitset) in &model.attachment_presence_data {
            if !self
                .voxel_attachment_presence_allocations
                .contains_key(&attachment_id.id())
            {
                let presence_allocation_size = presence_bitset.data().len() * 4;
                self.voxel_attachment_presence_allocations.insert(
                    attachment_id.id(),
                    allocator
                        .allocate(presence_allocation_size as u64)
                        .expect("Failed to allocate flat attachment presence data."),
                );
            }
        }

        for (attachment_id, attachment_data) in &model.attachment_data {
            if !self
                .voxel_attachment_data_allocations
                .contains_key(&attachment_id.id())
            {
                let attachment_data_allocation_size = attachment_data.len() * 4;
                self.voxel_attachment_data_allocations.insert(
                    attachment_id.id(),
                    allocator
                        .allocate(attachment_data_allocation_size as u64)
                        .expect("Failed to allocate flat attachment data."),
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
        let model = model.downcast_ref::<VoxelModelFlat>().unwrap();

        if !self.initialized_data && self.voxel_presence_allocation.is_some() {
            debug!("Writing Flat voxel model initial data");
            self.flat_length = model.length;

            allocator.write_world_data(
                device,
                self.voxel_presence_allocation.as_ref().unwrap(),
                bytemuck::cast_slice::<u32, u8>(model.presence_data.data()),
            );

            for (attachment, presence_data) in &model.attachment_presence_data {
                let allocation = self
                    .voxel_attachment_presence_allocations
                    .get(&attachment.id())
                    .expect("Voxel attachment presence allocation should've been allocated at this point.");

                allocator.write_world_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(presence_data.data()),
                );
            }

            for (attachment, attachment_data) in &model.attachment_data {
                let allocation = self
                    .voxel_attachment_data_allocations
                    .get(&attachment.id())
                    .expect("Voxel attachment presence allocation should've been allocated at this point.");

                allocator.write_world_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(attachment_data),
                );
            }

            self.initialized_data = true;
            return;
        }
    }
}
