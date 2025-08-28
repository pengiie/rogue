use std::collections::HashMap;

use crate::{
    consts,
    engine::{
        asset::{
            asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver},
            util::{AssetByteReader, AssetByteWriter},
        },
        voxel::{
            attachment::{Attachment, AttachmentMap},
            sft_compressed::{
                SFTAttachmentLookupNodeCompressed, SFTNodeCompressed, VoxelModelSFTCompressed,
            },
            thc::{THCAttachmentLookupNodeCompressed, THCNodeCompressed, VoxelModelTHCCompressed},
        },
    },
};

const FILE_VERSION: u32 = 1;

impl AssetLoader for VoxelModelSFTCompressed {
    fn load(data: &AssetFile) -> std::result::Result<VoxelModelSFTCompressed, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader = AssetByteReader::new(data.read_file()?, consts::io::header::SFT)?;
        return load_sft_model(reader);
    }
}

pub fn load_sft_model(
    mut reader: AssetByteReader,
) -> std::result::Result<VoxelModelSFTCompressed, AssetLoadError> {
    assert!(reader.version() == 1);

    let side_length = reader.read_u32()?;

    // Attachment block.
    let attachment_count = reader.read_u32()?;
    let mut sft = VoxelModelSFTCompressed::new_empty(side_length);
    let mut attachment_presence_map: HashMap<
        /*presence_pointer*/ u32,
        /*attachment_id*/ u8,
    > = HashMap::new();
    let mut attachment_data_map: HashMap</*presence_pointer*/ u32, /*attachment_id*/ u8> =
        HashMap::new();
    for i in 0..attachment_count {
        let id = reader.read::<u8>()?;
        let presence_pointer = reader.read::<u32>()?;
        let data_pointer = reader.read::<u32>()?;
        sft.attachment_map
            .register_attachment(Attachment::from_id(id));
        attachment_presence_map.insert(presence_pointer, id);
        attachment_data_map.insert(data_pointer, id);
    }

    let node_count = reader.read_u32()?;
    let mut node_data = Vec::with_capacity(node_count as usize);
    for i in 0..node_count {
        let child_ptr = reader.read_u32()?;
        let child_mask_lower = reader.read_u32()?;
        let child_mask_upper = reader.read_u32()?;
        let child_mask = ((child_mask_upper as u64) << 32) | child_mask_lower as u64;
        let leaf_mask_lower = reader.read_u32()?;
        let leaf_mask_upper = reader.read_u32()?;
        let leaf_mask = ((leaf_mask_upper as u64) << 32) | leaf_mask_lower as u64;
        node_data.push(SFTNodeCompressed {
            child_ptr,
            child_mask,
            leaf_mask,
        })
    }
    sft.node_data = node_data;

    while let Some(attachment_id) = attachment_presence_map.get(&(reader.cursor_pos()? as u32)) {
        let node_count = reader.read_u32()?;
        let mut attachment_lookup_nodes = Vec::with_capacity(node_count as usize);
        for i in 0..node_count {
            let data_ptr = reader.read_u32()?;
            let attachment_mask_lower = reader.read_u32()?;
            let attachment_mask_upper = reader.read_u32()?;
            let attachment_mask =
                ((attachment_mask_upper as u64) << 32) | attachment_mask_lower as u64;
            attachment_lookup_nodes.push(SFTAttachmentLookupNodeCompressed {
                data_ptr,
                attachment_mask,
            })
        }
        sft.attachment_lookup_data
            .insert(*attachment_id, attachment_lookup_nodes);
    }
    while let Some(attachment_id) = attachment_data_map.get(&(reader.cursor_pos()? as u32)) {
        let data_count = reader.read_u32()?;
        let mut attachment_data = vec![0u32; data_count as usize];
        reader.read_to_slice(&mut attachment_data);
        sft.attachment_raw_data
            .insert(*attachment_id, attachment_data);
    }

    Ok(sft)
}

impl AssetSaver for VoxelModelSFTCompressed {
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        let mut writer = AssetByteWriter::new(out_file.write_file(), consts::io::header::SFT, 1);
        writer.write_u32(data.side_length);

        // Write attachment info block.
        writer.write_u32(data.attachment_map.iter().count() as u32);
        let mut attachment_positions = AttachmentMap::new();
        for (attachment_id, _) in data.attachment_map.iter() {
            writer.write(&attachment_id);
            let presence_pointer = writer.write_later::<u32>();
            let raw_pointer = writer.write_later::<u32>();
            attachment_positions.insert(attachment_id, (presence_pointer, raw_pointer));
        }

        // Write node data.
        writer.write_u32(data.node_data.len() as u32);
        for node in &data.node_data {
            writer.write_u32(node.child_ptr);
            writer.write_u32(node.child_mask as u32);
            writer.write_u32((node.child_mask >> 32) as u32);
            writer.write_u32(node.leaf_mask as u32);
            writer.write_u32((node.leaf_mask >> 32) as u32);
        }

        // Write lookup attachment nodes.
        for (attachment_id, data) in data.attachment_lookup_data.iter() {
            let presence_pointer = writer.cursor_pos() as u32;
            writer.write_at(attachment_positions[attachment_id].0, &presence_pointer);
            writer.write_u32(data.len() as u32);
            for node in data {
                writer.write_u32(node.data_ptr);
                writer.write_u32(node.attachment_mask as u32);
                writer.write_u32((node.attachment_mask >> 32) as u32);
            }
        }

        // Write raw attachment data.
        for (attachment_id, data) in data.attachment_raw_data.iter() {
            let raw_pointer = writer.cursor_pos() as u32;
            writer.write_at(attachment_positions[attachment_id].1, &raw_pointer);
            writer.write_u32(data.len() as u32);
            writer.write_slice(&data);
        }
        writer.finish_writes()?;

        Ok(())
    }
}
