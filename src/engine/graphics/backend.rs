use core::panic;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::Hash,
    io::Write,
    mem::MaybeUninit,
    num::NonZeroU32,
    ops::Deref,
    sync::Arc,
    u32,
};

use downcast::Any;
use log::{debug, warn};
use nalgebra::{Vector2, Vector3};
use ron::to_string;
use serde::{Deserialize, Serialize};

use crate::{
    common::color::{Color, ColorSpaceSrgb},
    engine::{
        event::Events,
        graphics::shader::{ShaderBinding, ShaderBindingType},
        window::window::{Window, WindowHandle},
    },
};

use super::{
    frame_graph::{FrameGraph, FrameGraphContext, FrameGraphContextImpl},
    shader::{Shader, ShaderCompiler, ShaderPath, ShaderSetBinding},
};

pub enum GraphicsBackendEvent {
    Initialized,
    PipelineShaderUpdate(ResourceId<Untyped>),
}

pub trait GraphicsBackendDevice {
    /// Updates every window redraw request before recieving `GraphicsBackendEvent::Initialized`.
    fn pre_init_update(&mut self, events: &mut Events);

    /// Updates at the start of window redraw event.
    fn begin_frame(&mut self, events: &mut Events);

    fn swapchain_size(&self) -> Vector2<u32>;

    fn create_frame_graph_executor(&mut self) -> Box<dyn GraphicsBackendFrameGraphExecutor>;

    fn register_compute_pipeline(
        &mut self,
        create_info: GfxComputePipelineCreateInfo,
    ) -> ResourceId<ComputePipeline>;
    fn register_raster_pipeline(
        &mut self,
        create_info: GfxRasterPipelineCreateInfo,
    ) -> ResourceId<RasterPipeline>;

    fn create_image(&mut self, create_info: GfxImageCreateInfo) -> ResourceId<Image>;
    fn get_image_info(&self, image: &ResourceId<Image>) -> GfxImageInfo;
    fn write_image(&mut self, write_info: GfxImageWrite);

    fn create_buffer(&mut self, create_info: GfxBufferCreateInfo) -> ResourceId<Buffer>;
    fn write_buffer(&mut self, buffer: &ResourceId<Buffer>, offset: u64, size: u64) -> &mut [u8];
    fn write_buffer_slice(&mut self, buffer: &ResourceId<Buffer>, offset: u64, data: &[u8]) {
        self.write_buffer(buffer, offset, data.len() as u64)
            .copy_from_slice(data);
    }
    fn get_buffer_info(&self, buffer: &ResourceId<Buffer>) -> GfxBufferInfo;

    fn create_sampler(&mut self, create_info: GfxSamplerCreateInfo) -> ResourceId<Sampler>;

    fn end_frame(&mut self);

    fn update_pipelines(&mut self, shader_compiler: &ShaderCompiler) -> anyhow::Result<()>;

    fn acquire_swapchain_image(&mut self) -> anyhow::Result<ResourceId<Image>>;
    /// `skip_frame` is true if we are skipping rendering the current cpu, aka. next gpu frame.
    /// This helps the device with synchronization.
    fn resize_swapchain(
        &mut self,
        new_size: winit::dpi::PhysicalSize<NonZeroU32>,
        skip_frame: bool,
    );

    fn device_info(&self) -> GfxDeviceInfo;
}

pub trait GraphicsBackendRecorder {
    fn clear_color(&mut self, image: ResourceId<Image>, color: Color<ColorSpaceSrgb>);
    fn blit_full(&mut self, src: ResourceId<Image>, dst: ResourceId<Image>, filter: GfxFilterMode) {
        self.blit(GfxBlitInfo {
            src,
            src_offset: Vector2::zeros(),
            src_length: Vector2::new(u32::MAX, u32::MAX),
            dst,
            dst_offset: Vector2::zeros(),
            dst_length: Vector2::new(u32::MAX, u32::MAX),
            filter,
        });
    }
    // TODO: Support blitting specific image regions.
    fn blit(&mut self, info: GfxBlitInfo);
    fn begin_compute_pass(&mut self, compute_pipeline: ResourceId<ComputePipeline>) -> ComputePass;
    fn begin_render_pass(
        &mut self,
        raster_pipeline: ResourceId<RasterPipeline>,
        color_attachments: &[GfxRenderPassAttachment],
        depth_attachment: Option<GfxRenderPassAttachment>,
    ) -> RenderPass;

    fn get_image_info(&self, image: &ResourceId<Image>) -> GfxImageInfo;
}

pub trait GraphicsBackendComputePass {
    /// Expects all uniforms listed in the shader to be present in UniformData, however that
    /// doesn't mean the backend will constantly rebind the same descriptor set.
    fn bind_uniforms(&mut self, writer_fn: &mut dyn FnMut(&mut ShaderWriter));
    fn dispatch(&mut self, x: u32, y: u32, z: u32);
    fn workgroup_size(&self) -> Vector3<u32>;
}

pub trait GraphicsBackendRenderPass {
    /// Expects all uniforms listed in the shader to be present in UniformData, however that
    /// doesn't mean the backend will constantly rebind the same descriptor set.
    fn bind_uniforms(&mut self, writer_fn: &mut dyn FnMut(&mut ShaderWriter));
    fn bind_vertex_buffer(&mut self, vertex_buffer: ResourceId<Buffer>, offset: u64);
    fn bind_index_buffer(&mut self, index_buffer: ResourceId<Buffer>, offset: u64);
    fn set_scissor(&mut self, x: u32, y: u32, width: u32, height: u32);
    fn draw_indexed(&mut self, vertex_count: u32);
}

pub struct GfxDeviceInfo {
    pub max_allocation_size: u64,
    pub max_storage_buffer_size: u64,
    pub max_storage_buffer_array_binding_count: u64,
}

pub struct GfxBlitInfo {
    pub src: ResourceId<Image>,
    pub src_offset: Vector2<u32>,
    pub src_length: Vector2<u32>,
    pub dst: ResourceId<Image>,
    pub dst_offset: Vector2<u32>,
    pub dst_length: Vector2<u32>,
    pub filter: GfxFilterMode,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct GfxRenderPassAttachment {
    pub image: ResourceId<Image>,
    pub load_op: GfxLoadOp,
}

impl GfxRenderPassAttachment {
    pub fn new_clear(image: ResourceId<Image>) -> Self {
        Self {
            image,
            load_op: GfxLoadOp::Clear,
        }
    }

    pub fn new_load(image: ResourceId<Image>) -> Self {
        Self {
            image,
            load_op: GfxLoadOp::Load,
        }
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum GfxLoadOp {
    Clear,
    Load,
}

pub type ComputePass<'a> = Box<dyn GraphicsBackendComputePass + 'a>;
pub type RenderPass<'a> = Box<dyn GraphicsBackendRenderPass + 'a>;

pub trait GraphicsBackendSwapchain {
    /// Gets the next available image in the swapchain.
    fn get_next_image(&self);
    fn present(&mut self);
}

pub trait GraphicsBackendFrameGraphExecutor {
    fn begin_frame(&mut self, frame_graph: FrameGraph);
    fn end_frame(&mut self) -> FrameGraph;

    fn write_buffer(&mut self, name: &str, size: u64) -> &mut [u8];
    fn write_buffer_slice(&mut self, name: &str, data: &[u8]) {
        self.write_buffer(name, data.len() as u64)
            .copy_from_slice(data);
    }

    fn supply_image_ref(&mut self, name: &str, image: &ResourceId<Image>);
    fn supply_buffer_ref(&mut self, name: &str, buffer: &ResourceId<Buffer>);
    fn supply_pass_ref(&mut self, name: &str, pass: &mut dyn GfxPassOnceImpl<'_>);

    fn write_uniforms(&mut self, write_fn: &mut dyn FnMut(&mut ShaderWriter, &FrameGraphContext));

    fn supply_input(&mut self, name: &str, input_data: Box<dyn std::any::Any>);
}

pub trait GfxPassOnceImpl<'a> {
    fn run(&mut self, recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>);
}

impl<'a, F> GfxPassOnceImpl<'a> for F
where
    F: FnMut(&mut dyn GraphicsBackendRecorder, &FrameGraphContext) + 'a,
{
    fn run(&mut self, recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext) {
        self(recorder, ctx);
    }
}

pub struct BindGroup;
pub struct ComputePipeline;
pub struct RasterPipeline;
pub struct Untyped;
pub struct Image;
pub struct Sampler;
pub struct Buffer;
pub struct Memory;

pub struct ResourceId<T> {
    id: u32,
    _marker: std::marker::PhantomData<T>,
}

impl<T> std::fmt::Debug for ResourceId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceId")
            .field("id", &self.id)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T> std::fmt::Display for ResourceId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Resource id: {}, type_id: {}",
            self.id,
            std::any::type_name::<T>()
        ))
    }
}

impl<T> ResourceId<T> {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_untyped(&self) -> ResourceId<Untyped> {
        ResourceId {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_typed<S>(&self) -> ResourceId<S> {
        ResourceId {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

impl<T> std::hash::Hash for ResourceId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Eq for ResourceId<T> {}

impl<T> PartialEq for ResourceId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Copy for ResourceId<T> {}

impl<T> Clone for ResourceId<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GfxSwapchainInfo {
    pub present_mode: GfxPresentMode,
    /// May increase input-to-screen latency
    pub triple_buffering: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum GfxPresentMode {
    NoVsync,
    Vsync,
}

impl ResourceId<Image> {
    pub fn as_storage_binding(&self) -> Binding {
        Binding::StorageImage {
            image: self.clone(),
        }
    }

    pub fn as_sampled_binding(&self) -> Binding {
        Binding::SampledImage {
            image: self.clone(),
        }
    }

    pub fn info(&self, device: &dyn GraphicsBackendDevice) -> GfxImageInfo {
        device.get_image_info(self)
    }
}

impl ResourceId<Buffer> {
    pub fn as_uniform_binding(&self) -> Binding {
        Binding::UniformBuffer {
            buffer: self.clone(),
        }
    }

    pub fn as_storage_binding(&self) -> Binding {
        Binding::StorageBuffer {
            buffer: self.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GfxComputePipelineInfo {
    pub workgroup_size: Vector3<u32>,
    pub set_bindings: Vec<ShaderSetBinding>,
}

#[derive(PartialEq, Eq)]
pub struct ShaderWriter<'a> {
    shader_set_bindings: &'a [ShaderSetBinding],
    set_bindings: HashMap</*set_index=*/ u32, ShaderSetData>,
    global_writer: bool,
}

impl<'a> ShaderWriter<'a> {
    pub fn new(shader_set_bindings: &'a [ShaderSetBinding], global_writer: bool) -> Self {
        Self {
            shader_set_bindings,
            set_bindings: HashMap::new(),
            global_writer,
        }
    }

    pub fn take_set_data(&mut self) -> HashMap<u32, ShaderSetData> {
        std::mem::replace(&mut self.set_bindings, HashMap::new())
    }

    pub fn parse_uniform_name(
        full_uniform_name: &str,
    ) -> (/*set_name=*/ &str, /*param_path=*/ &str) {
        let separator_index = full_uniform_name
            .find(".")
            .expect("Invalid uniform path, must be {set}.{path}(.)?");
        let set_name = &full_uniform_name[0..separator_index];
        let param_path = &full_uniform_name[(separator_index + 1)..full_uniform_name.len()];
        (set_name, param_path)
    }

    pub fn use_set_cache(&mut self, set_name: impl ToString, cache_slot: u32) {
        let set_name = set_name.to_string();
        let set_info = self
            .shader_set_bindings
            .iter()
            .find(|set| set.name == set_name)
            .expect(&format!("No set with the name `{}` exists.", set_name,));
        let old = self
            .set_bindings
            .insert(set_info.set_index, ShaderSetData::CacheSlot(cache_slot));
        if old.is_some() {
            panic!(
                "Called use_set_cache() but set `{}` was written to prior which is not allowed.",
                set_name
            );
        }
    }

    pub fn write_set_cache(&mut self, set_name: impl ToString, cache_slot: u32) {
        let set_name = set_name.to_string();
        let set_info = self
            .shader_set_bindings
            .iter()
            .find(|set| set.name == set_name)
            .expect(&format!("No set with the name `{}` exists.", set_name,));
        let mut set = self
            .set_bindings
            .entry(set_info.set_index)
            .or_insert(ShaderSetData::new());
        let old_set_index = match set {
            ShaderSetData::Defined {
                bindings,
                uniform_data,
                cache_slot: slot,
            } => slot.replace(cache_slot),
            ShaderSetData::CacheSlot(_) => panic!("Set was defined as using the cache already."),
        };
        if old_set_index.is_some() {
            panic!(
                "Called use_set_cache() but set `{}` was written to prior which is not allowed.",
                set_name
            );
        }
    }

    pub fn write_binding<T: 'static>(
        &mut self,
        full_uniform_name: impl ToString,
        binding_resource: ResourceId<T>,
    ) {
        let full_uniform_name = full_uniform_name.to_string();
        let (set_name, param_path) = Self::parse_uniform_name(&full_uniform_name);

        let (set_index, binding_index, expected_type) = {
            let set_info = self
                .shader_set_bindings
                .iter()
                .find(|set| set.name == set_name)
                .expect(&format!(
                    "No set with the name `{}` exists for the submitted uniform `{}`",
                    set_name, full_uniform_name,
                ));

            let Some((binding_index, expected_type)) = set_info
                .bindings
                .iter()
                .find_map(|(path, (binding, is_used))| {
                    if path == param_path {
                        if !is_used {
                            warn!("Wrote uniform binding for `{}` but it is not used", full_uniform_name);
                            return None;
                        }

                        match binding {
                            ShaderBinding::Slot {
                                binding_index,
                                binding_type,
                            } => {
                                return Some((binding_index, binding_type));
                            },
                            ShaderBinding::Uniform {
                                ..
                            } => panic!("write_binding shouldn't be called when the expected type is a uniform for `{}`", full_uniform_name),
                        }
                    }

                    None
                }) else {
                    warn!(
                        "No uniform binding with the path `{}` exists for the submitted uniform `{}`",
                        param_path, full_uniform_name
                    );
                    return;
                };

            (set_info.set_index, *binding_index, *expected_type)
        };

        let binding = match expected_type {
            ShaderBindingType::Sampler => {
                assert_eq!(
                    std::any::TypeId::of::<T>(),
                    std::any::TypeId::of::<Sampler>(),
                    "Expected type for uniform `{}` is a Sampler, yet we recieved a ResourceId<{}>",
                    full_uniform_name,
                    std::any::type_name::<T>()
                );
                Binding::Sampler {
                    sampler: ResourceId::new(binding_resource.id()),
                }
            }
            ShaderBindingType::SampledImage => {
                assert_eq!(std::any::TypeId::of::<T>(), std::any::TypeId::of::<Image>(),
                    "Expected type for uniform `{}` is a SampledImage, yet we recieved a ResourceId<{}>", 
                    full_uniform_name,
                    std::any::type_name::<T>());
                Binding::SampledImage {
                    image: ResourceId::new(binding_resource.id()),
                }
            }
            ShaderBindingType::StorageImage => {
                assert_eq!(std::any::TypeId::of::<T>(), std::any::TypeId::of::<Image>(),
                    "Expected type for uniform `{}` is a StorageImage, yet we recieved a ResourceId<{}>", 
                    full_uniform_name,
                    std::any::type_name::<T>());
                Binding::StorageImage {
                    image: ResourceId::new(binding_resource.id()),
                }
            }
            ShaderBindingType::UniformBuffer => {
                assert_eq!(std::any::TypeId::of::<T>(), std::any::TypeId::of::<Buffer>(),
                    "Expected type for uniform `{}` is a ConstantBuffer, yet we recieved a ResourceId<{}>", 
                    full_uniform_name,
                    std::any::type_name::<T>());
                Binding::UniformBuffer {
                    buffer: ResourceId::new(binding_resource.id()),
                }
            }
            ShaderBindingType::StorageBuffer => {
                assert_eq!(std::any::TypeId::of::<T>(), std::any::TypeId::of::<Buffer>(),
                    "Expected type for uniform `{}` is a StorageBuffer, yet we recieved a ResourceId<{}>", 
                    full_uniform_name,
                    std::any::type_name::<T>());
                Binding::StorageBuffer {
                    buffer: ResourceId::new(binding_resource.id()),
                }
            }
        };

        let mut set = self
            .set_bindings
            .entry(set_index)
            .or_insert(ShaderSetData::new());
        if set.is_using_cache() {
            panic!("Uniform set `{}` was defined as using a cached set, but an attempt to write `{}` was made", set_name, full_uniform_name);
        }
        set.bindings_mut().insert(binding_index, binding);
    }

    pub fn write_uniform_mat4(
        &mut self,
        full_uniform_name: impl ToString,
        val: &nalgebra::Matrix4<f32>,
    ) {
        // Slang is by default row-major so we transpose first.
        let arr: [f32; 16] = val.transpose().as_slice().try_into().unwrap();
        self.write_uniform(full_uniform_name, arr);
    }

    pub fn write_uniform_mat3(
        &mut self,
        full_uniform_name: impl ToString,
        val: &nalgebra::Matrix3<f32>,
    ) {
        let val = val.transpose();
        let slice = val.as_slice();
        let mut arr: [f32; 12] = [0.0; 12];
        arr[0..3].copy_from_slice(&slice[0..3]);
        arr[4..7].copy_from_slice(&slice[3..6]);
        arr[8..11].copy_from_slice(&slice[6..9]);
        self.write_uniform(full_uniform_name, arr);
    }

    pub fn write_uniform<T: 'static>(&mut self, full_uniform_name: impl ToString, val: T) {
        let full_uniform_name = full_uniform_name.to_string();
        let (set_name, param_path) = Self::parse_uniform_name(&full_uniform_name);

        let set_info = self
            .shader_set_bindings
            .iter()
            .find(|set| set.name == set_name)
            .expect(&format!(
                "No set with the name `{}` exists for the submitted uniform `{}`",
                set_name, full_uniform_name,
            ));
        assert!(set_info.global_uniform_binding_index.is_some());

        let (expected_type, size, offset) = set_info
            .bindings
            .iter()
            .find_map(|(path, (binding, is_used))| {
                if path == param_path {
                        if !is_used {
                            warn!("Wrote uniform data for `{}` but it is not used", full_uniform_name);
                            return None;
                        }

                    match binding {
                        ShaderBinding::Slot {
                            ..
                        } => panic!("write_uniform shouldn't be called when the expected type is a binding for `{}`", full_uniform_name),
                        ShaderBinding::Uniform {
                            expected_type,
                            size,
                            offset,
                        } => {
                            return Some((*expected_type, *size, *offset));
                        },
                    }
                }

                None
            })
            .expect(&format!(
                "No binding with the path `{}` exists for the submitted uniform `{}`",
                param_path, full_uniform_name
            ));

        assert_eq!(expected_type, std::any::TypeId::of::<T>(), "Tried to write uniform with data type {}, but expected a different type for uniform `{}`", std::any::type_name::<T>(), full_uniform_name);
        assert_eq!(size, std::mem::size_of::<T>() as u32);

        let mut set = self
            .set_bindings
            .entry(set_info.set_index)
            .or_insert(ShaderSetData::new());
        if set.is_using_cache() {
            panic!("Uniform set `{}` was defined as using a cached set, but an attempt to write `{}` was made", set_name, full_uniform_name);
        }

        let UniformSetData {
            data,
            written_uniforms,
        } = &mut set.uniform_data_mut();
        let required_size = (offset + size) as usize;
        if data.len() < required_size {
            data.resize(required_size, 0);
        }

        // Safety: We resize data to the required size and we copy the correct number of
        // bytes since we assert above with `size`.
        unsafe {
            data[offset as usize..(offset + size) as usize].copy_from_slice(
                std::slice::from_raw_parts(
                    std::slice::from_ref(&val).as_ptr() as *const u8,
                    size as usize,
                ),
            );
        }
        written_uniforms.insert(param_path.to_owned());
    }

    pub fn validate(&self) {
        for set_info in self.shader_set_bindings {
            let Some(set) = self.set_bindings.get(&set_info.set_index) else {
                // We need to make sure we fill in all the shader uniforms if we are not writing
                // globally.
                // TODO: We can't check whether uniforms are used or not so ignore global shader
                // sets for now if we don't define them at all in `bind_uniforms`.
                if !self.global_writer && set_info.name != "u_frame" {
                    panic!(
                        "Set `{}` was not defined in the ShaderWriter, comparing against bindings {:?}.",
                        set_info.name,
                        self.shader_set_bindings
                    );
                }

                continue;
            };

            match set {
                ShaderSetData::Defined {
                    bindings,
                    uniform_data,
                    cache_slot,
                } => {
                    for (binding_name, (binding_info, is_used)) in &set_info.bindings {
                        if *is_used {
                            match &binding_info {
                                ShaderBinding::Slot {
                                    binding_index,
                                    binding_type,
                                } => {
                                    if !bindings.contains_key(&binding_index) {
                                        panic!("Uniform binding of type {:?} for `{}.{}` has not been set.", binding_type, set_info.name, binding_name);
                                    }
                                }
                                ShaderBinding::Uniform {
                                    expected_type,
                                    size,
                                    offset,
                                } => {
                                    if !uniform_data.written_uniforms.contains(binding_name) {
                                        panic!(
                                            "Uniform value for `{}.{}` has not been set.",
                                            set_info.name, binding_name
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                ShaderSetData::CacheSlot(_) => {}
            }
        }
    }
}

#[derive(PartialEq, Eq, Clone)]
pub enum ShaderSetData {
    Defined {
        /// Set bindings ordered by backend binding index.
        bindings: HashMap</*binding_index=*/ u32, Binding>,
        uniform_data: UniformSetData,
        cache_slot: Option<u32>,
    },
    CacheSlot(u32),
}

impl ShaderSetData {
    pub fn new() -> ShaderSetData {
        Self::Defined {
            bindings: HashMap::new(),
            uniform_data: UniformSetData::new(),
            cache_slot: None,
        }
    }

    pub fn bindings(&self) -> &HashMap<u32, Binding> {
        match self {
            ShaderSetData::Defined { bindings, .. } => bindings,
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't call bindings() on cached uniform set data.")
            }
        }
    }

    pub fn bindings_mut(&mut self) -> &mut HashMap<u32, Binding> {
        match self {
            ShaderSetData::Defined { bindings, .. } => bindings,
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't call bindings_mut() on cached uniform set data.")
            }
        }
    }

    pub fn take_bindings(&mut self) -> HashMap<u32, Binding> {
        match self {
            ShaderSetData::Defined {
                ref mut bindings, ..
            } => std::mem::replace(bindings, HashMap::new()),
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't call take_bindings() on cached uniform set data.")
            }
        }
    }

    pub fn set_bindings(&mut self, in_bindings: HashMap<u32, Binding>) {
        match self {
            ShaderSetData::Defined {
                ref mut bindings, ..
            } => *bindings = in_bindings,
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't call set_bindings() on cached uniform set data.")
            }
        }
    }

    pub fn take_uniform_data(&mut self) -> UniformSetData {
        match self {
            ShaderSetData::Defined {
                ref mut uniform_data,
                ..
            } => std::mem::replace(uniform_data, UniformSetData::new()),
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't take uniform data on a cached set.")
            }
        }
    }

    pub fn set_uniform_data(&mut self, data: UniformSetData) {
        match self {
            ShaderSetData::Defined {
                ref mut uniform_data,
                ..
            } => *uniform_data = data,
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't set uniform data on a cached set.")
            }
        }
    }

    pub fn uniform_data(&self) -> &UniformSetData {
        match self {
            ShaderSetData::Defined { uniform_data, .. } => uniform_data,
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't call bindings() on cached uniform set data.")
            }
        }
    }

    pub fn uniform_data_mut(&mut self) -> &mut UniformSetData {
        match self {
            ShaderSetData::Defined {
                ref mut uniform_data,
                ..
            } => uniform_data,
            ShaderSetData::CacheSlot(_) => {
                panic!("Shouldn't call bindings() on cached uniform set data.")
            }
        }
    }

    pub fn is_using_cache(&self) -> bool {
        match self {
            ShaderSetData::Defined { .. } => false,
            ShaderSetData::CacheSlot(_) => true,
        }
    }

    pub fn is_writing_cache(&self) -> bool {
        match self {
            ShaderSetData::Defined { cache_slot, .. } => cache_slot.is_some(),
            ShaderSetData::CacheSlot(_) => false,
        }
    }

    pub fn cache_slot(&self) -> Option<u32> {
        match self {
            ShaderSetData::Defined { cache_slot, .. } => *cache_slot,
            ShaderSetData::CacheSlot(slot_idx) => Some(*slot_idx),
        }
    }
}

#[derive(PartialEq, Eq, Clone)]
pub struct UniformSetData {
    pub data: Vec<u8>,
    pub written_uniforms: HashSet<String>,
}

impl UniformSetData {
    pub fn new() -> Self {
        UniformSetData {
            data: Vec::new(),
            written_uniforms: HashSet::new(),
        }
    }
}

pub trait BindingDataType {
    fn hash(&self, hasher: &mut dyn std::hash::Hasher);
    fn eq(&self, other: &dyn BindingDataType) -> bool;
    fn clone(&self) -> Box<dyn BindingDataType>;
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub enum Binding {
    StorageImage { image: ResourceId<Image> },
    SampledImage { image: ResourceId<Image> },
    Sampler { sampler: ResourceId<Sampler> },
    UniformBuffer { buffer: ResourceId<Buffer> },
    StorageBuffer { buffer: ResourceId<Buffer> },
}

pub struct GfxComputePipelineCreateInfo<'a> {
    pub shader: &'a Shader,
}

pub struct GfxImageWrite<'a> {
    pub image: ResourceId<Image>,
    pub data: &'a [u8],
    pub offset: Vector2<u32>,
    pub extent: Vector2<u32>,
}

#[derive(Clone)]
pub struct GfxRasterPipelineCreateInfo<'a> {
    pub vertex_shader: &'a Shader,
    pub fragment_shader: &'a Shader,

    pub cull_mode: GfxCullMode,
    pub front_face: GfxFrontFace,
    pub vertex_format: GfxVertexFormat,
    pub blend_state: GfxRasterPipelineBlendStateCreateInfo,

    pub color_formats: Vec<GfxImageFormat>,
    pub depth_format: Option<GfxImageFormat>,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct GfxVertexFormat {
    pub attributes: Vec<GfxVertexAttribute>,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct GfxVertexAttribute {
    pub format: GfxVertexAttributeFormat,
    pub location: u32,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum GfxVertexAttributeFormat {
    Float2,
    Float3,
    Uint,
}

impl GfxVertexAttributeFormat {
    pub fn byte_size(&self) -> u32 {
        match self {
            GfxVertexAttributeFormat::Float2 => 8,
            GfxVertexAttributeFormat::Float3 => 12,
            GfxVertexAttributeFormat::Uint => 4,
        }
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum GfxCullMode {
    None,
    Front,
    Back,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum GfxFrontFace {
    Clockwise,
    CounterClockwise,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct GfxRasterPipelineBlendStateCreateInfo {
    pub attachments: Vec<GfxRasterPipelineBlendStateAttachmentInfo>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct GfxRasterPipelineBlendStateAttachmentInfo {
    pub enable_blend: bool,
    pub src_color_blend_factor: GfxBlendFactor,
    pub dst_color_blend_factor: GfxBlendFactor,
    pub color_blend_op: GfxBlendOp,
    pub src_alpha_blend_factor: GfxBlendFactor,
    pub dst_alpha_blend_factor: GfxBlendFactor,
    pub alpha_blend_op: GfxBlendOp,
}

impl GfxRasterPipelineBlendStateAttachmentInfo {
    pub const fn additive() -> Self {
        Self {
            enable_blend: true,
            src_color_blend_factor: GfxBlendFactor::SrcAlpha,
            dst_color_blend_factor: GfxBlendFactor::One,
            color_blend_op: GfxBlendOp::Add,
            src_alpha_blend_factor: GfxBlendFactor::One,
            dst_alpha_blend_factor: GfxBlendFactor::Zero,
            alpha_blend_op: GfxBlendOp::Add,
        }
    }
    pub const fn one_minus_src_alpha() -> Self {
        Self {
            enable_blend: true,
            src_color_blend_factor: GfxBlendFactor::SrcAlpha,
            dst_color_blend_factor: GfxBlendFactor::OneMinusSrcAlpha,
            color_blend_op: GfxBlendOp::Add,
            src_alpha_blend_factor: GfxBlendFactor::One,
            dst_alpha_blend_factor: GfxBlendFactor::Zero,
            alpha_blend_op: GfxBlendOp::Add,
        }
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum GfxBlendFactor {
    Zero,
    One,
    OneMinusSrcAlpha,
    SrcColor,
    DstColor,
    SrcAlpha,
    DstAlpha,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum GfxBlendOp {
    Add,
    Subtract,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GfxBufferCreateInfo {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GfxSamplerCreateInfo {
    pub mag_filter: GfxFilterMode,
    pub min_filter: GfxFilterMode,
    pub mipmap_filter: GfxFilterMode,
    pub address_mode: GfxAddressMode,
}

#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
pub enum GfxAddressMode {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GfxImageCreateInfo {
    pub name: String,
    pub image_type: GfxImageType,
    pub format: GfxImageFormat,
    pub extent: Vector2<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum GfxImageType {
    D2,
    DepthD2,
    Cube,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum GfxFilterMode {
    Nearest,
    Linear,
}

pub struct GfxBufferInfo {
    pub size: u64,
}

pub struct GfxImageInfo {
    pub resolution: Vector3<u32>,
    pub format: GfxImageFormat,
}

impl GfxImageInfo {
    pub fn resolution_xy(&self) -> Vector2<u32> {
        self.resolution.xy()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum GfxImageFormat {
    R16Float,
    Rgba32Float,
    Rgba8Unorm,
    Rgba8Srgb,
    D16Unorm,
    D24UnormS8Uint,
    D32Float,
}
