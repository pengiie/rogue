use std::collections::HashMap;

use crate::asset::asset::AssetLoader;

pub struct RMapAssetHeader {}

impl AssetLoader for RMapAssetHeader {
    fn load(
        file: &crate::asset::asset::AssetFile,
    ) -> std::result::Result<Self, crate::asset::asset::AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        todo!()
    }
}
