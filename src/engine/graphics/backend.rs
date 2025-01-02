use std::{collections::HashMap, future::Future, mem::MaybeUninit, num::NonZeroU32, sync::Arc};

use nalgebra::{Vector2, Vector3};
use serde::{Deserialize, Serialize};

use crate::{
    common::color::{Color, ColorSpaceSrgb},
    engine::{
        event::Events,
        window::window::{Window, WindowHandle},
    },
};

use super::{
    frame_graph::{FrameGraph, FrameGraphContextImpl},
    shader::{ShaderCompiler, ShaderPath, ShaderSetBinding},
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

    fn create_frame_graph_executor(&mut self) -> Box<dyn GraphicsBackendFrameGraphExecutor>;

    fn register_compute_pipeline(
        &mut self,
        create_info: GfxComputePipelineCreateInfo,
    ) -> ResourceId<ComputePipeline>;
    fn register_raster_pipeline(
        &mut self,
        create_info: RasterPipelineCreateInfo,
    ) -> ResourceId<RasterPipeline>;

    fn create_image(&mut self, create_info: GfxImageCreateInfo) -> ResourceId<Image>;
    fn get_image_info(&self, image: &ResourceId<Image>) -> GfxImageInfo;

    fn create_buffer(&mut self, create_info: GfxBufferCreateInfo) -> ResourceId<Buffer>;

    fn write_buffer(&mut self, buffer: &ResourceId<Buffer>, offset: u64, bytes: &[u8]);

    fn end_frame(&mut self);

    fn update_pipelines(&mut self, shader_compiler: &ShaderCompiler) -> anyhow::Result<()>;

    fn acquire_swapchain_image(&mut self) -> anyhow::Result<ResourceId<Image>>;
    fn resize_swapchain(&mut self, new_size: winit::dpi::PhysicalSize<NonZeroU32>);
}

pub trait GraphicsBackendRecorder {
    fn clear_color(&mut self, image: ResourceId<Image>, color: Color<ColorSpaceSrgb>);
    fn blit_full(&mut self, src: ResourceId<Image>, dst: ResourceId<Image>, filter: GfxFilterMode) {
        self.blit(src, dst, filter);
    }
    // TODO: Support blitting specific image regions.
    fn blit(&mut self, src: ResourceId<Image>, dst: ResourceId<Image>, filter: GfxFilterMode);
    fn begin_compute_pass(&mut self, compute_pipeline: ResourceId<ComputePipeline>) -> ComputePass;
}

pub trait GraphicsBackendComputePass {
    /// Expects all uniforms listed in the shader to be present in UniformData, however that
    /// doesn't mean the backend will constantly rebind the same descriptor set.
    fn bind_uniforms(&mut self, uniform_data: UniformData);
    fn dispatch(&mut self, x: u32, y: u32, z: u32);
    fn workgroup_size(&self) -> Vector3<u32>;
}

pub type ComputePass<'a> = Box<dyn GraphicsBackendComputePass + 'a>;

pub trait GraphicsBackendSwapchain {
    /// Gets the next available image in the swapchain.
    fn get_next_image(&self);
    fn present(&mut self);
}

pub trait GraphicsBackendPipelineManager {}

pub trait GraphicsBackendFrameGraphExecutor {
    fn begin_frame(&mut self, shader_compiler: &mut ShaderCompiler, frame_graph: FrameGraph);
    fn end_frame(&mut self) -> FrameGraph;

    fn supply_image_ref(&mut self, name: &str, image: &ResourceId<Image>);
    fn supply_pass_ref(&mut self, name: &str, pass: Box<dyn GfxPassOnceImpl>);
}

pub trait GfxPassOnceImpl {
    fn run(&mut self, recorder: &mut dyn GraphicsBackendRecorder, ctx: &dyn FrameGraphContextImpl);
}

pub struct BindGroup;
pub struct ComputePipeline;
pub struct RasterPipeline;
pub struct Untyped;
pub struct Image;
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
        Binding::Image {
            image: self.clone(),
        }
    }

    pub fn info(&self, device: &dyn GraphicsBackendDevice) -> GfxImageInfo {
        device.get_image_info(self)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GfxComputePipelineInfo {
    pub workgroup_size: Vector3<u32>,
    pub set_bindings: Vec<ShaderSetBinding>,
}

#[derive(PartialEq, Eq)]
pub struct UniformData {
    data: HashMap<String, Binding>,
}

impl UniformData {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    pub fn load(&mut self, full_uniform_name: impl ToString, binding: Binding) {
        self.data.insert(full_uniform_name.to_string(), binding);
    }

    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.data.values()
    }

    /// Requires the passed in set bindings to be sorted by set index, and returns a vec sorted by
    /// set index with the set bindings also sorted by binding index.
    pub fn as_sets(&self, shader_set_bindings: &Vec<ShaderSetBinding>) -> Vec<UniformSetData> {
        assert!(shader_set_bindings.is_sorted_by_key(|set| set.set_index));
        let mut buckets = shader_set_bindings
            .iter()
            .map(|set| (0..set.bindings.len()).map(|_| None).collect())
            .collect::<Vec<Vec<Option<Binding>>>>();

        for (full_uniform_name, binding) in &self.data {
            let parts = full_uniform_name.split(".").collect::<Vec<_>>();
            let Some((set_index, binding_index)) =
                shader_set_bindings
                    .iter()
                    .enumerate()
                    .find_map(|(set_idx, set)| {
                        if set.name != parts[0] {
                            return None;
                        }

                        let binding_idx =
                            set.bindings
                                .iter()
                                .enumerate()
                                .find_map(|(i, shader_binding)| {
                                    (shader_binding.binding_name == parts[1]).then_some(i)
                                });

                        binding_idx.map(|binding_idx| (set_idx, binding_idx))
                    })
            else {
                panic!("Loaded uniform `{}` but a uniform with that qualified name does not exist in the target shader.", full_uniform_name);
            };

            buckets[set_index][binding_index] = Some(binding.clone());
        }

        // Safety: We ensure each uniforms data has been provided for the given sets.
        buckets
            .into_iter()
            .enumerate()
            .map(|(set_idx, bucket)| UniformSetData {
                data: bucket
                    .into_iter()
                    .enumerate()
                    .map(|(binding_idx, binding)| {
                        binding.expect(&format!(
                            "Uniform for set {}, binding {} was not set.",
                            set_idx, binding_idx
                        ))
                    })
                    .collect(),
            })
            .collect()
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct UniformSetData {
    /// Set bindings ordered by backend binding index.
    pub data: Vec<Binding>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Binding {
    Image { image: ResourceId<Image> },
    Sampler {},
    Buffer {},
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct GfxComputePipelineCreateInfo {
    pub shader_path: ShaderPath,
    pub entry_point_fn: String,
}

pub struct RasterPipelineCreateInfo {}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GfxBufferCreateInfo {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GfxImageCreateInfo {
    pub name: String,
    pub image_type: GfxImageType,
    pub format: ImageFormat,
    pub extent: Vector2<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum GfxImageType {
    D2,
    DepthD2,
    Cube,
}

#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum GfxFilterMode {
    Nearest,
    Linear,
}

pub struct GfxImageInfo {
    resolution: Vector3<u32>,
}

impl GfxImageInfo {
    pub fn resolution_xy(&self) -> Vector2<u32> {
        self.resolution.xy()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ImageFormat {
    Rgba32Float,
    Rgba8Unorm,
    D16Unorm,
}
