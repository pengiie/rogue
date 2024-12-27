use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap, HashSet},
    fs::File,
    future::Future,
    path::PathBuf,
    str::FromStr,
    sync::mpsc::channel,
    time::Duration,
};

use anyhow::{anyhow, Context};
use log::{debug, info};
use pollster::FutureExt;
use rogue_macros::Resource;
use slang::{Downcast, OptionsBuilder, SessionDescBuilder, TargetDescBuilder};
use wgpu::ErrorFilter;

use crate::engine::{
    asset::asset::{AssetFile, AssetLoadFuture, AssetLoader, Assets},
    resource::ResMut,
};

use super::device::DeviceResource;

pub const SHADER_BLIT: &'static str = "blit.slang";
pub const SHADER_TERRAIN_PREPASS: &'static str = "blit.slang";

#[derive(Resource)]
pub struct ShaderCompiler {
    global_session: slang::GlobalSession,

    cached_shaders: HashMap<ShaderCompilationOptions, Shader>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ShaderCompilationOptions {
    pub module: String,
    pub entry_point: String,
    pub stage: ShaderStage,
    pub target: ShaderCompilationTarget,
    pub macro_defines: HashMap<String, String>,
}

impl std::hash::Hash for ShaderCompilationOptions {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.module.hash(state);
        self.entry_point.hash(state);
        self.stage.hash(state);
        self.target.hash(state);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderCompilationTarget {
    SpirV,
    Wgsl,
}

impl From<ShaderCompilationTarget> for slang::CompileTarget {
    fn from(target: ShaderCompilationTarget) -> Self {
        match target {
            ShaderCompilationTarget::SpirV => slang::CompileTarget::Spirv,
            ShaderCompilationTarget::Wgsl => slang::CompileTarget::Wgsl,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

impl From<ShaderStage> for slang::Stage {
    fn from(value: ShaderStage) -> Self {
        match value {
            ShaderStage::Vertex => slang::Stage::Vertex,
            ShaderStage::Fragment => slang::Stage::Fragment,
            ShaderStage::Compute => slang::Stage::Compute,
        }
    }
}

impl ShaderCompiler {
    pub fn new() -> Self {
        let global_session = slang::GlobalSession::new().unwrap();

        Self {
            global_session,
            cached_shaders: HashMap::new(),
        }
    }

    pub fn update(mut shader_compiler: ResMut<ShaderCompiler>, mut assets: ResMut<Assets>) {}

    pub fn compile_shader<'a>(
        &'a mut self,
        options: ShaderCompilationOptions,
    ) -> anyhow::Result<&'a Shader> {
        match self.cached_shaders.entry(options) {
            Entry::Occupied(e) => Ok(e.into_mut()),
            Entry::Vacant(e) => {
                let options = e.key();

                let search_path = std::ffi::CString::new("assets/shaders").unwrap();

                let targets = [*TargetDescBuilder::new().format(options.target.into())];
                let mut slang_opts = OptionsBuilder::new().stage(options.stage.into());
                for (key, value) in &options.macro_defines {
                    slang_opts = slang_opts.macro_define(key, value);
                }

                let mut session = self
                    .global_session
                    .create_session(
                        &SessionDescBuilder::new()
                            .targets(&targets)
                            .search_paths(&[search_path.as_ptr()])
                            .options(&slang_opts),
                    )
                    .expect("Failed to create slang session");
                let module = session.load_module(&options.module).map_err(|err| {
                    anyhow!(
                        "Failed to create module {}.\nSlang: {:?}",
                        options.module,
                        err
                    )
                })?;
                let entry_point = module
                    .find_entry_point_by_name(&options.entry_point)
                    .with_context(|| {
                        format!(
                            "Failed to find shader entry point '{}'",
                            options.entry_point
                        )
                    })?;
                let program = session
                    .create_composite_component_type(&[
                        module.downcast().clone(),
                        entry_point.downcast().clone(),
                    ])
                    .map_err(|err| anyhow!("Failed to create shader program.\nSlang: {:?}", err))?;
                let linked_program = program.link().expect("Failed to link slang program.");
                let kernel_blob = linked_program
                    .entry_point_code(0, 0)
                    .expect("Failed to produce slang kernel blob.");

                Ok(e.insert(Shader {
                    code_blob: kernel_blob,
                }))
            }
        }
    }
}

pub struct ShaderPath {
    path: String,
}

impl ShaderPath {
    pub fn new(path: String) -> anyhow::Result<Self> {
        let regex = regex::Regex::new(r"^[a-zA-Z_](::[a-zA-Z_])*$").unwrap();
        anyhow::ensure!(regex.is_match(&path), "Shader path of {} is invalid.", path);

        Ok(Self { path })
    }

    pub fn file_path(&self) -> PathBuf {
        let mut relative_path = self
            .path
            .split("::")
            .fold(String::new(), |mut acc, segment| {
                acc += segment;
                acc += "/";
                acc
            });
        relative_path.truncate(relative_path.len() - 1); // Remove final `/`.

        let path = "assets/shaders/".to_owned() + &relative_path + ".slang";
        PathBuf::from_str(&path).unwrap()
    }
}

pub struct Shader {
    code_blob: slang::Blob,
}

impl Shader {
    pub fn as_str(&self) -> anyhow::Result<&str> {
        Ok(self.code_blob.as_str()?)
    }

    pub fn as_u32_slice(&self) -> &[u32] {
        bytemuck::cast_slice(self.code_blob.as_slice())
    }

    pub fn bindings(&self) -> HashMap</*uniform_name=*/ String, (/*set=*/ u32, /*binding=*/ u32)> {
        todo!("")
    }

    // pub fn create_wgpu_module(
    //     &self,
    //     device: &DeviceResource,
    // ) -> anyhow::Result<wgpu::ShaderModule> {
    //     #[cfg(not(target_arch = "wasm32"))]
    //     device.push_error_scope(ErrorFilter::Validation);

    //     let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    //         label: None,
    //         source: wgpu::ShaderSource::Wgsl(Cow::from(self.code_blob.as_str().unwrap())),
    //     });

    //     #[cfg(not(target_arch = "wasm32"))]
    //     if let Some(error) = pollster::block_on(device.pop_error_scope()) {
    //         let shader_info = pollster::block_on(module.get_compilation_info());
    //         return Err(anyhow!(error.to_string()));
    //     }

    //     Ok(module)
    // }
}
