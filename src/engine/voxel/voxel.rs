use std::{
    collections::HashMap,
    ops::{Deref, DerefMut, Range},
};

use downcast::{downcast, Any};
use hecs::Bundle;
use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    common::color::{
        Color, ColorSpace, ColorSpaceSrgbLinear, ColorSpaceTransitionFrom, ColorSpaceTransitionInto,
    },
    engine::{graphics::device::DeviceResource, physics::transform::Transform},
};

use super::{
    attachment::{Attachment, AttachmentId, PTMaterial},
    esvo::VoxelModelESVO,
    flat::VoxelModelFlat,
    unit::VoxelModelUnit,
    voxel_allocator::VoxelAllocator,
    voxel_transform::VoxelModelTransform,
    voxel_world::VoxelModelId,
};

pub struct VoxelRange {
    /// Local position of the voxel model being edited, with origin at (-x, -y, -z).
    position: Vector3<u32>,
    data: VoxelRangeData,
}

pub enum VoxelRangeData {
    Unit(VoxelModelUnit),
    Flat(VoxelModelFlat),
}

impl VoxelRange {
    pub fn from_unit(position: Vector3<u32>, unit: impl Into<VoxelModelUnit>) -> Self {
        Self {
            position,
            data: VoxelRangeData::Unit(unit.into()),
        }
    }

    pub fn position(&self) -> Vector3<u32> {
        self.position
    }

    pub fn length(&self) -> Vector3<u32> {
        match &self.data {
            VoxelRangeData::Unit(_) => Vector3::new(1, 1, 1),
            VoxelRangeData::Flat(flat) => flat.length().clone(),
        }
    }

    pub fn data(&self) -> &VoxelRangeData {
        &self.data
    }
}

pub trait VoxelModelImpl: Send + Sync + Any {
    fn set_voxel_range_impl(&mut self, range: VoxelRange);
    fn set_voxel_range(&mut self, range: VoxelRange) {
        // Asserts that the range's position with its length fits within this voxel model.
        assert!(range
            .length()
            .zip_map(&(self.length() - range.position()), |x, y| x <= y)
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
        S: ColorSpace + ColorSpaceTransitionInto<ColorSpaceSrgbLinear>,
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
