use std::ops::Deref;

use nalgebra::Vector3;

use crate::{
    common::morton,
    consts,
    engine::{
        asset::{
            asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver},
            util::{AssetByteReader, AssetByteWriter},
        },
        voxel::terrain::chunks::{VoxelChunkRegionData, VoxelChunkRegionNode, VoxelRegionLeafNode},
    },
};

impl AssetLoader for VoxelChunkRegionData {
    fn load(data: &AssetFile) -> std::result::Result<Self, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader = AssetByteReader::new(data.read_file()?, consts::io::REGION_FILE_HEADER)?;
        let region_pos = reader.read::<Vector3<i32>>()?;
        let mut region = VoxelChunkRegionData::empty(region_pos);

        let node_size = reader.read::<u32>()?;
        let mut nodes = vec![0u32; node_size as usize];
        reader.read_to_slice(&mut nodes)?;

        let mut stack = vec![(0u32, 0u32, 0u64)];
        while (!stack.is_empty()) {
            let (next_node_pointer, next_node_height, next_node_morton) = stack.pop().unwrap();
            if next_node_height == consts::voxel::TERRAIN_REGION_TREE_HEIGHT - 1 {
                for i in 0u32..8u32 {
                    let local_chunk_pos = morton::morton_decode((next_node_morton << 3) | i as u64);
                    let chunk_uuid_pointer = (next_node_pointer + i * 4) as usize;
                    let chunk_uuid = uuid::Uuid::from_slice(bytemuck::cast_slice::<u32, u8>(
                        &nodes[chunk_uuid_pointer..(chunk_uuid_pointer + 4)],
                    ))
                    .unwrap();
                    // Node is initialized to Empty so we don't have to worry about when uuid is
                    // nil.
                    let node = region.get_or_create_chunk_mut(
                        &(region_pos * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32
                            + local_chunk_pos.cast::<i32>()),
                    );
                    if !chunk_uuid.is_nil() {
                        *node = VoxelRegionLeafNode::Existing {
                            uuid: chunk_uuid,
                            model: None,
                        };
                    }
                }
            } else {
                for i in 0..8 {
                    let child_pointer = nodes[next_node_pointer as usize + i];
                    if child_pointer != 0 {
                        stack.push((
                            child_pointer,
                            next_node_height + 1,
                            (next_node_morton << 3) | i as u64,
                        ))
                    }
                }
            }
        }

        Ok(region)
    }
}

impl AssetSaver for VoxelChunkRegionData {
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        let mut writer =
            AssetByteWriter::new(out_file.write_file(), consts::io::REGION_FILE_HEADER, 1);
        writer.write::<Vector3<i32>>(&data.region_pos);
        let mut bytes: Vec<u32> = Vec::new();

        // Add the nodes to the to the tree array with depth-first traversal.
        let mut curr_node = &data.root_node;
        let mut stack = vec![(curr_node, 0, None)];
        let curr_height = 0;
        while (!stack.is_empty()) {
            let curr_node_index = stack.len() - 1;
            let (next_node, curr_iter, node_pos) = stack.last_mut().unwrap();
            // Allocate memory for this node and track the location.
            if node_pos.is_none() {
                let req_size = match (*next_node).deref() {
                    VoxelChunkRegionNode::Internal(_) => 8,
                    VoxelChunkRegionNode::Preleaf(_) => 4 * 8,
                };
                *node_pos = Some(bytes.len() as u32);
                bytes.extend_from_slice(&vec![0u32; req_size])
            }
            log::info!("node_pos {:?}, curr_iter {}", node_pos, curr_iter);

            if *curr_iter >= 8 {
                stack.pop();
                continue;
            }

            'b_point: {
                match (*next_node).deref() {
                    VoxelChunkRegionNode::Internal(children_nodes) => {
                        let curr_child = &children_nodes[*curr_iter];
                        if let Some(curr_child) = curr_child {
                            bytes[node_pos.unwrap() as usize + *curr_iter] = bytes.len() as u32;
                            stack.push((curr_child, 0, None));
                        }
                        break 'b_point;
                    }
                    VoxelChunkRegionNode::Preleaf(leaf_chunks) => {
                        'chunk_leaf_loop: for (leaf_i, leaf) in leaf_chunks.iter().enumerate() {
                            let leaf = match leaf {
                                VoxelRegionLeafNode::Empty => continue 'chunk_leaf_loop,
                                VoxelRegionLeafNode::Existing { uuid, .. } => uuid,
                            };
                            let min = node_pos.unwrap() as usize + leaf_i * 4;
                            bytes[min..(min + 4)]
                                .copy_from_slice(bytemuck::cast_slice::<u8, u32>(leaf.as_bytes()));
                        }
                        *curr_iter = 8;
                    }
                }
            }

            let (next_node, curr_iter, node_pos) = &mut stack[curr_node_index];
            *curr_iter += 1;
        }
        writer.write::<u32>(&(bytes.len() as u32));
        writer.write_slice(bytes.as_slice());
        writer.finish_writes()?;

        Ok(())
    }
}
