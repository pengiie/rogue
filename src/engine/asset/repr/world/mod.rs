use std::{ops::Deref, os::unix::fs::FileExt};

use anyhow::bail;
use nalgebra::Vector3;

use crate::{
    common::morton,
    consts,
    engine::{
        asset::{
            asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver},
            util::{AssetByteReader, AssetByteWriter},
        },
        voxel::voxel_terrain::{
            VoxelChunkRegion, VoxelChunkRegionData, VoxelChunkRegionNode, VoxelRegionLeafNode,
        },
    },
};

pub mod region;

// use crate::engine::{
//     asset::util::AssetByteWriter,
//     resource::Res,
//     voxel::{voxel_terrain::VoxelChunks, voxel_world::VoxelWorld},
//     world::game_world::GameWorld,
// };
//
// use super::{
//     asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver},
//     util::AssetByteReader,
// };
//
// pub mod voxel;
//
// pub struct VoxelTerrainAsset {
//     // Leaf layer of chunk uuids.
//     pub chunk_tree: Vec<uuid::Uuid>,
//     pub side_length: u32,
// }
//
// const FILE_VERSION: u32 = 1;
//
// impl VoxelTerrainAsset {
//     pub fn from_terrain(voxel_terrain: &VoxelTerrain) -> Self {
//         let side_length = voxel_terrain.chunk_tree().chunk_side_length;
//         let tree = vec![uuid::Uuid::nil(); side_length.pow(3) as usize];
//
//         VoxelTerrainAsset {
//             chunk_tree: tree,
//             side_length,
//         }
//     }
// }
//
// impl AssetLoader for VoxelTerrainAsset {
//     fn load(data: &AssetFile) -> std::result::Result<VoxelTerrainAsset, AssetLoadError>
//     where
//         Self: Sized + std::any::Any,
//     {
//         let mut reader = AssetByteReader::new(data.read_file(), "TERR")?;
//
//         let side_length = reader.read::<u32>()?;
//
//         let mut chunk_tree = vec![uuid::Uuid::nil(); side_length.pow(3) as usize];
//         reader.read_to_slice(chunk_tree.as_mut_slice())?;
//
//         Ok(VoxelTerrainAsset {
//             chunk_tree,
//             side_length,
//         })
//     }
// }
//
// impl AssetSaver for VoxelTerrainAsset {
//     fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
//     where
//         Self: Sized,
//     {
//         // 4 byte header, 4 byte version, 4 byte side_length.
//         let mut writer = AssetByteWriter::new(out_file.write_file(), "TERR", 1);
//         writer.write(&data.side_length);
//         writer.write_slice(data.chunk_tree.as_slice());
//
//         Ok(())
//     }
// }
