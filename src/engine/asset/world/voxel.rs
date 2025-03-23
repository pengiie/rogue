use std::{collections::HashMap, future::Future, io::Read, os::unix::fs::FileExt};

use anyhow::{anyhow, bail};

use crate::engine::voxel::{attachment::AttachmentId, thc::VoxelModelTHC};

use super::super::asset::{AssetFile, AssetLoader, AssetSaver};

pub struct VoxelModelTHCAsset {
    model: VoxelModelTHC,
}

const FILE_VERSION: u32 = 1;

impl AssetLoader for VoxelModelTHC {
    fn load(data: &AssetFile) -> anyhow::Result<Self>
    where
        Self: Sized + std::any::Any,
    {
        let mut file = data.read_file();
        let mut header = 0x0;
        let Ok(n) = file.read(bytemuck::bytes_of_mut(&mut header)) else {
            bail!("Failed to read file bytes.");
        };
        if (n != 4 || header != 0x56544843/*VTHC*/) {
            bail!("Expected file header VTHC.");
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
