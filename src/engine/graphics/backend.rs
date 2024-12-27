use std::{collections::HashMap, future::Future, num::NonZeroU32, sync::Arc};

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
    frame_graph::FrameGraph,
    shader::{ShaderCompiler, ShaderPath},
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
        create_info: ComputePipelineCreateInfo,
    ) -> ResourceId<ComputePipeline>;
    fn register_raster_pipeline(
        &mut self,
        create_info: RasterPipelineCreateInfo,
    ) -> ResourceId<RasterPipeline>;

    fn create_image(&mut self, create_info: ImageCreateInfo) -> ResourceId<Image>;
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
    fn blit(&mut self, src: ResourceId<Image>, dst: ResourceId<Image>);
    fn begin_compute_pass(&mut self, compute_pipeline: ResourceId<ComputePipeline>) -> ComputePass;
}

pub trait GraphicsBackendComputePass {
    fn bind_uniforms(&mut self, bindings: HashMap<&str, Binding>);
    fn dispatch(&mut self, x: u32, y: u32, z: u32);
}

pub type ComputePass = Box<dyn GraphicsBackendComputePass>;

pub trait GraphicsBackendSwapchain {
    /// Gets the next available image in the swapchain.
    fn get_next_image(&self);
    fn present(&mut self);
}

pub trait GraphicsBackendPipelineManager {}

pub trait GraphicsBackendFrameGraphExecutor {
    fn begin_frame(&mut self, frame_graph: FrameGraph);
    fn end_frame(&mut self) -> FrameGraph;

    fn supply_image_ref(&mut self, name: &str, image: &ResourceId<Image>);
}

pub struct ComputePipeline;
pub struct RasterPipeline;
pub struct Untyped;
pub struct Image;
pub struct Buffer;
pub struct Memory;

#[derive(Debug)]
pub struct ResourceId<T> {
    id: u32,
    _marker: std::marker::PhantomData<T>,
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
    pub fn as_binding(&self) -> Binding {
        Binding::Image {
            image: self.clone(),
        }
    }

    pub fn info(&self, device: &dyn GraphicsBackendDevice) -> GfxImageInfo {
        device.get_image_info(self)
    }
}

pub trait HasDeviceContext {}

pub enum Binding {
    Image { image: ResourceId<Image> },
    Sampler {},
    Buffer {},
}

pub struct ComputePipelineCreateInfo {
    shader_path: ShaderPath,
    entry_point_fn: String,
}

pub struct RasterPipelineCreateInfo {}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImageCreateInfo {}

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GfxImageType {
    D2,
    DepthD2,
    Cube,
}

pub struct GfxImageInfo {
    resolution: Vector3<u32>,
}

impl GfxImageInfo {
    pub fn resolution_xy(&self) -> Vector2<u32> {
        self.resolution.xy()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageFormat {
    Rgba8Unorm,
}
