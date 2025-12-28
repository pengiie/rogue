use std::io::{BufRead, BufReader, Read};

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

impl ImageAsset {
    pub fn supported_extensions() -> &'static [&'static str] {
        &["png", "jpg", "jpeg"]
    }

    /// Returns the data in the format of RGBA, adds an alpha
    /// channel of 1 if it doesn't exist.
    pub fn convert_to_rgba(&self) -> Vec<u8> {
        match self.format {
            ImageAssetFormat::RGB => {
                let mut rgba_data = Vec::with_capacity((self.size.x * self.size.y * 4) as usize);
                for pixel_index in 0..(self.size.x * self.size.y) as usize {
                    let r = self.data[pixel_index * 3];
                    let g = self.data[pixel_index * 3 + 1];
                    let b = self.data[pixel_index * 3 + 2];
                    rgba_data.push(r);
                    rgba_data.push(g);
                    rgba_data.push(b);
                    rgba_data.push(255);
                }
                rgba_data
            }
            ImageAssetFormat::RGBA => self.data.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageAssetFormat {
    RGB,
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
            "jpg" | "jpeg" => {
                let mut reader = BufReader::new(file);
                let mut decoder = zune_jpeg::JpegDecoder::new(reader);
                decoder
                    .decode_headers()
                    .map_err(|err| anyhow::anyhow!("Failed to decode jpeg headers: {}", err))?;
                let info = decoder.info().ok_or_else(|| {
                    anyhow::anyhow!("Failed to get jpeg info after decoding headers.")
                })?;
                let data = decoder
                    .decode()
                    .map_err(|err| anyhow::anyhow!("Failed to decode jpeg data: {}", err))?;

                let format = match info.components {
                    3 => ImageAssetFormat::RGB,
                    4 => ImageAssetFormat::RGBA,
                    _ => {
                        return Err(anyhow::anyhow!(
                            "Unsupported number of components in jpeg: {}",
                            info.components
                        )
                        .into())
                    }
                };

                Ok(ImageAsset {
                    data,
                    format,
                    size: Vector2::new(info.width as u32, info.height as u32),
                })
            }
            ext => Err(anyhow::anyhow!("Unsupported extension \"{}\"", ext).into()),
        }
    }
}
