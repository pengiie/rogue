use std::collections::HashMap;

use nalgebra::Vector3;

use crate::{
    common::bitset::Bitset,
    engine::{
        asset::{
            asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver},
            util::{AssetByteReader, AssetByteWriter},
        },
        voxel::{
            attachment::{Attachment, AttachmentMap},
            flat::VoxelModelFlat,
        },
    },
};

pub fn load_flat_model(
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
