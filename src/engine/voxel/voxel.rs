use std::{
    collections::HashMap,
    ops::{Deref, DerefMut, Range},
};

use downcast::{downcast, Any};
use hecs::Bundle;
use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    common::aabb::AABB,
    engine::{graphics::device::DeviceResource, physics::transform::Transform},
};

use super::{esvo::VoxelModelESVO, flat::VoxelModelFlat, voxel_allocator::VoxelAllocator};

pub struct VoxelRange {
    pub data: VoxelModelFlat,
    position: Vector3<u32>,
    length: Vector3<u32>,
}

impl VoxelRange {
    pub fn new(data: VoxelModelFlat, position: Vector3<u32>, length: Vector3<u32>) -> Self {
        Self {
            data,
            position,
            length,
        }
    }

    pub fn position(&self) -> Vector3<u32> {
        self.position
    }

    pub fn length(&self) -> Vector3<u32> {
        self.length
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Attachment {
    name: &'static str,

    // Size in terms of u32s
    size: u32,
    renderable_index: u8,
}

impl Attachment {
    pub const ALBEDO_RENDER_INDEX: u8 = 0;
    pub const ALBEDO: Attachment = Attachment {
        name: "albedo",
        size: 1,
        renderable_index: Self::ALBEDO_RENDER_INDEX,
    };
    pub const NORMAL_RENDER_INDEX: u8 = 1;
    pub const NORMAL: Attachment = Attachment {
        name: "normal",
        size: 1,
        renderable_index: Self::NORMAL_RENDER_INDEX,
    };

    pub const MAX_RENDER_INDEX: u8 = 2;

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn renderable_index(&self) -> u8 {
        self.renderable_index
    }

    pub fn encode_albedo(r: f32, g: f32, b: f32, a: f32) -> u32 {
        assert!(r >= 0.0 && r <= 1.0);
        assert!(g >= 0.0 && g <= 1.0);
        assert!(b >= 0.0 && b <= 1.0);
        assert!(a >= 0.0 && a <= 1.0);

        (((r * 255.0).floor() as u32) << 24)
            | (((g * 255.0).floor() as u32) << 16)
            | (((b * 255.0).floor() as u32) << 8)
            | (a * 255.0).floor() as u32
    }

    pub fn decode_albedo(albedo: u32) -> (f32, f32, f32, f32) {
        let r = (albedo >> 24) as f32 / 255.0;
        let g = ((albedo >> 16) & 0xFF) as f32 / 255.0;
        let b = ((albedo >> 8) & 0xFF) as f32 / 255.0;
        let a = (albedo & 0xFF) as f32 / 255.0;

        (r, g, b, a)
    }

    pub fn encode_normal(normal: Vector3<f32>) -> u32 {
        assert!(normal.norm() == 1.0);

        let mut x = 0u32;
        x |= (((normal.x * 0.5 + 0.5) * 255.0).ceil() as u32) << 16;
        x |= (((normal.y * 0.5 + 0.5) * 255.0).ceil() as u32) << 8;
        x |= ((normal.z * 0.5 + 0.5) * 255.0).ceil() as u32;

        x
    }

    pub fn decode_normal(normal: u32) -> Vector3<f32> {
        let x = (((normal >> 16) & 0xFF) as f32 / 255.0) * 2.0 - 1.0;
        let y = (((normal >> 8) & 0xFF) as f32 / 255.0) * 2.0 - 1.0;
        let z = ((normal & 0xFF) as f32 / 255.0) * 2.0 - 1.0;

        Vector3::new(x, y, z)
    }
}

pub trait VoxelModelImpl: Send + Sync + Any {
    fn set_voxel_range(&mut self, range: VoxelRange);
    fn schema(&self) -> VoxelModelSchema;
    fn length(&self) -> Vector3<u32>;
}
downcast!(dyn VoxelModelImpl);

pub trait VoxelModelImplConcrete: VoxelModelImpl + Clone {
    /// The corresponding gpu management of this voxel model.
    type Gpu: VoxelModelGpuImplConcrete;
}

pub trait VoxelModelGpuImpl: Send + Sync {
    // Returns the pointers required to traverse this data structure.
    // Can encode other model specific data here as well.
    fn aggregate_model_info(&self) -> Option<Vec<u32>>;

    fn update_gpu_objects(&mut self, allocator: &mut VoxelAllocator, model: &dyn VoxelModelImpl);

    fn write_gpu_updates(
        &mut self,
        device: &DeviceResource,
        allocator: &mut VoxelAllocator,
        model: &dyn VoxelModelImpl,
    );
}
pub trait VoxelModelGpuImplConcrete: VoxelModelGpuImpl {
    fn new() -> Self;
}

pub struct VoxelModelGpuNone;

impl VoxelModelGpuImpl for VoxelModelGpuNone {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        unimplemented!("This gpu model is not renderable.")
    }

    fn update_gpu_objects(&mut self, allocator: &mut VoxelAllocator, model: &dyn VoxelModelImpl) {
        unimplemented!("This gpu model is not renderable.")
    }

    fn write_gpu_updates(
        &mut self,
        device: &DeviceResource,
        allocator: &mut VoxelAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        unimplemented!("This gpu model is not renderable.")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoxelModelSchema {
    ESVO = 1,
}

#[derive(Bundle)]
pub struct RenderableVoxelModel<T: VoxelModelImplConcrete> {
    pub transform: Transform,
    pub voxel_model: VoxelModel<T>,
    voxel_model_gpu: VoxelModelGpu<T::Gpu>,
}

impl<T> RenderableVoxelModel<T>
where
    T: VoxelModelImplConcrete,
{
    pub fn new(transform: Transform, voxel_model: impl Into<VoxelModel<T>>) -> Self {
        Self {
            transform,
            voxel_model: voxel_model.into(),
            voxel_model_gpu: VoxelModelGpu::new(T::Gpu::new()),
        }
    }
}

pub struct VoxelModelGpu<T: VoxelModelGpuImplConcrete> {
    model_gpu: T,
}

impl<T> VoxelModelGpu<T>
where
    T: VoxelModelGpuImplConcrete,
{
    pub fn new(model_gpu: T) -> Self {
        Self { model_gpu }
    }
}

#[derive(Debug)]
pub struct VoxelModel<T: VoxelModelImpl> {
    model: T,
}

// Create voxel model type iter generator macro under rogue_macros.
//
// Must be a proc macro because we need to iterate over component and generate param names for the
// intermediate conversion to the &dyn VoxelModelImpl.
macro_rules! voxel_model_iter {
    ($model_type:ty, $ecs_world:expr, $( $component:ty),*) => {
        let q = ecs_world.query::<(&m, $($component),*)>();
        let i = q.into_iter().map(|entity, (model, )|)
    };
}

macro_rules! query_voxel_models {
    ($ecs_world:expr, $( $component:ty),*) => {};
}

impl<T> VoxelModel<T>
where
    T: VoxelModelImpl,
{
    pub fn new(model: T) -> Self {
        Self { model }
    }

    pub fn length(&self) -> Vector3<u32> {
        self.model.length()
    }
}

impl<T> Clone for VoxelModel<T>
where
    T: VoxelModelImplConcrete,
{
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
        }
    }
}

impl<T> Deref for VoxelModel<T>
where
    T: VoxelModelImpl,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl<T> DerefMut for VoxelModel<T>
where
    T: VoxelModelImpl,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.model
    }
}

impl<T> Deref for VoxelModelGpu<T>
where
    T: VoxelModelGpuImplConcrete,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.model_gpu
    }
}

impl<T> DerefMut for VoxelModelGpu<T>
where
    T: VoxelModelGpuImplConcrete,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.model_gpu
    }
}
