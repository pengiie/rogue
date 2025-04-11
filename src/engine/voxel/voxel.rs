use std::{
    collections::HashMap,
    ops::{Deref, DerefMut, Range},
};

use downcast::{downcast, Any};
use hecs::Bundle;
use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    common::{
        aabb::AABB,
        color::{
            Color, ColorSpace, ColorSpaceSrgb, ColorSpaceSrgbLinear, ColorSpaceTransitionFrom,
            ColorSpaceTransitionInto,
        },
        ray::Ray,
    },
    engine::{
        graphics::{
            device::{DeviceResource, GfxDevice},
            gpu_allocator::GpuBufferAllocator,
        },
        physics::transform::Transform,
    },
};

use super::{
    attachment::{Attachment, AttachmentId, PTMaterial},
    esvo::VoxelModelESVO,
    flat::VoxelModelFlat,
    unit::VoxelModelUnit,
    voxel_registry::VoxelModelId,
    voxel_transform::VoxelModelTransform,
};

pub struct VoxelModelRange {
    pub offset: Vector3<u32>,
    pub data: VoxelModelFlat,
}

pub trait VoxelModelImpl: Send + Sync + Any {
    // Returns the local voxel hit if it was hit.
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<Vector3<u32>>;

    fn set_voxel_range_impl(&mut self, range: &VoxelModelRange);
    fn set_voxel_range(&mut self, range: &VoxelModelRange) {
        // Asserts that the range's position with its length fits within this voxel model.
        assert!(range
            .data
            .length()
            .zip_map(&(self.length() - range.offset), |x, y| x <= y)
            .iter()
            .all(|x| *x));
        self.set_voxel_range_impl(range);
    }
    fn schema(&self) -> VoxelModelSchema;
    fn length(&self) -> Vector3<u32>;

    fn volume(&self) -> u64 {
        self.length().map(|x| x as u64).product()
    }
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

    /// Makes any necessary allocations for the model to work and returns true if the
    /// model info needs to be re-registered, i.e. model allocation pointers have changed.
    fn update_gpu_objects(
        &mut self,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool;

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut GpuBufferAllocator,
        model: &dyn VoxelModelImpl,
    );
}
pub trait VoxelModelGpuImplConcrete: VoxelModelGpuImpl {
    fn new() -> Self;
}

// pub struct VoxelModelGpuNone;
//
// impl VoxelModelGpuImpl for VoxelModelGpuNone {
//     fn aggregate_model_info(&self) -> Option<Vec<u32>> {
//         unimplemented!("This gpu model is not renderable.")
//     }
//
//     fn update_gpu_objects(
//         &mut self,
//         allocator: &mut GpuBufferAllocator,
//         model: &dyn VoxelModelImpl,
//     ) -> bool {
//         unimplemented!("This gpu model is not renderable.")
//     }
//
//     fn write_gpu_updates(
//         &mut self,
//         device: &mut ,
//         allocator: &mut GpuBufferAllocator,
//         model: &dyn VoxelModelImpl,
//     ) {
//         unimplemented!("This gpu model is not renderable.")
//     }
// }

pub type VoxelModelSchema = u32;

pub struct RenderableVoxelModelRef(pub VoxelModelId);

impl std::ops::Deref for RenderableVoxelModelRef {
    type Target = VoxelModelId;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Bundle)]
pub struct RenderableVoxelModel {
    pub transform: VoxelModelTransform,
    pub renderable_voxel_model_ref: RenderableVoxelModelRef,
}

impl RenderableVoxelModel {
    pub fn new(transform: VoxelModelTransform, voxel_model_id: VoxelModelId) -> Self {
        Self {
            transform,
            renderable_voxel_model_ref: RenderableVoxelModelRef(voxel_model_id),
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

    pub fn into_model_gpu(self) -> T {
        self.model_gpu
    }
}

#[derive(Debug)]
pub struct VoxelModel<T: VoxelModelImpl> {
    model: T,
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

    pub fn into_model(self) -> T {
        self.model
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

pub type OptionalVoxelData = Option<VoxelData>;

/// The data of a singular voxel, holds the attachments and values stored by the attachments.
/// Stored in a contiguous homogenous array to avoid cache misses due to pointer hopping with
/// something like a HashMap<Attachment, Vec<u32>>.
///
/// Data is encoded as, size of attachments, attachment ids, followed by attachment data in the
/// same order. This data will start at some default size supporting the most common attachment
/// needs and will have to perform a heap allocation for additional attachments.
pub struct VoxelData {
    // TODO: Actually implement that.
    data: HashMap<AttachmentId, Vec<u32>>,
}

impl VoxelData {
    const DEFAULT_CAPACITY: usize = 2;

    pub fn empty() -> Self {
        Self {
            data: HashMap::with_capacity(Self::DEFAULT_CAPACITY),
        }
    }

    pub fn with_diffuse<S>(mut self, albedo: Color<S>) -> Self
    where
        S: ColorSpace + ColorSpaceTransitionInto<ColorSpaceSrgb>,
    {
        let material = PTMaterial::diffuse(albedo.into_color_space());
        self.add_attachment(
            &Attachment::PTMATERIAL,
            &Attachment::encode_ptmaterial(&material),
        );
        self
    }

    pub fn with_normal(mut self, normal: Vector3<f32>) -> Self {
        self.add_attachment(&Attachment::NORMAL, &Attachment::encode_normal(&normal));
        self
    }

    pub fn with_emmisive(mut self, candela: u32) -> Self {
        self.add_attachment(&Attachment::EMMISIVE, &Attachment::encode_emmisive(candela));
        self
    }

    fn add_attachment<T: bytemuck::Pod>(&mut self, attachment: &Attachment, data: &T) {
        self.data.insert(
            attachment.id(),
            bytemuck::cast_slice(bytemuck::bytes_of(data)).to_vec(),
        );
    }

    pub fn iter(&self) -> impl Iterator<Item = (&AttachmentId, &[u32])> {
        self.data
            .iter()
            .map(|(attachment_id, data)| (attachment_id, data.as_slice()))
    }

    pub fn attachment_ids(&self) -> impl Iterator<Item = &AttachmentId> {
        self.data.keys()
    }
}
