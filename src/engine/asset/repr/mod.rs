use super::asset::{AssetLoadError, AssetLoader, AssetSaver};

pub mod collider;
pub mod components;
pub mod editor_settings;
pub mod game_entity;
pub mod image;
pub mod project;
pub mod settings;
pub mod voxel;
pub mod world;

pub struct TextAsset {
    pub contents: String,
}

impl AssetLoader for TextAsset {
    fn load(
        data: &super::asset::AssetFile,
    ) -> std::result::Result<Self, super::asset::AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        data.read_contents()
            .map(|contents| TextAsset { contents })
            .map_err(|err| AssetLoadError::Other(anyhow::format_err!(err)))
    }
}

impl AssetSaver for TextAsset {
    fn save(data: &Self, out_file: &super::asset::AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        Ok(out_file.write_contents(data.contents.clone())?)
    }
}
