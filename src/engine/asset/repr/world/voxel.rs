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
            asset::AssetLoadError,
            util::{AssetByteReader, AssetByteWriter},
        },
        voxel::{
            attachment::{Attachment, AttachmentId, AttachmentMap},
            flat::VoxelModelFlat,
            thc::VoxelModelTHC,
        },
    },
};

use super::super::asset::{AssetFile, AssetLoader, AssetSaver};

pub struct VoxelModelTHCAsset {
    model: VoxelModelTHC,
}

const FILE_VERSION: u32 = 1;

impl AssetLoader for VoxelModelTHC {
    fn load(data: &AssetFile) -> std::result::Result<VoxelModelTHC, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut file = data.read_file();
        let mut header = 0x0;
        let Ok(n) = file.read(bytemuck::bytes_of_mut(&mut header)) else {
            return Err(AssetLoadError::Other(anyhow!("Failed to read file bytes.")));
        };
        if (n != 4 || header != 0x56544843/*VTHC*/) {
            return Err(AssetLoadError::Other(anyhow!("Expected file header VTHC.")));
        }

        Ok(VoxelModelTHC::new(16))
    }
}

impl AssetSaver for VoxelModelTHC {
    fn save(model: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        // 4 byte header, 4 byte version, 4 byte for side_length.
        const HEADER_BYTE_SIZE: u32 = 4 + 4 + 4;
        let mut req_bytes = HEADER_BYTE_SIZE as usize;

        // 4 byte for attachment_map_size.
        req_bytes += 4;
        for (attachment_id, attachment) in model.attachment_map.iter() {
            // TODO: attachment name so we can confirm on write with versioning.
            // 1 byte attachment id, 4 byte attachment data pointer.
            req_bytes += 5;
        }

        // 4 byte for node_data size.
        req_bytes += 4;
        req_bytes += model.node_data.len() * /*size of node*/12;

        for (_attachment_id, lookup_data) in model.attachment_lookup_data.iter() {
            // 4 byte for lookup_data size.
            req_bytes += 4;
            req_bytes += lookup_data.len() * /*size of lookup node*/12;
        }

        for (attachment_id, raw_data) in model.attachment_raw_data.iter() {
            // 4 byte for raw_data size.
            req_bytes += 4;
            req_bytes += raw_data.len() * 4;
        }

        let mut file = out_file.write_file();
        file.set_len(req_bytes as u64);

        assert_eq!(
            file.write_at(
                bytemuck::bytes_of(&[0x56544843u32, FILE_VERSION, model.length]),
                0,
            )?,
            HEADER_BYTE_SIZE as usize
        );

        let written_attachments_size = model.attachment_map.iter().count() as u32;
        let written_attachments_byte_size = 4 + written_attachments_size * 9;
        assert_eq!(
            file.write_at(
                bytemuck::bytes_of(&[written_attachments_size]),
                HEADER_BYTE_SIZE as u64
            )?,
            4
        );

        assert_eq!(
            file.write_at(
                bytemuck::bytes_of(&[model.node_data.len() as u32]),
                HEADER_BYTE_SIZE as u64 + written_attachments_byte_size as u64
            )?,
            4
        );
        let mut bytes = vec![0u32; model.node_data.len() * 3];
        for (i, node) in model.node_data.iter().enumerate() {
            let offset = i * 3;
            bytes[offset] = node.child_ptr;
            bytes[offset + 1] = (node.child_mask >> 32) as u32;
            bytes[offset + 2] = node.child_mask as u32;
        }
        assert_eq!(
            file.write_at(
                bytemuck::cast_slice::<u32, u8>(bytes.as_slice()),
                HEADER_BYTE_SIZE as u64 + written_attachments_byte_size as u64 + 4
            )?,
            bytes.len() * 4
        );

        let mut written_so_far_bytes = HEADER_BYTE_SIZE as u64
            + written_attachments_byte_size as u64
            + 4
            + model.node_data.len() as u64 * 12;
        let mut written_attachments: HashMap<AttachmentId, (u32, u32)> = HashMap::new();
        for (attachment_id, lookup_data) in model.attachment_lookup_data.iter() {
            written_attachments.insert(*attachment_id, (written_so_far_bytes as u32, 0));
            assert_eq!(
                file.write_at(
                    bytemuck::bytes_of(&(lookup_data.len() as u32)),
                    written_so_far_bytes
                )?,
                4
            );
            written_so_far_bytes += 4;

            let mut bytes = vec![0u32; lookup_data.len() * 3];
            for (i, lookup_node) in lookup_data.iter().enumerate() {
                let offset = i * 3;
                bytes[offset] = lookup_node.data_ptr;
                bytes[offset + 1] = (lookup_node.attachment_mask >> 32) as u32;
                bytes[offset + 2] = lookup_node.attachment_mask as u32;
            }
            assert_eq!(
                file.write_at(
                    bytemuck::cast_slice::<u32, u8>(bytes.as_slice()),
                    written_so_far_bytes
                )?,
                bytes.len() * 4
            );
            written_so_far_bytes += bytes.len() as u64 * 4;
        }

        for (attachment_id, raw_data) in model.attachment_raw_data.iter() {
            written_attachments.get_mut(attachment_id).unwrap().1 = written_so_far_bytes as u32;
            assert_eq!(
                file.write_at(
                    bytemuck::bytes_of(&(raw_data.len() as u32)),
                    written_so_far_bytes
                )?,
                4
            );
            written_so_far_bytes += 4;

            assert_eq!(
                file.write_at(
                    bytemuck::cast_slice::<u32, u8>(&raw_data),
                    written_so_far_bytes
                )?,
                raw_data.len() * 4
            );
            written_so_far_bytes += raw_data.len() as u64 * 4;
        }

        for (i, (attachment_id, (attachment_lookup_ptr, attachment_data_ptr))) in
            written_attachments.iter().enumerate()
        {
            let mut bytes = [0u8; 9];
            bytes[0] = *attachment_id;
            bytes[1..5].copy_from_slice(bytemuck::bytes_of(attachment_lookup_ptr));
            bytes[5..9].copy_from_slice(bytemuck::bytes_of(attachment_data_ptr));
            file.write_at(&bytes, HEADER_BYTE_SIZE as u64 + 5 * i as u64);
        }

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
        writer.write(&(data.attachment_map.iter().count() as u32));
        let mut attachment_positions = HashMap::new();
        for (attachment_id, _) in data.attachment_map.iter() {
            writer.write(attachment_id);
            let presence_pointer = writer.write_later::<u32>();
            let raw_pointer = writer.write_later::<u32>();
            attachment_positions.insert(*attachment_id, (presence_pointer, raw_pointer));
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

impl AssetLoader for VoxelModelFlat {
    fn load(data: &AssetFile) -> std::result::Result<VoxelModelFlat, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader = AssetByteReader::new(data.read_file(), "FLAT")?;
        assert!(reader.version() == 1);

        let side_length = reader.read::<Vector3<u32>>()?;
        let attachment_count = reader.read::<u32>()?;
        let mut flat = VoxelModelFlat::new_empty(side_length);
        let mut attachment_presence_map: HashMap<
            /*presence_pointer*/ u32,
            /*attachment_id*/ u8,
        > = HashMap::new();
        let mut attachment_data_map: HashMap<
            /*presence_pointer*/ u32,
            /*attachment_id*/ u8,
        > = HashMap::new();
        for i in 0..attachment_count {
            let id = reader.read::<u8>()?;
            let presence_pointer = reader.read::<u32>()?;
            let data_pointer = reader.read::<u32>()?;
            flat.attachment_map
                .register_attachment(&Attachment::from_id(id));
            attachment_presence_map.insert(presence_pointer, id);
            attachment_data_map.insert(data_pointer, id);
        }
        reader.read_to_slice(flat.presence_data.data_mut());
        while let Some(attachment_id) = attachment_presence_map.get(&(reader.cursor_pos()? as u32))
        {
            let mut bitset = Bitset::new(flat.volume());
            reader.read_to_slice(bitset.data_mut());
            flat.attachment_presence_data.insert(*attachment_id, bitset);
        }
        while let Some(attachment_id) = attachment_data_map.get(&(reader.cursor_pos()? as u32)) {
            let attachment = flat.attachment_map.get_attachment(*attachment_id);
            let mut data = vec![0u32; flat.volume() * attachment.size() as usize];
            reader.read_to_slice(&mut data);
            flat.attachment_data.insert(*attachment_id, data);
        }

        Ok(flat)
    }
}
