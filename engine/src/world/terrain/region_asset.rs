use crate::asset::{asset::AssetLoader, util::AssetByteReader};
use crate::world::terrain::region::WorldRegionNode;
use crate::voxel::sft_compressed::VoxelModelSFTCompressed;

pub struct WorldRegionAsset {
    pub nodes: Vec<WorldRegionNode>,
    pub model_handles: Vec<VoxelModelSFTCompressed>,
}

struct RegionAssetHeader {}

impl AssetLoader for WorldRegionAsset {
    fn load(
        file: &crate::asset::asset::AssetFile,
    ) -> std::result::Result<Self, crate::asset::asset::AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let file = file.read_file()?;
        let mut reader = AssetByteReader::new(file, "REG ")?;
        // if reader.version() != 0 {
        //     anyhow::bail!("Unsupported region asset version {}", reader.version());
        // }
        todo!()
    }
}
