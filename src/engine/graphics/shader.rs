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
use nalgebra::{Vector, Vector3};
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

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct ShaderSetBinding {
    pub name: String,
    pub set_index: u32,
    pub bindings: Vec<ShaderBinding>,
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct ShaderBinding {
    pub binding_name: String,
    pub binding_index: u32,
    pub binding_type: ShaderBindingType,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ShaderBindingType {
    Sampler,
    SampledImage,
    StorageImage,
    UniformBuffer,
    StorageBuffer,
}

#[derive(Debug)]
pub struct ShaderPipelineInfo {
    pub workgroup_size: Option<Vector3<u32>>,
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
            Entry::Occupied(e) => {
                //todo!("Check modified date of file");
                Ok(e.into_mut())
            }
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

                let program_layout = program
                    .layout(0)
                    .map_err(|_| anyhow!("Failed to get shader program layout."))?;

                let mut shader_bindings = Self::reflect_bindings(program_layout);
                shader_bindings.sort_by_key(|set| set.set_index);

                let shader_pipeline_info =
                    Self::reflect_pipeline_info(program_layout, &options.entry_point);

                let shader = Shader {
                    code_blob: kernel_blob,
                    entry_point_name: options.entry_point.clone(),
                    bindings: shader_bindings,
                    pipline_info: shader_pipeline_info,
                };
                log::debug!(
                    "Compiled shader module `{}` with entry point `{}`.",
                    options.module,
                    options.entry_point
                );
                log::debug!("\tShader pipeline info:");
                log::debug!("\t\t{:?}", shader.pipline_info);
                log::debug!("\tBindings:");
                for binding in &shader.bindings {
                    log::debug!("\t\t{:?}", binding);
                }

                Ok(e.insert(shader))
            }
        }
    }

    fn reflect_bindings(program_layout: &slang::reflection::Shader) -> Vec<ShaderSetBinding> {
        let mut set_bindings = Vec::new();

        let global_var_layout = program_layout.global_params_var_layout();
        let global_type_layout = global_var_layout.type_layout();
        match global_type_layout.kind() {
            // Module is considered a struct.
            slang::TypeKind::Struct => {
                for global_field in global_type_layout.fields() {
                    let field_type = global_field.type_layout();
                    assert_eq!(field_type.kind(), slang::TypeKind::ParameterBlock);
                    let set_index = global_field.semantic_index();
                    let set_param_block_name = global_field.variable().name().unwrap();

                    let mut bindings = Vec::new();

                    for field in field_type.element_type_layout().fields() {
                        let field_type = field.type_layout();
                        if field_type.kind() != slang::TypeKind::Resource {
                            panic!("Encountered non-resource ParameterBlock field when reflecting shader.");
                        }
                        let binding_type = match field_type.resource_shape() {
                            slang::ResourceShape::SlangTexture2d => {
                                let has_write = field_type.resource_access()
                                    == slang::ResourceAccess::Write
                                    || field_type.resource_access()
                                        == slang::ResourceAccess::ReadWrite;
                                if has_write {
                                    ShaderBindingType::StorageImage
                                } else {
                                    ShaderBindingType::SampledImage
                                }
                            }
                            slang::ResourceShape::SlangStructuredBuffer => {
                                ShaderBindingType::UniformBuffer
                            }
                            ty => todo!("Support reflection for shader resource type {:?}", ty),
                        };
                        let binding_name = field
                            .variable()
                            .name()
                            .expect("Failed to get shader binding variable name");

                        bindings.push(ShaderBinding {
                            binding_name: binding_name.to_owned(),
                            binding_index: field.binding_index(),
                            binding_type,
                        });
                    }

                    set_bindings.push(ShaderSetBinding {
                        name: set_param_block_name.to_owned(),
                        set_index: set_index as u32,
                        bindings,
                    });
                }
            }
            _ => unreachable!(),
        }

        set_bindings
    }

    fn reflect_pipeline_info(
        program_layout: &slang::reflection::Shader,
        entry_point: &str,
    ) -> ShaderPipelineInfo {
        let entry_point = program_layout
            .find_entry_point_by_name(entry_point)
            .expect("Failed to reflect entry point.");
        let workgroup_size = (entry_point.stage() == slang::Stage::Compute).then(|| {
            let size = entry_point.compute_thread_group_size();
            Vector3::new(size.0 as u32, size.1 as u32, size.2 as u32)
        });

        ShaderPipelineInfo { workgroup_size }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct ShaderPath {
    path: String,
}

impl ShaderPath {
    pub fn new(path: String) -> anyhow::Result<Self> {
        let regex =
            regex::Regex::new(r"^(([a-zA-Z]+)(_[a-zA-Z]+)*)(::(([a-zA-Z]+)(_[a-zA-Z]+)*))*$")
                .unwrap();
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

    pub fn module(&self) -> String {
        let split = self.path.split("::");
        split.last().unwrap().to_owned()
    }
}

pub struct Shader {
    code_blob: slang::Blob,
    entry_point_name: String,
    bindings: Vec<ShaderSetBinding>,
    pipline_info: ShaderPipelineInfo,
}

impl Shader {
    pub fn as_str(&self) -> anyhow::Result<&str> {
        Ok(self.code_blob.as_str()?)
    }

    pub fn as_u32_slice(&self) -> &[u32] {
        bytemuck::cast_slice(self.code_blob.as_slice())
    }

    pub fn bindings(&self) -> &Vec<ShaderSetBinding> {
        &self.bindings
    }

    pub fn pipeline_info(&self) -> &ShaderPipelineInfo {
        &self.pipline_info
    }

    pub fn entry_point_name(&self) -> &str {
        &self.entry_point_name
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
