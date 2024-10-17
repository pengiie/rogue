use std::{
    collections::{HashMap, HashSet},
    fs::File,
    future::Future,
    sync::mpsc::channel,
    time::Duration,
};

use anyhow::anyhow;
use log::{debug, info};
use naga_oil::compose::ShaderDefValue;
use pollster::FutureExt;
use wgpu::ErrorFilter;

use crate::engine::asset::asset::{AssetFile, AssetFuture, AssetLoader};

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
    fn load(data: &AssetFile) -> impl AssetFuture<Self> {
        async move {
            Ok(RawShader {
                source: data.read_contents().await,
            })
        }
    }
}

pub struct Shader {
    source: String,
}

impl Shader {
    pub fn process_raw(
        raw_shader: &RawShader,
        defines: HashMap<String, bool>,
    ) -> anyhow::Result<Self> {
        let preprocessor = naga_oil::compose::preprocess::Preprocessor::default();
        let out = preprocessor.preprocess(
            &raw_shader.source,
            &defines
                .into_iter()
                .map(|(def, val)| (def, ShaderDefValue::Bool(val)))
                .collect::<HashMap<_, _>>(),
        )?;

        Ok(Self {
            source: out.preprocessed_source,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn create_module(&self, device: &DeviceResource) -> anyhow::Result<wgpu::ShaderModule> {
        //device.push_error_scope(ErrorFilter::Validation);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(&self.source)),
        });
        debug!("Making shader module!!!!");

        // let mut error: Option<wgpu::Error> = None;
        // cfg_if::cfg_if! {
        //     if #[cfg(target_arch = "wasm32")] {
        //         let (send, recv) = channel::<Option<wgpu::Error>>();
        //         let error_fut = device.pop_error_scope();
        //         let fut = async move {
        //             let error = error_fut.await;
        //             let _ = send.send(error);
        //         };
        //         wasm_bindgen_futures::spawn_local(fut);
        //         if let Ok(recv_error) = recv.recv_timeout(Duration::from_secs(10)) {
        //             error = recv_error;
        //         } else {
        //             anyhow::bail!("Couldn't pop wgpu error scope, sender timed out.");
        //         }
        //     } else {
        //         error = pollster::block_on(device.pop_error_scope());
        //     }
        // }

        // if let Some(error) = error {
        //     let shader_info = pollster::block_on(module.get_compilation_info());
        //     return Err(anyhow!(error.to_string()));
        // }

        Ok(module)
    }
}
