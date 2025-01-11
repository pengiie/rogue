use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap, HashSet},
    fs::File,
    future::Future,
    hash::Hash,
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::channel,
    time::Duration,
};

use anyhow::{anyhow, Context};
use log::{debug, info, warn};
use nalgebra::{Vector, Vector3};
use pollster::FutureExt;
use rogue_macros::Resource;
use slang::{Downcast, OptionsBuilder, SessionDescBuilder, TargetDescBuilder};
use wgpu::ErrorFilter;

use crate::{
    consts,
    engine::{
        asset::asset::{AssetFile, AssetLoadFuture, AssetLoader, Assets},
        resource::ResMut,
        window::time::Instant,
    },
};

use super::device::DeviceResource;

pub const SHADER_DIR: &'static str = "assets/shaders/";
pub const SHADER_BLIT: &'static str = "blit.slang";
pub const SHADER_TERRAIN_PREPASS: &'static str = "blit.slang";

#[derive(Resource)]
pub struct ShaderCompiler {
    global_session: slang::GlobalSession,

    cached_shaders: HashMap<ShaderCompilationOptions, Shader>,

    shader_constants: HashMap<String, String>,
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

#[derive(Clone, Debug)]
pub struct ShaderSetBinding {
    /// The name of the ParameterBlock global variable.
    pub name: String,
    pub set_index: u32,

    /// Mapping the full variable path to a shader binding.
    pub bindings: HashMap<String, (ShaderBinding, bool)>,

    /// If any global constant uniforms are defined, they will
    /// be placed in this UniformBuffer's binding index.
    pub global_uniform_binding_index: Option<u32>,
    pub global_uniforms_used: bool,
    pub global_uniforms_size: u32,
}

impl PartialEq for ShaderSetBinding {
    fn eq(&self, other: &Self) -> bool {
        let struct_match = self.name == other.name
            && self.set_index == other.set_index
            && self.global_uniform_binding_index == other.global_uniform_binding_index
            && self.global_uniforms_size == other.global_uniforms_size;

        let bindings_matched = !self
            .bindings
            .iter()
            .any(|(binding_name, (binding, _is_used))| {
                other
                    .bindings
                    .get(binding_name)
                    .map_or(true, |(b, _b_used)| binding != b)
            });

        struct_match && bindings_matched
    }
}

impl Eq for ShaderSetBinding {}

impl std::hash::Hash for ShaderSetBinding {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.set_index.hash(state);

        let mut bindings_copy = self.bindings.iter().collect::<Vec<_>>();
        bindings_copy.sort_by_key(|b| b.0);

        for (name, (binding, _is_used)) in bindings_copy.iter() {
            name.hash(state);
            binding.hash(state);
        }
        self.global_uniform_binding_index.hash(state);
        self.global_uniforms_size.hash(state);
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum ShaderBinding {
    Slot {
        binding_index: u32,
        binding_type: ShaderBindingType,
    },

    Uniform {
        expected_type: std::any::TypeId,
        size: u32,
        // Offset from the start of the uniform buffer.
        offset: u32,
    },
}

impl ShaderBinding {
    pub fn binding_slot_type(&self) -> Option<ShaderBindingType> {
        match self {
            ShaderBinding::Slot { binding_type, .. } => Some(*binding_type),
            ShaderBinding::Uniform { .. } => None,
        }
    }

    pub fn binding_index(&self, set: &ShaderSetBinding) -> u32 {
        match self {
            ShaderBinding::Slot {
                binding_index,
                binding_type,
            } => *binding_index,
            ShaderBinding::Uniform { .. } => set
                .global_uniform_binding_index
                .expect("Global uniform binding index should be set if we have a uniform binding."),
        }
    }
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

        let shader_constants = {
            let mut map = HashMap::new();
            map.insert(
                "CONST_METERS_PER_VOXEL".to_owned(),
                consts::voxel::VOXEL_METER_LENGTH.to_string(),
            );
            map.insert(
                "CONST_TERRAIN_CHUNK_VOXEL_LENGTH".to_owned(),
                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH.to_string(),
            );
            map
        };

        Self {
            global_session,
            cached_shaders: HashMap::new(),
            shader_constants,
        }
    }

    pub fn invalidate_cache(&mut self) {
        self.cached_shaders.clear();
    }

    pub fn get_library_bindings(&self) -> anyhow::Result<Vec<ShaderSetBinding>> {
        let search_path = std::ffi::CString::new("assets/shaders").unwrap();

        let targets = [*TargetDescBuilder::new().format(slang::CompileTarget::Spirv)];

        let mut slang_opts = OptionsBuilder::new()
            .stage(slang::Stage::None)
            .warnings_as_errors("all");
        for (key, value) in &self.shader_constants {
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
        let lib_module = session
            .load_module("lib")
            .map_err(|err| anyhow!("Failed to create module `lib`.\nSlang: {:?}", err))?;
        let program = session
            .create_composite_component_type(&[lib_module.downcast().clone()])
            .map_err(|err| anyhow!("Failed to create shader program.\nSlang: {:?}", err))?;

        let program_layout = program
            .layout(0)
            .map_err(|_| anyhow!("Failed to get shader program layout."))?;

        let mut shader_bindings = Self::reflect_bindings(program_layout, None);
        shader_bindings.sort_by_key(|set| set.set_index);

        log::debug!("Compiled library shader module.",);
        log::debug!("\tLibary bindings:");
        for binding in &shader_bindings {
            log::debug!("\t\t{:?}", binding);
        }

        Ok(shader_bindings)
    }

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

                let mut slang_opts = OptionsBuilder::new()
                    .stage(options.stage.into())
                    .warnings_as_errors("all");
                for (key, value) in &self.shader_constants {
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
                let lib_module = session.load_module("lib").map_err(|err| {
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
                        // Must include lib module to get reflection info for its defined global
                        // parameters.
                        lib_module.downcast().clone(),
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
                let entry_point_metadata = program
                    .entry_point_metadata(0, 0)
                    .map_err(|_| anyhow!("Failed to get shader entry point metadata."))?;

                let mut shader_bindings =
                    Self::reflect_bindings(program_layout, Some(&entry_point_metadata));
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

    fn reflect_bindings(
        program_layout: &slang::reflection::Shader,
        metadata: Option<&slang::Metadata>,
    ) -> Vec<ShaderSetBinding> {
        let mut set_bindings = Vec::new();

        fn process_var_layout(
            metadata: Option<&slang::Metadata>,
            set: &mut ShaderSetBinding,
            var_layout: &slang::reflection::VariableLayout,
            mut parent_var_name: String,
            mut binding_offset: u32,
            mut uniform_offset: u32,
        ) {
            let ty_layout = var_layout.type_layout();
            debug!(
                "Processing var layout: {:?}, type: {:?}",
                var_layout.variable().name(),
                ty_layout.kind()
            );
            let var_name = var_layout.variable().name().unwrap().to_owned();
            let var_name = if parent_var_name.is_empty() {
                var_name
            } else {
                parent_var_name + "." + &var_name
            };

            for category_index in 0..var_layout.category_count() {
                let category = var_layout.category_by_index(category_index);
                let offset = var_layout.offset(category) as u32;
                if category == slang::ParameterCategory::Uniform {
                    uniform_offset += offset;
                } else if category == slang::ParameterCategory::DescriptorTableSlot {
                    binding_offset += offset;
                }

                debug!(
                    "Category: {:?}, offset: {}, binspace: {}",
                    category,
                    offset,
                    var_layout.binding_space_with_category(category)
                );
            }

            let is_used_binding = metadata.map_or(true, |metadata| {
                metadata
                    .is_parameter_location_used(
                        slang::ParameterCategory::DescriptorTableSlot,
                        var_layout.binding_space_with_category(
                            slang::ParameterCategory::DescriptorTableSlot,
                        ) as u64,
                        binding_offset as u64,
                    )
                    .unwrap_or(false)
            });
            let is_used_uniform = metadata.map_or(true, |metadata| {
                metadata
                    .is_parameter_location_used(
                        slang::ParameterCategory::Uniform,
                        var_layout.binding_space_with_category(slang::ParameterCategory::Uniform)
                            as u64,
                        uniform_offset as u64,
                    )
                    .unwrap_or(false)
            });
            // TODO: Slang doesn't support querying whether uniforms are used, so always pretend
            // they are used for now.
            let is_used_uniform = true;
            debug!(
                "Is used binding {}, uniform {}",
                is_used_binding, is_used_uniform
            );

            match ty_layout.kind() {
                slang::TypeKind::Resource => 'm: {
                    let shape = ty_layout.resource_shape();
                    if shape == slang::ResourceShape::SlangStructuredBuffer {
                        process_var_layout(
                            metadata,
                            set,
                            ty_layout.element_var_layout(),
                            var_name,
                            binding_offset,
                            uniform_offset,
                        );
                        break 'm;
                    }
                    debug!("Resource with resource kind {:?}, binding: ", shape);

                    let binding_type = match ty_layout.resource_shape() {
                        slang::ResourceShape::SlangTexture2d => match ty_layout.resource_access() {
                            slang::ResourceAccess::Read => ShaderBindingType::SampledImage,
                            slang::ResourceAccess::Write => ShaderBindingType::StorageImage,
                            _ => unreachable!(),
                        },
                        slang::ResourceShape::SlangTextureCube => todo!(),
                        slang::ResourceShape::SlangByteAddressBuffer => {
                            ShaderBindingType::StorageBuffer
                        }
                        _ => todo!(),
                    };
                    set.bindings.insert(
                        var_name,
                        (
                            ShaderBinding::Slot {
                                binding_index: binding_offset,
                                binding_type,
                            },
                            is_used_binding,
                        ),
                    );
                }
                slang::TypeKind::ConstantBuffer => {
                    debug!("Constant buffer");
                    set.bindings.insert(
                        var_name,
                        (
                            ShaderBinding::Slot {
                                binding_index: binding_offset,
                                binding_type: ShaderBindingType::UniformBuffer,
                            },
                            is_used_binding,
                        ),
                    );
                }
                slang::TypeKind::Struct => {
                    debug!("Struct");
                    for field in ty_layout.fields() {
                        process_var_layout(
                            metadata,
                            set,
                            field,
                            var_name.clone(),
                            binding_offset,
                            uniform_offset,
                        );
                    }
                }
                slang::TypeKind::Scalar => {
                    let binding = match ty_layout.scalar_type() {
                        slang::ScalarType::Uint32 => ShaderBinding::Uniform {
                            expected_type: std::any::TypeId::of::<u32>(),
                            size: ty_layout.size(slang::ParameterCategory::Uniform) as u32,
                            offset: uniform_offset,
                        },
                        kind => todo!("Need to implement scalar kind {:?}", kind),
                    };
                    set.bindings.insert(var_name, (binding, is_used_uniform));
                }
                kind => {
                    warn!("Ignoring type with {:?}", kind);
                }
            }
        }

        fn process_global_scope(
            metadata: Option<&slang::Metadata>,
            set: &mut ShaderSetBinding,
            var_layout: &slang::reflection::VariableLayout,
            descriptor_slot_offset: Option<u32>,
        ) {
            let ty_layout = var_layout.type_layout();
            debug!(
                "Processing scope: {:?}, type: {:?}",
                var_layout.variable().name(),
                ty_layout.kind()
            );

            for category_index in 0..var_layout.category_count() {
                let category = var_layout.category_by_index(category_index);
                let offset = var_layout.offset(category);
                let space = var_layout.binding_space_with_category(category);
                let binding_space = var_layout.binding_space();
                debug!(
                    "Category: {:?}, offset: {}, space: {}, binding_space: {}, size: {}",
                    category,
                    offset,
                    space,
                    binding_space,
                    ty_layout.size(category)
                );
            }

            match ty_layout.kind() {
                slang::TypeKind::ParameterBlock => {
                    let binding_space = var_layout.binding_space_with_category(
                        slang::ParameterCategory::SubElementRegisterSpace,
                    );
                    //assert_eq!(
                    //    binding_space, set.set_index as usize,
                    //    "Set index should match the SubElementRegisterSpace binding space."
                    //);
                    assert!(descriptor_slot_offset.is_none());

                    process_global_scope(
                        metadata,
                        set,
                        ty_layout.element_var_layout(),
                        descriptor_slot_offset,
                    );
                }
                slang::TypeKind::Struct => {
                    for category_index in 0..var_layout.category_count() {
                        let category = var_layout.category_by_index(category_index);
                        if category == slang::ParameterCategory::Uniform {
                            set.global_uniform_binding_index =
                                Some(var_layout.offset(category) as u32);
                            set.global_uniforms_size = ty_layout.size(category) as u32;
                        }
                    }

                    let slot_offset =
                        var_layout.offset(slang::ParameterCategory::DescriptorTableSlot) as u32;
                    let uniform_offset =
                        var_layout.offset(slang::ParameterCategory::Uniform) as u32;

                    for field in ty_layout.fields() {
                        process_var_layout(
                            metadata,
                            set,
                            field,
                            String::new(),
                            slot_offset,
                            uniform_offset,
                        );
                    }
                }
                _ => todo!(),
            }

            // let fields = if ty_layout.kind() == slang::TypeKind::ParameterBlock {
            //     ty_layout.element_type_layout().fields()
            // } else {
            //     ty_layout.fields()
            // };
            // for field in fields {
            //     debug!(
            //         "field bindings space {}",
            //         field.offset(slang::ParameterCategory::Uniform)
            //     );
            //     let field_type = field.type_layout();
            //     debug!("Encountered kind {:?}", field_type.kind());
            //     if field_type.kind() == slang::TypeKind::Struct {
            //         debug!("Processing struct {}", field.variable().name().unwrap());
            //         process_global_scope(bindings, set_index, field);
            //         continue;
            //     }

            //     let binding_type = match field_type.kind() {
            //         slang::TypeKind::Resource => match field_type.resource_shape() {
            //             slang::ResourceShape::SlangTexture2d => {
            //                 let has_write = field_type.resource_access()
            //                     == slang::ResourceAccess::Write
            //                     || field_type.resource_access() == slang::ResourceAccess::ReadWrite;
            //                 if has_write {
            //                     ShaderBindingType::StorageImage
            //                 } else {
            //                     ShaderBindingType::SampledImage
            //                 }
            //             }
            //             slang::ResourceShape::SlangStructuredBuffer
            //             | slang::ResourceShape::SlangByteAddressBuffer => {
            //                 ShaderBindingType::StorageBuffer
            //             }
            //             ty => todo!("Support reflection for shader resource type {:?}", ty),
            //         },
            //         slang::TypeKind::ConstantBuffer => ShaderBindingType::UniformBuffer,
            //         type_kind => {
            //             debug!("the space is {:?}", field.binding_space());
            //             panic!("Encountered non-supported non-resource ParameterBlock field when of type {:?} when reflecting shader.", type_kind);
            //         }
            //     };
            //     let binding_name = field
            //         .variable()
            //         .name()
            //         .expect("Failed to get shader binding variable name");

            //     bindings.push(ShaderBinding {
            //         binding_name: binding_name.to_owned(),
            //         binding_index: field.binding_index(),
            //         binding_type,
            //     });
            // }
        }

        let global_var_layout = program_layout.global_params_var_layout();

        let global_type_layout = global_var_layout.type_layout();
        match global_type_layout.kind() {
            // Module is considered a struct.
            slang::TypeKind::Struct => {
                for global_field in global_type_layout.fields() {
                    let field_type = global_field.type_layout();
                    assert_eq!(field_type.kind(), slang::TypeKind::ParameterBlock);
                    let set_index = global_field.binding_index();
                    let set_param_block_name = global_field.variable().name().unwrap();

                    let mut bindings = ShaderSetBinding {
                        name: set_param_block_name.to_owned(),
                        set_index: set_index as u32,
                        bindings: HashMap::new(),
                        global_uniform_binding_index: None,
                        global_uniforms_used: false,
                        global_uniforms_size: 0,
                    };
                    process_global_scope(metadata, &mut bindings, global_field, None);
                    bindings.global_uniforms_used = bindings
                        .bindings
                        .values()
                        .find(|(binding, is_used)| match binding {
                            ShaderBinding::Slot { .. } => false,
                            ShaderBinding::Uniform {
                                expected_type,
                                size,
                                offset,
                            } => *is_used,
                        })
                        .is_some();

                    set_bindings.push(bindings);
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
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ShaderModificationTree {
    dir_modified: Instant,
    files: HashMap<PathBuf, Instant>,
}

impl ShaderModificationTree {
    pub fn from_current_state() -> Self {
        let shader_dir_path = PathBuf::from_str(SHADER_DIR).unwrap();
        let shader_dir =
            std::fs::metadata(&shader_dir_path).expect("Unable to query shader dir metadata.");

        let mut tree = ShaderModificationTree {
            dir_modified: shader_dir
                .modified()
                .expect("Failed to get dir modified system time")
                .into(),
            files: HashMap::new(),
        };

        let mut to_process_dirs: Vec<PathBuf> = vec![shader_dir_path];
        while !to_process_dirs.is_empty() {
            let curr_dir_path = to_process_dirs.pop().unwrap();

            for child in std::fs::read_dir(curr_dir_path)
                .expect("Failed to read shader dir.")
                .into_iter()
            {
                if let Ok(child) = child {
                    let child_path = child.path();
                    let child_metadata = std::fs::metadata(&child_path).unwrap();
                    if child_metadata.is_dir() {
                        to_process_dirs.push(child_path.clone());
                    }

                    tree.files.insert(
                        child_path.clone(),
                        child_metadata.modified().unwrap().into(),
                    );
                } else {
                    warn!("Couldn't read dir child.");
                }
            }
        }

        tree
    }
}
