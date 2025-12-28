use crate::{
    common::dyn_vec::TypeInfo,
    consts,
    engine::{
        asset::{
            asset::{AssetFile, AssetLoadError, AssetLoader},
            util::AssetByteReader,
        },
        voxel::{
            flat::VoxelModelFlat,
            sft::VoxelModelSFT,
            voxel::{VoxelModelImpl, VoxelModelImplMethods},
        },
    },
};

use super::{flat::load_flat_model, sft::load_sft_model, thc::load_thc_model};

pub struct VoxelModelAsset {
    model: *mut u8,
    model_type: String,
    model_type_info: TypeInfo,
}

impl VoxelModelAsset {
    pub fn new<T: VoxelModelImpl>(model: T) -> Self {
        Self {
            model: Box::into_raw(Box::new(model)) as *mut u8,
            model_type: T::NAME.to_owned(),
            model_type_info: TypeInfo::new::<T>(),
        }
    }
}

impl Drop for VoxelModelAsset {
    fn drop(&mut self) {
        if !self.model.is_null() {
            // Safety: We check the model ptr ownership isn't taken, and it is allocated with Box
            // which is the same layout as the type.
            unsafe { self.model_type_info.drop(self.model) };
            unsafe { std::alloc::dealloc(self.model, self.model_type_info.layout(1)) };
        }
    }
}

// Safety: `model` is an owned ptr so it is safe to send across threads.
unsafe impl Send for VoxelModelAsset {}

impl AssetLoader for VoxelModelAsset {
    fn load(data: &AssetFile) -> std::result::Result<Self, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader = AssetByteReader::new_unknown(data.read_file()?)?;
        match reader.header() {
            Some("FLAT") => Ok(VoxelModelAsset::new(load_flat_model(reader)?)),
            Some(consts::io::header::SFT) => Ok(VoxelModelAsset::new(load_sft_model(reader)?)),
            _ => Err(anyhow::anyhow!("Unknown header").into()),
        }
    }
}
