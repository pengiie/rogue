use core::panic;
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
    attachment::Attachment,
    esvo::{VoxelModelESVO, VoxelModelESVONode},
    voxel::{VoxelData, VoxelModelGpuNone, VoxelModelImpl},
};

/// A float 1D array representing a 3D voxel region.
pub struct VoxelModelFlat {
    pub attachment_data: HashMap<Attachment, Vec<u32>>,
    pub attachment_presence_data: HashMap<Attachment, Bitset>,
    pub presence_data: Bitset,
    length: Vector3<u32>,
    volume: usize,
}

impl VoxelModelImpl for VoxelModelFlat {
    fn set_voxel_range_impl(&mut self, range: super::voxel::VoxelRange) {
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
        for y in 0..self.length.x {
            nodes_str.push_str(&format!("Y: {}\n", y));
            for z in 0..self.length.y {
                let mut row = String::new();
                for x in 0..self.length.z {
                    let voxel = self.get_voxel(Vector3::new(x, y, z));
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

    pub fn get_attachment_data(&self) -> HashMap<Attachment, &'a [u32]> {
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
                Some((attachment.clone(), data))
            })
            .collect::<HashMap<_, _>>()
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
        let volume = length.pow(3);
        let height = length.trailing_zeros();
        let mut esvo_nodes = Vec::new();
        let mut esvo_attachment_raw: HashMap<Attachment, Vec<u32>> = HashMap::new();

        // levels(vec) -> octants(vec) -> exists?(option) ->
        //   (node data, has_attachments?(option) -> (attachment, child_octants_attachment_data)(hashmap))
        let mut levels: Vec<Vec<Option<FlatESVONode>>> = vec![vec![]; height as usize + 1];

        for i in 0..volume {
            let pos = morton_decode(i as u64);
            let mut exists = false;
            let mut attachment_data = None;

            if flat.in_bounds(pos) {
                let voxel = flat.get_voxel(pos);
                exists = !voxel.is_empty();
                attachment_data = Some(voxel.get_attachment_data());
            }
            // let mut attachment_data_index = if let Some(attachment_data) = attachment_data {
            //     attachment_data.into_iter().map(|(attachment, data)| {
            //         let access = esvo_attachment_raw.entry(attachment.clone()).or_insert_with(Vec::new());
            //         let raw_index = access.len();

            //         (attachment,)
            //     }).collect::<HashMap<_, _>>()
            // } else {None};
            // Do a copy of the attachment data.
            let attachment_data = attachment_data.map(|attachment_data| {
                attachment_data
                    .into_iter()
                    .map(|(attachment, data_ref)| {
                        (
                            attachment,
                            data_ref.into_iter().map(|x| *x).collect::<Vec<_>>(),
                        )
                    })
                    .collect::<HashMap<_, _>>()
            });
            levels.last_mut().unwrap().push(if exists {
                Some(FlatESVONode::Leaf {
                    attachment_data: attachment_data
                        .expect("Expect some sort of attachment data if the voxel exists."),
                })
            } else {
                None
            });

            // We need to pop our octant, which may need to pop up the chain so we iterate over
            // each level bottom up.
            if levels.last().unwrap().len() == 8 {
                for h in (1..=height).rev() {
                    let nodes = &mut levels[h as usize];
                    // Pop.
                    if nodes.len() == 8 {
                        let mut child_mask = 0u32;
                        let mut leaf_mask = 0u32;
                        let mut raw_attachment_indices = HashMap::new();
                        let mut to_add_esvo_nodes = Vec::new();
                        let mut children_start = esvo_nodes.len();
                        for (octant, node) in nodes.drain(..).enumerate() {
                            if let Some(node) = node {
                                let octant_bit = 1 << octant;
                                child_mask |= octant_bit;
                                match node {
                                    FlatESVONode::Leaf { attachment_data } => {
                                        leaf_mask |= octant_bit;

                                        // Write attachment data and store the pointers for the
                                        // lookup entry.
                                        for (attachment, mut value) in attachment_data {
                                            let attachment_raw_ref = esvo_attachment_raw
                                                .entry(attachment.clone())
                                                .or_default();
                                            let lookup_entry = raw_attachment_indices
                                                .entry(attachment)
                                                .or_insert_with(|| {
                                                    (attachment_raw_ref.len() as u32, 0)
                                                });
                                            attachment_raw_ref.append(&mut value);
                                            lookup_entry.1 |= octant_bit;
                                        }
                                    }
                                    FlatESVONode::NonLeaf {
                                        node_data,
                                        attachment_lookup_node_data,
                                    } => {
                                        to_add_esvo_nodes
                                            .push((node_data, attachment_lookup_node_data));
                                    }
                                }
                            }
                        }

                        // Add the esvo child nodes in reverse order since we reverse the whole
                        // list at the end we need our child offsets to be relative with an offset
                        // of 0 closest to the parent.
                        to_add_esvo_nodes.reverse();
                        for (i, (node_data, attachment_lookup_node_data)) in
                            to_add_esvo_nodes.into_iter().enumerate()
                        {
                            let n = VoxelModelESVONode(node_data);
                            // The 3rd to last layer and above will have nodes that use
                            // a child pointer to reference children nodes. This is a
                            // relative pointer however so we must change that here,
                            // the leaf data nodes (2nd to last layer) have child
                            // pointers that don't point to anything.
                            // TODO: Implement far pointers for special cases.
                            let updated_child_ptr = if h < height - 1 {
                                children_start - n.relative_ptr() as usize + 1 + i
                            } else {
                                n.relative_ptr() as usize
                            };

                            esvo_nodes.push((
                                VoxelModelESVONode::encode_node(
                                    updated_child_ptr as u32,
                                    n.is_far(),
                                    n.valid_mask(),
                                    n.leaf_mask(),
                                ),
                                attachment_lookup_node_data,
                            ));
                        }

                        let leaf_mask = if h == height { child_mask } else { 0 };
                        let node = if child_mask > 0 {
                            let child_ptr = if h == height {
                                0u32
                            } else {
                                esvo_nodes.len() as u32
                            };
                            let node_data = VoxelModelESVONode::encode_node(
                                child_ptr, false, child_mask, leaf_mask,
                            )
                            .0;

                            let attachment_lookup_node_data = if raw_attachment_indices.is_empty() {
                                None
                            } else {
                                Some(
                                    raw_attachment_indices
                                        .into_iter()
                                        .map(|(attachment, (index, mask))| {
                                            (
                                                attachment,
                                                VoxelModelESVO::encode_attachment_lookup(
                                                    index, mask,
                                                ),
                                            )
                                        })
                                        .collect::<HashMap<_, _>>(),
                                )
                            };
                            Some(FlatESVONode::NonLeaf {
                                node_data,
                                attachment_lookup_node_data,
                            })
                        } else {
                            None
                        };

                        levels[h as usize - 1].push(node);
                    }
                }
            }
        }

        esvo_nodes.push(levels.first().unwrap().first().unwrap().clone().map_or(
            (VoxelModelESVONode(0), None),
            |node| match node {
                FlatESVONode::NonLeaf {
                    node_data,
                    attachment_lookup_node_data,
                } => {
                    let n = VoxelModelESVONode(node_data);
                    let updated_child_ptr = if height > 1 {
                        esvo_nodes.len() - n.relative_ptr() as usize + 1
                    } else {
                        n.relative_ptr() as usize
                    };

                    (
                        VoxelModelESVONode::encode_node(
                            updated_child_ptr as u32,
                            n.is_far(),
                            n.valid_mask(),
                            n.leaf_mask(),
                        ),
                        None,
                    )
                }
                FlatESVONode::Leaf { attachment_data } => {
                    panic!("Should have leaf node as the root.")
                }
            },
        ));

        // Reverse so the root node is first, child_ptrs can stay the same.
        esvo_nodes.reverse();

        todo!("Append the nodes to the esvo, and change the esvo_nodes so that there is not a hashmap per node but separate it out simiar and hold a hashmap of <Attachment, Vec<u32>> and then in VoxelModelESVO create a new function which takes the node data as essentially the same as how the esvo stores it but will append it in a way that generates buckets and page headers");
        //VoxelModelESVO::with_nodes(esvo_nodes, esvo_attachment_raw, length, false)
    }
}
