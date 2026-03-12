use std::{
    borrow::Cow,
    collections::{HashMap, HashSet, hash_map::Entry},
    fs::File,
    future::Future,
    hash::Hash,
    path::{Path, PathBuf},
    str::FromStr,
    sync::mpsc::channel,
    time::Duration,
};

use anyhow::{Context, anyhow};
use log::{debug, info, warn};
use nalgebra::{Vector, Vector3};
use pollster::FutureExt;
use rogue_macros::Resource;

use super::device::DeviceResource;
use crate::asset::asset::{AssetFile, AssetLoadFuture, AssetLoader, Assets};
use crate::consts;
use crate::resource::ResMut;
use crate::window::time::Instant;

pub const SHADER_DIR: &'static str = "assets/shaders/";

pub struct ShaderCompiler {
    global_session: shader_slang::GlobalSession,
    cached_shaders: HashMap<ShaderDesc, Shader>,
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

impl ShaderCompilationOptions {
    fn get_desc(&self) -> ShaderDesc {
        ShaderDesc {
            module: ShaderPath::new_unchecked(self.module.clone()),
            entry_point_name: self.entry_point.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderCompilationTarget {
    SpirV,
    Wgsl,
}

/// Set binding info generated from shader_slang reflection.
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

impl ShaderSetBinding {
    pub fn merge_clone(&self, other: &ShaderSetBinding) -> ShaderSetBinding {
        todo!("merge clone this set aggregating the used bindings between both sets.");
    }
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
    SamplerArray { size: u32 },
    SampledImage,
    SampledImageArray { size: u32 },
    StorageImage,
    UniformBuffer,
    StorageBuffer,
    StorageBufferArray { size: u32 },
}

#[derive(Debug)]
pub struct ShaderPipelineInfo {
    pub workgroup_size: Option<Vector3<u32>>,
}

impl From<ShaderCompilationTarget> for shader_slang::CompileTarget {
    fn from(target: ShaderCompilationTarget) -> Self {
        match target {
            ShaderCompilationTarget::SpirV => shader_slang::CompileTarget::Spirv,
            ShaderCompilationTarget::Wgsl => shader_slang::CompileTarget::Wgsl,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

impl From<ShaderStage> for shader_slang::Stage {
    fn from(value: ShaderStage) -> Self {
        match value {
            ShaderStage::Vertex => shader_slang::Stage::Vertex,
            ShaderStage::Fragment => shader_slang::Stage::Fragment,
            ShaderStage::Compute => shader_slang::Stage::Compute,
        }
    }
}

impl ShaderCompiler {
    pub fn new() -> Self {
        let global_session = shader_slang::GlobalSession::new().unwrap();

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
            map.insert(
                "CONST_TERRAIN_REGION_CHUNK_LENGTH".to_owned(),
                consts::voxel::TERRAIN_REGION_CHUNK_LENGTH.to_string(),
            );
            map.insert(
                "CONST_TERRAIN_REGION_VOXEL_LENGTH".to_owned(),
                consts::voxel::TERRAIN_REGION_VOXEL_LENGTH.to_string(),
            );
            map
        };

        Self {
            global_session,
            cached_shaders: HashMap::new(),
            shader_constants,
        }
    }

    pub fn get_shader(&self, options: &ShaderCompilationOptions) -> &Shader {
        self.cached_shaders
            .get(&options.get_desc())
            .expect(&format!(
                "Failed to get shader with path `{:?}`",
                options.module
            ))
    }

    pub fn invalidate_cache(&mut self) {
        self.cached_shaders.clear();
    }

    pub fn get_library_bindings(&self) -> anyhow::Result<Vec<ShaderSetBinding>> {
        let search_path = std::ffi::CString::new("assets/shaders").unwrap();

        let targets =
            [shader_slang::TargetDesc::default().format(shader_slang::CompileTarget::Spirv)];

        let mut shader_slang_opts = shader_slang::CompilerOptions::default()
            .stage(shader_slang::Stage::None)
            .warnings_as_errors("all");
        for (key, value) in &self.shader_constants {
            shader_slang_opts = shader_slang_opts.macro_define(key, value);
        }

        let mut session = self
            .global_session
            .create_session(
                &shader_slang::SessionDesc::default()
                    .targets(&targets)
                    .search_paths(&[search_path.as_ptr()])
                    .options(&shader_slang_opts),
            )
            .expect("Failed to create shader_slang session");
        let lib_module = session
            .load_module("lib")
            .map_err(|err| anyhow!("Failed to create module `lib`.\nSlang: {:?}", err))?;
        let program = session
            .create_composite_component_type(&[lib_module.into()])
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

    fn compile_shader_ref(&mut self, options: ShaderCompilationOptions) -> anyhow::Result<&Shader> {
        let shader: anyhow::Result<&mut Shader> = match self
            .cached_shaders
            .entry(options.get_desc())
        {
            Entry::Occupied(e) => {
                //todo!("Check modified date of file");
                Ok(e.into_mut())
            }
            Entry::Vacant(e) => {
                let search_paths = [std::ffi::CString::new("assets/shaders").unwrap()];
                let search_path_ptrs = search_paths
                    .iter()
                    .map(|path| path.as_ptr())
                    .collect::<Vec<_>>();

                let targets = [shader_slang::TargetDesc::default().format(options.target.into())];

                let mut shader_slang_opts = shader_slang::CompilerOptions::default()
                    .stage(options.stage.into())
                    .warnings_as_errors("all")
                    .emit_spirv_directly(true)
                    .optimization(shader_slang::OptimizationLevel::High)
                    .vulkan_use_entry_point_name(true)
                    // Enable for shader shader_slang debugging.
                    //.dump_intermediates(true)
                    .capability(
                        self.global_session
                            .find_capability("SPV_KHR_non_semantic_info"),
                    );
                for (key, value) in &self.shader_constants {
                    shader_slang_opts = shader_slang_opts.macro_define(key, value);
                }

                let mut session = self
                    .global_session
                    .create_session(
                        &shader_slang::SessionDesc::default()
                            .targets(&targets)
                            .search_paths(&search_path_ptrs)
                            .options(&shader_slang_opts),
                    )
                    .expect("Failed to create shader_slang session");
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
                        module.into(),
                        entry_point.into(),
                        // Must include lib module to get reflection info for its defined global
                        // parameters, however include it last since for some reason it will try
                        // and take up set binding space even if it is not used in the resulting
                        // SPIR-V. TODO: Figure out how to properly get the set index.
                        lib_module.into(),
                    ])
                    .map_err(|err| anyhow!("Failed to create shader program.\nSlang: {:?}", err))?;
                let linked_program = program
                    .link()
                    .expect("Failed to link shader_slang program.");
                let kernel_blob = linked_program
                    .entry_point_code(0, 0)
                    .map_err(|err| anyhow!("Failed to produce shader_slang kernel blob."))?;

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
                    module_name: options.module.clone(),
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
        };

        Ok(shader?)
    }

    pub fn compile_shader<'a>(
        &'a mut self,
        options: ShaderCompilationOptions,
    ) -> anyhow::Result<&'a Shader> {
        self.compile_shader_ref(options)
    }

    pub fn compile_shader_pair<'a>(
        &'a mut self,
        options_a: ShaderCompilationOptions,
        options_b: ShaderCompilationOptions,
    ) -> anyhow::Result<(&'a Shader, &'a Shader)> {
        if options_a == options_b {
            panic!("Should not call `compile_shader_pair` if the options are equal.");
        }

        let desc_a = options_a.get_desc();
        let desc_b = options_b.get_desc();

        match self.compile_shader_ref(options_a) {
            Ok(ptr) => {}
            Err(err) => anyhow::bail!(err),
        };
        match self.compile_shader_ref(options_b) {
            Ok(ptr) => {}
            Err(err) => anyhow::bail!(err),
        };

        Ok((
            self.cached_shaders.get(&desc_a).unwrap(),
            self.cached_shaders.get(&desc_b).unwrap(),
        ))
    }

    fn reflect_bindings(
        program_layout: &shader_slang::reflection::Shader,
        metadata: Option<&shader_slang::Metadata>,
    ) -> Vec<ShaderSetBinding> {
        let mut set_bindings = Vec::new();

        fn process_var_layout(
            metadata: Option<&shader_slang::Metadata>,
            set: &mut ShaderSetBinding,
            var_layout: &shader_slang::reflection::VariableLayout,
            mut parent_var_name: String,
            mut binding_offset: u32,
            mut uniform_offset: u32,
            descriptor_array_size: Option<u32>,
        ) {
            let ty_layout = var_layout.type_layout().unwrap();
            // debug!(
            //     "Processing var layout: {:?}, type: {:?}",
            //     var_layout.variable().name(),
            //     ty_layout.kind()
            // );
            let var_name = var_layout.variable().unwrap().name().unwrap().to_owned();
            let var_name = if parent_var_name.is_empty() {
                var_name
            } else {
                parent_var_name + "." + &var_name
            };

            for category_index in 0..var_layout.category_count() {
                let category = var_layout.category_by_index(category_index).unwrap();
                let offset = var_layout.offset(category) as u32;
                if category == shader_slang::ParameterCategory::Uniform {
                    uniform_offset += offset;
                } else if category == shader_slang::ParameterCategory::DescriptorTableSlot {
                    binding_offset += offset;
                }

                // debug!("Category: {:?}, offset: {}", category, offset,);
            }

            let is_used_binding = metadata.map_or(true, |metadata| {
                metadata
                    .is_parameter_location_used(
                        shader_slang::ParameterCategory::DescriptorTableSlot,
                        var_layout.binding_space_with_category(
                            shader_slang::ParameterCategory::DescriptorTableSlot,
                        ) as u64,
                        binding_offset as u64,
                    )
                    .unwrap_or(false)
            });
            let is_used_uniform = metadata.map_or(true, |metadata| {
                metadata
                    .is_parameter_location_used(
                        shader_slang::ParameterCategory::Uniform,
                        var_layout
                            .binding_space_with_category(shader_slang::ParameterCategory::Uniform)
                            as u64,
                        uniform_offset as u64,
                    )
                    .unwrap_or(false)
            });
            // TODO: Slang doesn't support querying whether uniforms are used, so always pretend
            // they are used for now.
            let is_used_uniform = true;
            // debug!(
            //     "Is used binding {}, uniform {}",
            //     is_used_binding, is_used_uniform
            // );

            match ty_layout.kind() {
                shader_slang::TypeKind::Resource => 'm: {
                    // debug!("Resource with resource kind {:?}, binding: ", shape);

                    let binding_type = match ty_layout.resource_shape().unwrap() {
                        shader_slang::ResourceShape::SlangTexture2d => match ty_layout
                            .resource_access()
                            .unwrap()
                        {
                            shader_slang::ResourceAccess::Read => ShaderBindingType::SampledImage,
                            shader_slang::ResourceAccess::Write => ShaderBindingType::StorageImage,
                            shader_slang::ResourceAccess::ReadWrite => {
                                ShaderBindingType::StorageImage
                            }
                            _ => unreachable!(),
                        },
                        shader_slang::ResourceShape::SlangTextureCube => todo!(),
                        shader_slang::ResourceShape::SlangByteAddressBuffer
                        | shader_slang::ResourceShape::SlangStructuredBuffer => {
                            if let Some(descriptor_array_size) = descriptor_array_size {
                                ShaderBindingType::StorageBufferArray {
                                    size: descriptor_array_size,
                                }
                            } else {
                                // For debug
                                //let inner_layout = ty_layout.resource_result_type();
                                //log::debug!(
                                //    "Showing struct offsets for array struct {}",
                                //    inner_layout.name()
                                //);
                                ShaderBindingType::StorageBuffer
                            }
                        }
                        shader_slang::ResourceShape::SlangTexture2dArray => {
                            match ty_layout.resource_access().unwrap() {
                                shader_slang::ResourceAccess::Read => {
                                    ShaderBindingType::SampledImageArray {
                                        size: ty_layout.element_count().unwrap() as u32,
                                    }
                                }
                                shader_slang::ResourceAccess::Write => todo!(),
                                shader_slang::ResourceAccess::ReadWrite => todo!(),
                                _ => unreachable!(),
                            }
                        }
                        shader_slang::ResourceShape::SlangResourceExtShapeMask => {
                            todo!()
                        }
                        ty => todo!("{:?} not implemented", ty),
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
                shader_slang::TypeKind::ConstantBuffer => {
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
                shader_slang::TypeKind::Struct => {
                    for field in ty_layout.fields() {
                        process_var_layout(
                            metadata,
                            set,
                            field,
                            var_name.clone(),
                            binding_offset,
                            uniform_offset,
                            None,
                        );
                    }
                }
                shader_slang::TypeKind::SamplerState => {
                    set.bindings.insert(
                        var_name,
                        (
                            ShaderBinding::Slot {
                                binding_index: binding_offset,
                                binding_type: ShaderBindingType::Sampler,
                            },
                            is_used_binding,
                        ),
                    );
                }
                shader_slang::TypeKind::Scalar => {
                    let size = ty_layout.size(shader_slang::ParameterCategory::Uniform) as u32;
                    let offset = uniform_offset;
                    let expected_type = match ty_layout.scalar_type().unwrap() {
                        shader_slang::ScalarType::Uint32 => std::any::TypeId::of::<u32>(),
                        shader_slang::ScalarType::Float32 => std::any::TypeId::of::<f32>(),

                        kind => todo!("Need to implement scalar kind {:?}", kind),
                    };

                    set.bindings.insert(
                        var_name,
                        (
                            ShaderBinding::Uniform {
                                expected_type,
                                size,
                                offset,
                            },
                            is_used_uniform,
                        ),
                    );
                }
                shader_slang::TypeKind::Vector => 'm: {
                    let expected_type = match (
                        ty_layout.scalar_type().unwrap(),
                        ty_layout.element_count().unwrap(),
                    ) {
                        (shader_slang::ScalarType::Float32, 2) => {
                            Some(std::any::TypeId::of::<nalgebra::Vector2<f32>>())
                        }
                        (shader_slang::ScalarType::Float32, 3) => {
                            Some(std::any::TypeId::of::<nalgebra::Vector3<f32>>())
                        }
                        (shader_slang::ScalarType::Int32, 3) => {
                            Some(std::any::TypeId::of::<nalgebra::Vector3<i32>>())
                        }
                        (shader_slang::ScalarType::Uint32, 3) => {
                            Some(std::any::TypeId::of::<nalgebra::Vector3<u32>>())
                        }
                        _ => None,
                    };

                    let Some(expected_type) = expected_type else {
                        log::error!(
                            "Slang vector type with {:?} elements and scalar type {:?} is not supported.",
                            ty_layout.element_count(),
                            ty_layout.scalar_type()
                        );
                        break 'm;
                    };

                    let size = ty_layout.size(shader_slang::ParameterCategory::Uniform) as u32;
                    let offset = uniform_offset;
                    set.bindings.insert(
                        var_name,
                        (
                            ShaderBinding::Uniform {
                                expected_type,
                                size,
                                offset,
                            },
                            is_used_uniform,
                        ),
                    );
                }
                shader_slang::TypeKind::Matrix => 'm: {
                    let rows = ty_layout.row_count().unwrap();
                    let cols = ty_layout.column_count().unwrap();

                    let expected_type = match (rows, cols) {
                        (4, 4) => match ty_layout.scalar_type().unwrap() {
                            shader_slang::ScalarType::Float32 => {
                                Some(std::any::TypeId::of::<[f32; 16]>())
                            }
                            _ => None,
                        },
                        (3, 3) => match ty_layout.scalar_type().unwrap() {
                            shader_slang::ScalarType::Float32 => {
                                Some(std::any::TypeId::of::<[f32; 12]>())
                            }
                            _ => None,
                        },
                        (rows, cols) => None,
                    };

                    let Some(expected_type) = expected_type else {
                        log::error!(
                            "Slang matrix type with {}x{} elements is not supported.",
                            rows,
                            cols
                        );
                        break 'm;
                    };

                    let size = ty_layout.size(shader_slang::ParameterCategory::Uniform) as u32;
                    let offset = uniform_offset;
                    set.bindings.insert(
                        var_name,
                        (
                            ShaderBinding::Uniform {
                                expected_type,
                                size,
                                offset,
                            },
                            is_used_uniform,
                        ),
                    );
                }
                shader_slang::TypeKind::Array => {
                    let array_size = ty_layout.element_count().unwrap();

                    let element_ty_layout = ty_layout.element_type_layout().unwrap();
                    match element_ty_layout.kind() {
                        shader_slang::TypeKind::Resource => {
                            let binding_type = match element_ty_layout.resource_shape().unwrap() {
                                shader_slang::ResourceShape::SlangByteAddressBuffer
                                | shader_slang::ResourceShape::SlangStructuredBuffer => {
                                    ShaderBindingType::StorageBufferArray {
                                        size: array_size as u32,
                                    }
                                }
                                shader_slang::ResourceShape::SlangTexture2d => {
                                    match element_ty_layout.resource_access().unwrap() {
                                        shader_slang::ResourceAccess::Read => {
                                            ShaderBindingType::SampledImageArray {
                                                size: ty_layout.element_count().unwrap() as u32,
                                            }
                                        }
                                        shader_slang::ResourceAccess::Write => todo!(),
                                        shader_slang::ResourceAccess::ReadWrite => todo!(),
                                        _ => unreachable!(),
                                    }
                                }
                                res => {
                                    let texture_2d_array_shape =
                                        shader_slang::ResourceShape::SlangTexture2d as u32
                                            | shader_slang::ResourceShape::SlangTextureCombinedFlag
                                                as u32;
                                    if res as u32 == texture_2d_array_shape {
                                        todo!();
                                    }
                                    unreachable!(
                                        "Resource array of kind {:?} not implemented",
                                        res as u32
                                    );
                                }
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
                        shader_slang::TypeKind::SamplerState => {
                            set.bindings.insert(
                                var_name,
                                (
                                    ShaderBinding::Slot {
                                        binding_index: binding_offset,
                                        binding_type: ShaderBindingType::SamplerArray {
                                            size: array_size as u32,
                                        },
                                    },
                                    is_used_binding,
                                ),
                            );
                        }
                        kind => todo!("Implement kind {:?}", kind),
                    }
                }
                kind => {
                    warn!("Ignoring type with {:?}", kind);
                }
            }
        }

        fn process_global_scope(
            metadata: Option<&shader_slang::Metadata>,
            set: &mut ShaderSetBinding,
            var_layout: &shader_slang::reflection::VariableLayout,
            descriptor_slot_offset: Option<u32>,
        ) {
            let ty_layout = var_layout.type_layout().unwrap();
            // debug!(
            //     "Processing scope: {:?}, type: {:?}",
            //     var_layout.variable().name(),
            //     ty_layout.kind()
            // );

            for category_index in 0..var_layout.category_count() {
                let category = var_layout.category_by_index(category_index).unwrap();
                let offset = var_layout.offset(category);
                let space = var_layout.binding_space_with_category(category);
                let binding_space = var_layout.binding_space();
                // debug!(
                //     "Category: {:?}, offset: {}, space: {}, binding_space: {}, size: {}",
                //     category,
                //     offset,
                //     space,
                //     binding_space,
                //     ty_layout.size(category)
                // );
            }

            match ty_layout.kind() {
                shader_slang::TypeKind::ParameterBlock => {
                    let binding_space = var_layout.binding_space_with_category(
                        shader_slang::ParameterCategory::SubElementRegisterSpace,
                    );
                    //assert_eq!(
                    //    binding_space, set.set_index as usize,
                    //    "Set index should match the SubElementRegisterSpace binding space."
                    //);
                    assert!(descriptor_slot_offset.is_none());

                    process_global_scope(
                        metadata,
                        set,
                        ty_layout.element_var_layout().unwrap(),
                        descriptor_slot_offset,
                    );
                }
                shader_slang::TypeKind::ConstantBuffer => {
                    for category_index in 0..var_layout.category_count() {
                        let category = var_layout.category_by_index(category_index).unwrap();
                        let offset = var_layout.offset(category);
                        if category == shader_slang::ParameterCategory::Uniform {
                            debug!("Uniform Constant Buffer catorgory with ofset {}", offset);
                        } else if category == shader_slang::ParameterCategory::DescriptorTableSlot {
                            debug!(
                                "Uniform Constant Buffer catorgory slot slot with ofset {}",
                                offset
                            );
                        }
                    }
                    process_global_scope(
                        metadata,
                        set,
                        ty_layout.element_var_layout().unwrap(),
                        descriptor_slot_offset,
                    );
                }
                shader_slang::TypeKind::Struct => {
                    for category_index in 0..var_layout.category_count() {
                        let category = var_layout.category_by_index(category_index).unwrap();
                        if category == shader_slang::ParameterCategory::Uniform {
                            set.global_uniform_binding_index =
                                Some(var_layout.offset(category) as u32);
                            set.global_uniforms_size = ty_layout.size(category) as u32;
                        }
                    }

                    let slot_offset = var_layout
                        .offset(shader_slang::ParameterCategory::DescriptorTableSlot)
                        as u32;
                    let uniform_offset =
                        var_layout.offset(shader_slang::ParameterCategory::Uniform) as u32;

                    for field in ty_layout.fields() {
                        process_var_layout(
                            metadata,
                            set,
                            field,
                            String::new(),
                            slot_offset,
                            uniform_offset,
                            None,
                        );
                    }
                }
                _ => todo!(),
            }
        }

        let global_var_layout = program_layout.global_params_var_layout().unwrap();

        let global_type_layout = global_var_layout.type_layout().unwrap();
        match global_type_layout.kind() {
            // Module is considered a struct.
            shader_slang::TypeKind::Struct => {
                for global_field in global_type_layout.fields() {
                    let field_type = global_field.type_layout().unwrap();
                    assert_eq!(field_type.kind(), shader_slang::TypeKind::ParameterBlock);
                    let set_index = global_field
                        .offset(shader_slang::ParameterCategory::SubElementRegisterSpace)
                        + global_field.offset(shader_slang::ParameterCategory::RegisterSpace);
                    let set_param_block_name = global_field.variable().unwrap().name().unwrap();

                    let mut bindings = ShaderSetBinding {
                        name: set_param_block_name.to_owned(),
                        set_index: set_index as u32,
                        bindings: HashMap::new(),
                        global_uniform_binding_index: None,
                        global_uniforms_used: false,
                        global_uniforms_size: 0,
                    };
                    for category_index in 0..global_field.category_count() {
                        let category = global_field.category_by_index(category_index).unwrap();
                        if category == shader_slang::ParameterCategory::DescriptorTableSlot {
                            bindings.global_uniform_binding_index =
                                Some(global_field.offset(category) as u32);
                        }
                    }
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
            shader_slang::TypeKind::ConstantBuffer => panic!(
                "New code must have accidentally introducted a global non-opque variant such as a uint, maybe meant to add \"static\"?"
            ),
            type_kind => unreachable!(
                "Somehow got {:?} with name {:?}",
                type_kind,
                global_var_layout.semantic_name().unwrap_or("unknown")
            ),
        }

        set_bindings
    }

    fn reflect_pipeline_info(
        program_layout: &shader_slang::reflection::Shader,
        entry_point: &str,
    ) -> ShaderPipelineInfo {
        let entry_point = program_layout
            .find_entry_point_by_name(entry_point)
            .expect("Failed to reflect entry point.");
        let workgroup_size = (entry_point.stage() == shader_slang::Stage::Compute).then(|| {
            let size = entry_point.compute_thread_group_size();
            Vector3::new(size[0] as u32, size[1] as u32, size[2] as u32)
        });

        ShaderPipelineInfo { workgroup_size }
    }
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct ShaderPath {
    path: String,
}

impl ShaderPath {
    pub fn new_unchecked(path: String) -> Self {
        Self { path }
    }

    /// Path in the form of dir::file
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

        let path = "assets/shaders/".to_owned() + &relative_path + ".shader_slang";
        PathBuf::from_str(&path).unwrap()
    }

    pub fn module(&self) -> String {
        let split = self.path.split("::");
        split.last().unwrap().to_owned()
    }
}

pub struct Shader {
    code_blob: shader_slang::Blob,
    module_name: String,
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

    pub fn module_name(&self) -> &str {
        &self.module_name
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

#[derive(Hash, Clone, PartialEq, Eq, Debug)]
pub struct ShaderDesc {
    pub module: ShaderPath,
    pub entry_point_name: String,
}
