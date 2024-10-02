use std::collections::HashMap;

use bitflags::Flags;
use bytemuck::Pod;
use nalgebra::Vector3;

use crate::common::bitset::Bitset;

use super::{
    esvo::VoxelModelESVO,
    voxel::{Attachment, VoxelModelGpuNone, VoxelModelImpl},
};

/// A float 1D array representing a 3D voxel region.
pub struct VoxelModelFlat {
    pub attachment_data: HashMap<Attachment, Vec<u32>>,
    pub presence_data: Bitset,
    length: Vector3<u32>,
    volume: usize,
}

impl VoxelModelImpl for VoxelModelFlat {
    fn set_voxel_range(&mut self, range: super::voxel::VoxelRange) {
        todo!()
    }

    fn schema(&self) -> super::voxel::VoxelModelSchema {
        todo!()
    }

    fn length(&self) -> Vector3<u32> {
        todo!()
    }
}

impl VoxelModelFlat {
    pub fn new(
        presence_data: Bitset,
        attachment_data: HashMap<Attachment, Vec<u32>>,
        length: Vector3<u32>,
    ) -> Self {
        let volume = length.product() as usize;

        // Ensure all data fits the expected volume.
        assert_eq!(presence_data.bits(), volume as usize);
        for (attachment, data) in &attachment_data {
            assert_eq!(attachment.size() as usize * volume as usize, data.len());
        }
        Self {
            presence_data,
            attachment_data,
            length,
            volume,
        }
    }

    pub fn new_empty(length: Vector3<u32>) -> Self {
        let volume = length.x * length.y * length.z;
        Self::new(Bitset::new(volume as usize), HashMap::new(), length)
    }

    pub fn get_voxel_index(&self, position: Vector3<u32>) -> usize {
        position.x as usize
            + position.y as usize * self.length.x as usize
            + position.z as usize * self.length.z as usize
    }

    pub fn get_voxel_position(&self, index: usize) -> Vector3<u32> {
        Vector3::new(
            (index % self.length.x as usize) as u32,
            ((index / self.length.x as usize) % self.length.y as usize) as u32,
            (index / (self.length.x as usize * self.length.y as usize)) as u32,
        )
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

pub struct VoxelModelFlatVoxelAccessMut<'a> {
    pub flat_model: &'a mut VoxelModelFlat,
    pub index: usize,
}

impl<'a> VoxelModelFlatVoxelAccessMut<'a> {
    pub fn set_attachment<T: Pod>(&mut self, attachment: Attachment, value: T) {
        self.flat_model.presence_data.set_bit(self.index, true);
        if !self.flat_model.attachment_data.contains_key(&attachment) {
            self.flat_model.attachment_data.insert(
                attachment.clone(),
                vec![0u32; attachment.size() as usize * self.flat_model.volume()],
            );
        }

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
    }
}

impl Into<VoxelModelESVO> for VoxelModelFlat {
    fn into(self) -> VoxelModelESVO {
        VoxelModelESVO {
            length: todo!(),
            data: todo!(),
            bucket_lookup: todo!(),
            updates: todo!(),
        }
    }
}
