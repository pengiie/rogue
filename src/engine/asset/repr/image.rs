use nalgebra::Vector2;

use crate::engine::{
    asset::asset::{AssetFile, AssetLoadError, AssetLoader},
    graphics::backend::GfxImageFormat,
};

pub struct ImageAsset {
    pub data: Vec<u8>,
    pub format: ImageAssetFormat,
    pub size: Vector2<u32>,
}

pub enum ImageAssetFormat {
    RGBA,
}

impl AssetLoader for ImageAsset {
    fn load(data: &AssetFile) -> std::result::Result<Self, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let file = data.read_file()?;

        match data.extension() {
            "png" => {
                let decoder = png::Decoder::new(file);
                let mut reader = decoder
                    .read_info()
                    .map_err(|err| anyhow::anyhow!("Failed to load png."))?;

                let mut buf = vec![0; reader.output_buffer_size()];
                let info = reader
                    .next_frame(&mut buf)
                    .map_err(|err| anyhow::anyhow!("Failed to load png."))?;

                let bytes = buf.into_boxed_slice()[0..info.buffer_size()].to_vec();

                Ok(ImageAsset {
                    data: bytes,
                    format: ImageAssetFormat::RGBA,
                    size: Vector2::new(info.width, info.height),
                })
            }
            ext => Err(anyhow::anyhow!("Unsupported extension \"{}\"", ext).into()),
        }
    }
}
