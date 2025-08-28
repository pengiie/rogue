use crate::{
    consts,
    engine::{
        asset::{
            asset::{AssetFile, AssetLoadError, AssetLoader},
            util::AssetByteReader,
        },
        voxel::voxel::{VoxelModelImpl, VoxelModelType},
    },
};

use super::{flat::load_flat_model, sft::load_sft_model, thc::load_thc_model};

pub struct VoxelModelAnyAsset {
    pub model: Box<dyn VoxelModelImpl>,
    pub model_type: VoxelModelType,
}

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
                model_type: VoxelModelType::THCCompressed,
            }),
            Some(consts::io::header::SFT) => Ok(VoxelModelAnyAsset {
                model: Box::new(load_sft_model(reader)?),
                model_type: VoxelModelType::SFTCompressed,
            }),
            _ => Err(anyhow::anyhow!("Unknown header").into()),
        }
    }
}
