use std::{
    collections::{HashMap, HashSet},
    fs::File,
};

use naga_oil::compose::ShaderDefValue;

use crate::engine::asset::asset::{AssetFile, AssetLoader};

use super::device::DeviceResource;

macro_rules! include_shader {
    ($e:expr) => {
        include_str!(concat!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/assets/shaders/"),
            $e
        ))
    };
}

pub mod ui {
    pub const SOURCE: &str = include_shader!("ui.wgsl");
    pub const PATH: &str = "shaders::ui::wgsl";
}

pub mod blit {
    pub const SOURCE: &str = include_shader!("blit.wgsl");
    pub const PATH: &str = "shaders::blit::wgsl";
}

pub mod voxel_trace {
    pub const SOURCE: &str = include_shader!("voxel_trace.wgsl");
    pub const PATH: &str = "shaders::voxel_trace::wgsl";
    pub const WORKGROUP_SIZE: [u32; 3] = [8, 8, 1];
}

pub struct RawShader {
    source: String,
}

impl AssetLoader<AssetFile> for RawShader {
    fn load(data: &AssetFile) -> Self {
        RawShader {
            source: data.read_contents(),
        }
    }
}

pub struct Shader {
    source: String,
}

impl Shader {
    pub fn process_raw(raw_shader: &RawShader, defines: HashMap<String, bool>) -> Self {
        let preprocessor = naga_oil::compose::preprocess::Preprocessor::default();
        let out = preprocessor
            .preprocess(
                &raw_shader.source,
                &defines
                    .into_iter()
                    .map(|(def, val)| (def, ShaderDefValue::Bool(val)))
                    .collect::<HashMap<_, _>>(),
            )
            .expect("Failed to process shader");

        Self {
            source: out.preprocessed_source,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn create_module(&self, device: &DeviceResource) -> wgpu::ShaderModule {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(&self.source)),
        })
    }
}
