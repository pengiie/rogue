use std::{
    collections::HashMap,
    future::Future,
    io::{Read, Write},
    os::unix::fs::FileExt,
};

use anyhow::{anyhow, bail};
use nalgebra::Vector3;

use crate::{
    common::bitset::Bitset,
    engine::{
        asset::{
            asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver},
            util::{AssetByteReader, AssetByteWriter},
        },
        voxel::{
            attachment::{Attachment, AttachmentId, AttachmentMap},
            flat::VoxelModelFlat,
            thc::{THCAttachmentLookupNode, THCNode, VoxelModelTHC},
            voxel::{VoxelModelImpl, VoxelModelType},
        },
    },
};

pub struct VoxelModelTHCAsset {
    model: VoxelModelTHC,
}

const FILE_VERSION: u32 = 1;

impl AssetLoader for VoxelModelAnyAsset {
    fn load(data: &AssetFile) -> std::result::Result<Self, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader = AssetByteReader::new_unknown(data.read_file()?)?;
        match reader.header() {
            Some("FLAT") => Ok(VoxelModelAnyAsset {
                model: Box::new(load_flat_model(reader)?),
                model_type: VoxelModelType::Flat,
            }),
            Some("THC ") => Ok(VoxelModelAnyAsset {
                model: Box::new(load_thc_model(reader)?),
                model_type: VoxelModelType::THC,
            }),
            _ => Err(anyhow::anyhow!("Unknown header").into()),
        }
    }
}

pub struct VoxelModelAnyAsset {
    pub model: Box<dyn VoxelModelImpl>,
    pub model_type: VoxelModelType,
}

impl AssetLoader for VoxelModelTHC {
    fn load(data: &AssetFile) -> std::result::Result<VoxelModelTHC, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader = AssetByteReader::new(data.read_file()?, "THC ")?;
        return load_thc_model(reader);
    }
}

fn load_thc_model(
    mut reader: AssetByteReader,
) -> std::result::Result<VoxelModelTHC, AssetLoadError> {
    assert!(reader.version() == 1);

    let side_length = reader.read_u32()?;

    // Attachment block.
    let attachment_count = reader.read_u32()?;
    let mut thc = VoxelModelTHC::new_empty(side_length);
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
        thc.attachment_map
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
        node_data.push(THCNode {
            child_ptr,
            child_mask,
        })
    }
    thc.node_data = node_data;

    while let Some(attachment_id) = attachment_presence_map.get(&(reader.cursor_pos()? as u32)) {
        let node_count = reader.read_u32()?;
        let mut attachment_lookup_nodes = Vec::with_capacity(node_count as usize);
        for i in 0..node_count {
            let data_ptr = reader.read_u32()?;
            let attachment_mask_lower = reader.read_u32()?;
            let attachment_mask_upper = reader.read_u32()?;
            let attachment_mask =
                ((attachment_mask_upper as u64) << 32) | attachment_mask_lower as u64;
            attachment_lookup_nodes.push(THCAttachmentLookupNode {
                data_ptr,
                attachment_mask,
            })
        }
        thc.attachment_lookup_data
            .insert(*attachment_id, attachment_lookup_nodes);
    }
    while let Some(attachment_id) = attachment_data_map.get(&(reader.cursor_pos()? as u32)) {
        let data_count = reader.read_u32()?;
        let mut attachment_data = vec![0u32; data_count as usize];
        reader.read_to_slice(&mut attachment_data);
        thc.attachment_raw_data
            .insert(*attachment_id, attachment_data);
    }

    Ok(thc)
}

impl AssetSaver for VoxelModelTHC {
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        let mut writer = AssetByteWriter::new(out_file.write_file(), "THC ", 1);
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

impl AssetSaver for VoxelModelFlat {
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        let mut writer = AssetByteWriter::new(out_file.write_file(), "FLAT", 1);
        writer.write(data.side_length());

        // Write attachment info block.
        writer.write(&(data.attachment_map.iter().count() as u32));
        let mut attachment_positions = AttachmentMap::new();
        for (attachment_id, _) in data.attachment_map.iter() {
            writer.write(&attachment_id);
            let presence_pointer = writer.write_later::<u32>();
            let raw_pointer = writer.write_later::<u32>();
            attachment_positions.insert(attachment_id, (presence_pointer, raw_pointer));
        }

        writer.write_slice(data.presence_data.data());
        for (attachment_id, data) in data.attachment_presence_data.iter() {
            let presence_pointer = writer.cursor_pos() as u32;
            writer.write_at(attachment_positions[attachment_id].0, &presence_pointer);
            writer.write_slice(data.data());
        }
        for (attachment_id, data) in data.attachment_data.iter() {
            let raw_pointer = writer.cursor_pos() as u32;
            writer.write_at(attachment_positions[attachment_id].1, &raw_pointer);
            writer.write_slice(data.as_slice());
        }
        writer.finish_writes()?;

        Ok(())
    }
}

fn load_flat_model(
    mut reader: AssetByteReader,
) -> std::result::Result<VoxelModelFlat, AssetLoadError> {
    assert!(reader.version() == 1);

    let side_length = reader.read::<Vector3<u32>>()?;
    let attachment_count = reader.read::<u32>()?;
    let mut flat = VoxelModelFlat::new_empty(side_length);
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
        flat.attachment_map
            .register_attachment(Attachment::from_id(id));
        attachment_presence_map.insert(presence_pointer, id);
        attachment_data_map.insert(data_pointer, id);
    }
    reader.read_to_slice(flat.presence_data.data_mut());
    while let Some(attachment_id) = attachment_presence_map.get(&(reader.cursor_pos()? as u32)) {
        let mut bitset = Bitset::new(flat.volume());
        reader.read_to_slice(bitset.data_mut());
        flat.attachment_presence_data.insert(*attachment_id, bitset);
    }
    while let Some(attachment_id) = attachment_data_map.get(&(reader.cursor_pos()? as u32)) {
        let attachment = flat.attachment_map.get_unchecked(*attachment_id);
        let mut data = vec![0u32; flat.volume() * attachment.size() as usize];
        reader.read_to_slice(&mut data);
        flat.attachment_data.insert(*attachment_id, data);
    }

    Ok(flat)
}

impl AssetLoader for VoxelModelFlat {
    fn load(data: &AssetFile) -> std::result::Result<VoxelModelFlat, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader = AssetByteReader::new(data.read_file()?, "FLAT")?;
        return load_flat_model(reader);
    }
}
