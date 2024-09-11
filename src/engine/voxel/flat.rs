use std::collections::HashMap;

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

    fn get_node_data(&self) -> &[u8] {
        todo!()
    }

    fn get_attachment_lookup_data(&self) -> &[u8] {
        todo!()
    }

    fn get_attachments_data(&self) -> std::collections::HashMap<super::voxel::Attributes, &[u8]> {
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
        let volume = length.x * length.y * length.z;
        let filled_attrs = attributes
            .into_iter()
            .map(|(attr, value)| (attr, vec![value; volume as usize]))
            .collect::<HashMap<_, _>>();

        Self::new(filled_attrs, length)
    }

    pub fn length(&self) -> &Vector3<u32> {
        &self.length
    }
}
