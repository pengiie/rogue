use std::collections::HashMap;

use bitflags::Flags;
use nalgebra::Vector3;

use super::voxel::{Attributes, VoxelModelImpl};

/// A float 1D array representing a 3D voxel region.
pub struct VoxelModelFlat {
    pub attributes: HashMap<Attributes, Vec<u32>>,
    length: Vector3<u32>,
}

impl VoxelModelImpl for VoxelModelFlat {
    fn set_voxel_range(&mut self, range: super::voxel::VoxelRange) {
        todo!()
    }

    fn schema(&self) -> super::voxel::VoxelModelSchema {
        todo!()
    }
}

impl VoxelModelFlat {
    pub fn new(attributes: HashMap<Attributes, Vec<u32>>, length: Vector3<u32>) -> Self {
        let volume = length.x * length.y * length.z;
        for (_attr, data) in &attributes {
            assert_eq!(data.len(), volume as usize);
        }
        Self { attributes, length }
    }

    pub fn new_filled(attributes: HashMap<Attributes, u32>, length: Vector3<u32>) -> Self {
        assert!(attributes.len() <= 8);
        let volume = length.x * length.y * length.z;
        let filled_attrs: HashMap<Attributes, Vec<u32>> = attributes
            .iter()
            .map(|(attr, value)| (*attr, vec![*value; volume as usize]))
            .collect();

        Self::new(filled_attrs, length)
    }

    pub fn get_voxel_index(&self, position: Vector3<u32>) -> u32 {
        position.x + position.y * self.length.x + position.z * self.length.z
    }

    pub fn length(&self) -> &Vector3<u32> {
        &self.length
    }
}
