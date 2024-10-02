use std::{
    collections::HashMap,
    ops::{Deref, DerefMut, Range},
};

use downcast::{downcast, Any};
use hecs::Bundle;
use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{common::aabb::AABB, engine::physics::transform::Transform};

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
    pub const ALBEDO: Attachment = Attachment {
        name: "albedo",
        size: 1,
        renderable_index: 0,
    };
    pub const COMPRESSED: Attachment = Attachment {
        name: "compressed",
        size: 1,
        renderable_index: 1,
    };

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn renderable_index(&self) -> u8 {
        self.renderable_index
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
    fn aggregate_model_info(&self) -> Vec<u32>;

    fn write_gpu_updates(&mut self, allocator: &mut VoxelAllocator, model: &dyn VoxelModelImpl);
}
pub trait VoxelModelGpuImplConcrete: VoxelModelGpuImpl {
    fn new() -> Self;
}

pub struct VoxelModelGpuNone;

impl VoxelModelGpuImpl for VoxelModelGpuNone {
    fn aggregate_model_info(&self) -> Vec<u32> {
        unimplemented!("This gpu model is not renderable.")
    }

    fn write_gpu_updates(&mut self, allocator: &mut VoxelAllocator, model: &dyn VoxelModelImpl) {
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
