use std::{
    collections::HashMap,
    fmt::{Pointer, Write},
};

use bitflags::Flags;
use bytemuck::Pod;
use log::debug;
use nalgebra::Vector3;

use crate::common::{bitset::Bitset, morton::morton_decode};

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
            + position.z as usize * (self.length.x as usize * self.length.y as usize)
    }

    pub fn get_voxel_position(&self, index: usize) -> Vector3<u32> {
        Vector3::new(
            (index % self.length.x as usize) as u32,
            ((index / self.length.x as usize) % self.length.y as usize) as u32,
            (index / (self.length.x as usize * self.length.y as usize)) as u32,
        )
    }

    pub fn get_voxel(&self, index: usize) -> VoxelModelFlatVoxelAccess<'_> {
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
}

impl std::fmt::Debug for VoxelModelFlat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut nodes_str = String::new();
        for y in 0..self.length.x {
            nodes_str.push_str(&format!("Y: {}\n", y));
            for z in 0..self.length.y {
                let mut row = String::new();
                for x in 0..self.length.z {
                    let voxel = self.get_voxel(self.get_voxel_index(Vector3::new(x, y, z)));
                    let char = if voxel.is_empty() { '0' } else { '1' };
                    row.push(char);
                    row.push(' ');
                }
                row.push_str("\n\n");
                nodes_str.push_str(&row);
            }
        }

        f.write_fmt(format_args!("Voxel Flat:\nNodes:\n{}", nodes_str))
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

impl From<VoxelModelFlat> for VoxelModelESVO {
    fn from(flat: VoxelModelFlat) -> Self {
        let length = flat.length().map(|x| x.next_power_of_two()).max().max(2);
        let volume = length.pow(3);
        let height = length.trailing_zeros();
        let mut esvo_nodes = Vec::new();

        let mut levels: Vec<Vec<Option<u32>>> = vec![vec![]; height as usize + 1];

        for i in 0..volume {
            let pos = morton_decode(i as u64);
            let mut exists = false;

            if flat.in_bounds(pos) {
                let voxel = flat.get_voxel(flat.get_voxel_index(pos));
                exists = !voxel.is_empty();
            }
            levels
                .last_mut()
                .unwrap()
                .push(if exists { Some(0) } else { None });

            // We need to pop our octant, which may need to pop up the chain so we iterate over
            // each level bottom up.
            if levels.last().unwrap().len() == 8 {
                for h in (1..=height).rev() {
                    let nodes = &levels[h as usize];
                    // Pop.
                    if nodes.len() == 8 {
                        let mut children_nodes = Vec::new();
                        let mut child_ptr = 0;
                        let mut child_mask = 0;
                        for (octant, node) in nodes.iter().enumerate() {
                            if node.is_some() {
                                child_mask |= 1 << octant;
                                children_nodes.push(node.clone());
                                if h < height {
                                    child_ptr = esvo_nodes.len();
                                    let n = VoxelModelESVO::decode_node(node.unwrap());
                                    let updated_child_ptr = if h + 1 < height - 1 {
                                        esvo_nodes.len() - n.0 as usize
                                    } else {
                                        n.0 as usize
                                    };
                                    esvo_nodes.push(VoxelModelESVO::encode_node(
                                        updated_child_ptr as u32,
                                        n.1,
                                        n.2,
                                        n.3,
                                    ));
                                }
                            }
                        }

                        let leaf_mask = if h == height { child_mask } else { 0 };
                        let node = if child_mask > 0 {
                            Some(VoxelModelESVO::encode_node(
                                child_ptr as u32,
                                false,
                                child_mask,
                                leaf_mask,
                            ))
                        } else {
                            None
                        };
                        levels[h as usize - 1].push(node);

                        levels[h as usize].clear();
                    }
                }
            }
        }

        esvo_nodes.push(levels.first().unwrap().first().unwrap().map_or(0, |node| {
            let n = VoxelModelESVO::decode_node(node);
            let updated_child_ptr = if height > 1 {
                esvo_nodes.len() - n.0 as usize
            } else {
                n.0 as usize
            };
            VoxelModelESVO::encode_node(updated_child_ptr as u32, n.1, n.2, n.3)
        }));

        // Reverse so the root node is first, child_ptrs can stay the same.
        esvo_nodes.reverse();
        // for (i, node) in esvo_nodes.iter().enumerate() {
        //     let (child_ptr, far, value_mask, leaf_mask) = VoxelModelESVO::decode_node(*node);
        //     let value_mask_str = (0..8).fold(String::new(), |mut str, octant| {
        //         str.push_str(if (value_mask & (1 << octant)) > 0 {
        //             "1"
        //         } else {
        //             "0"
        //         });

        //         str
        //     });
        //     let leaf_mask_str = (0..8).fold(String::new(), |mut str, octant| {
        //         str.push_str(if (leaf_mask & (1 << octant)) > 0 {
        //             "1"
        //         } else {
        //             "0"
        //         });

        //         str
        //     });
        //     debug!(
        //         "[{}] Child ptr: {}, Far: {}, Value Mask: {}, Leaf Mask: {}",
        //         i, child_ptr, far, value_mask_str, leaf_mask_str
        //     );
        // }

        VoxelModelESVO::with_nodes(esvo_nodes, length, false)
    }
}
