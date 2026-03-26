use std::{collections::HashMap, io::Read};

use zune_jpeg::zune_core::bytestream::ZByteWriterTrait;

use crate::{
    asset::asset::{AssetLoader, AssetSaver},
    voxel::{
        attachment::{Attachment, AttachmentMap},
        sft_compressed::{
            SFTAttachmentLookupNodeCompressed, SFTNodeCompressed, VoxelModelSFTCompressed,
        },
    },
};

pub struct RVOXAsset {
    pub sft_compressed: VoxelModelSFTCompressed,
}

impl AssetLoader for RVOXAsset {
    fn load(
        file: &crate::asset::asset::AssetFile,
    ) -> std::result::Result<Self, crate::asset::asset::AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut file = file.read_file()?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf);

        if buf[0..4] != [b'R', b'V', b'O', b'X'] {
            return Err(anyhow::anyhow!("Invalid RVOX header").into());
        }

        let length = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let node_data_len = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
        let mut node_data = Vec::with_capacity(node_data_len);
        let mut cursor = 12;
        for i in 0..node_data_len {
            let child_ptr = u32::from_le_bytes(buf[cursor..(cursor + 4)].try_into().unwrap());
            cursor += 4;
            let child_mask = u64::from_le_bytes(buf[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let leaf_mask = u64::from_le_bytes(buf[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;

            node_data.push(SFTNodeCompressed {
                child_ptr,
                child_mask,
                leaf_mask,
            });
        }

        let attachment_nodes_len =
            u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
        let mut attachment_bmat_nodes = Vec::with_capacity(attachment_nodes_len);
        cursor += 4;
        for i in 0..attachment_nodes_len {
            let data_ptr = u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let attachment_mask = u64::from_le_bytes(buf[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;

            attachment_bmat_nodes.push(SFTAttachmentLookupNodeCompressed {
                data_ptr,
                attachment_mask,
            });
        }

        let attachment_data_len =
            u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let attachment_bmat_data = bytemuck::cast_slice::<u8, u32>(
            &buf[cursor..(cursor + attachment_data_len * Attachment::BMAT.byte_size() as usize)],
        )
        .to_vec();

        let mut attachment_map = AttachmentMap::new();
        attachment_map.register_attachment(Attachment::BMAT);
        let mut attachment_lookup_data = AttachmentMap::new();
        attachment_lookup_data.insert(Attachment::BMAT_ID, attachment_bmat_nodes);
        let mut attachment_raw_data = AttachmentMap::new();
        attachment_raw_data.insert(Attachment::BMAT_ID, attachment_bmat_data);
        let sft = VoxelModelSFTCompressed {
            side_length: length,
            attachment_map,
            node_data,
            attachment_lookup_data,
            attachment_raw_data,
            update_tracker: 0,
        };

        Ok(RVOXAsset {
            sft_compressed: sft,
        })
    }
}

impl AssetSaver for RVOXAsset {
    fn save(data: &Self, out_file: &crate::asset::asset::AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        let sft = &data.sft_compressed;
        let mut bytes = vec![b'R', b'V', b'O', b'X'];
        // Write side length.
        bytes.extend_from_slice(&sft.side_length.to_le_bytes());

        // Node data chunk.
        // Write node data length.
        bytes.extend_from_slice(&(sft.node_data.len() as u32).to_le_bytes());
        for node in &sft.node_data {
            bytes.extend_from_slice(&node.child_ptr.to_le_bytes());
            bytes.extend_from_slice(&node.child_mask.to_le_bytes());
            bytes.extend_from_slice(&node.leaf_mask.to_le_bytes());
        }

        let attachment_nodes = sft
            .attachment_lookup_data
            .get(Attachment::BMAT_ID)
            .expect("all other attachments are legacy now but idk maybe its different later.");
        // This might be shorder than node length.
        bytes.extend_from_slice(&(attachment_nodes.len() as u32).to_le_bytes());
        for node in attachment_nodes {
            bytes.extend_from_slice(&node.data_ptr.to_le_bytes());
            bytes.extend_from_slice(&node.attachment_mask.to_le_bytes());
        }

        let attachment_data = sft
            .attachment_raw_data
            .get(Attachment::BMAT_ID)
            .expect("all other attachments are legacy now but idk maybe its different later.");
        bytes.extend_from_slice(
            &(attachment_data.len() as u32 / Attachment::BMAT.size()).to_le_bytes(),
        );
        bytes.extend_from_slice(bytemuck::cast_slice(attachment_data.as_slice()));

        let mut file = out_file.write_file();
        file.write_all_bytes(&bytes)
            .map_err(|e| anyhow::anyhow!("Failed to write bytes into RVOX file: {:?}", e))?;

        Ok(())
    }
}
