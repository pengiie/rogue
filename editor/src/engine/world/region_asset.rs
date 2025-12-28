use crate::engine::{
    asset::{asset::AssetLoader, util::AssetByteReader},
    voxel::sft_compressed::VoxelModelSFTCompressed,
    world::region::WorldRegionNode,
};

pub struct WorldRegionAsset {
    pub nodes: Vec<WorldRegionNode>,
    pub model_handles: Vec<VoxelModelSFTCompressed>,
}

struct RegionAssetHeader {}

impl AssetLoader for WorldRegionAsset {
    fn load(
        file: &crate::engine::asset::asset::AssetFile,
    ) -> std::result::Result<Self, crate::engine::asset::asset::AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let file = file.read_file()?;
        let mut reader = AssetByteReader::new(file, "REG ")?;
        if reader.version() != 0 {
            anyhow::bail!("Unsupported region asset version {}", reader.version());
        }
    }
}
