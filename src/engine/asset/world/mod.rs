use std::os::unix::fs::FileExt;

use anyhow::bail;

use crate::engine::{
    resource::Res,
    voxel::{voxel_terrain::VoxelTerrain, voxel_world::VoxelWorld},
    world::game_world::GameWorld,
};

use super::asset::{AssetFile, AssetSaver};

pub mod voxel;

pub struct VoxelTerrainAsset {
    // Full in array octree.
    chunk_tree: Vec<uuid::Uuid>,
    side_length: u32,
}

const FILE_VERSION: u32 = 1;

impl VoxelTerrainAsset {
    pub fn from_terrain(voxel_terrain: &VoxelTerrain) -> Self {
        let side_length = voxel_terrain.chunk_tree().chunk_side_length;
        let height = side_length.trailing_zeros();
        let n = (8u64.pow(height + 1) - 1) / 7;
        let tree = vec![uuid::Uuid::nil(); n as usize];

        VoxelTerrainAsset {
            chunk_tree: tree,
            side_length,
        }
    }
}

impl AssetSaver for VoxelTerrainAsset {
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        // 4 byte header, 4 byte version, 4 byte side_length.
        const HEADER_BYTE_SIZE: u32 = 4 + 4 + 4;
        let mut req_bytes = HEADER_BYTE_SIZE as usize;
        req_bytes += (16 * data.chunk_tree.len());
        let mut file = out_file.write_file();
        file.set_len(req_bytes as u64);

        assert_eq!(
            file.write_at(
                bytemuck::bytes_of(&[0x56544843u32, FILE_VERSION, data.side_length]),
                0,
            )?,
            HEADER_BYTE_SIZE as usize
        );

        assert_eq!(
            file.write_at(
                bytemuck::cast_slice::<uuid::Uuid, u8>(data.chunk_tree.as_slice()),
                HEADER_BYTE_SIZE as u64
            )?,
            16 * data.chunk_tree.len()
        );

        Ok(())
    }
}
