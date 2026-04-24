use serde::ser::{SerializeSeq, SerializeStruct};

use crate::asset::asset::GameAssetPath;
use crate::common::freelist::FreeListHandle;
use crate::impl_asset_load_save_serde;
use crate::material::material_bank::{MaterialAssetId, MaterialId};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaterialTextureType {
    Color,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct MaterialAsset {
    pub name: String,
    #[serde(skip)]
    pub asset_path: Option<GameAssetPath>,
    pub color_texture: Option<GameAssetPath>,
}

impl MaterialAsset {
    pub fn is_empty(&self) -> bool {
        return self.color_texture.is_none();
    }
}

impl_asset_load_save_serde!(MaterialAsset);

#[derive(Hash, Clone, PartialEq, Eq)]
pub struct MaterialSamplerOptions {}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MaterialSerializable {
    pub id: MaterialId,
    pub name: String,
    pub asset_path: Option<GameAssetPath>,
}
