use std::io::Read;

use crate::asset::asset::{AssetLoader, AssetSaver};

pub struct GltfAsset {
    pub document: gltf::Document,
    pub buffers: Vec<gltf::buffer::Data>,
    pub images: Vec<gltf::image::Data>,
}

impl AssetLoader for GltfAsset {
    fn load(
        data: &super::asset::AssetFile,
    ) -> std::result::Result<Self, super::asset::AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let (document, buffers, images) = gltf::import(data.path().path())
            .map_err(|e| super::asset::AssetLoadError::Other(anyhow::format_err!(e)))?;
        Ok(GltfAsset {
            document,
            buffers,
            images,
        })
    }
}

impl AssetSaver for gltf::Gltf {
    fn save(data: &Self, out_file: &super::asset::AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        todo!()
    }
}
