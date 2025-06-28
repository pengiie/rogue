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
            backend::{Buffer, GfxBufferCreateInfo, GraphicsBackendDevice, ResourceId},
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
    voxel_registry::VoxelModelId,
    voxel_transform::VoxelModelTransform,
    voxel_world::VoxelModelFlatEdit,
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum VoxelModelType {
    Flat,
    THC,
    THCCompressed,
}

impl AsRef<str> for VoxelModelType {
    fn as_ref(&self) -> &str {
        match self {
            VoxelModelType::Flat => "Flat",
            VoxelModelType::THC => "THC",
            VoxelModelType::THCCompressed => "THC (Compressed)",
        }
    }
}

impl ToString for VoxelModelType {
    fn to_string(&self) -> String {
        self.as_ref().to_string()
    }
}

pub struct VoxelModelEdit {
    pub offset: Vector3<u32>,
    pub data: VoxelModelFlatEdit,
}

pub struct VoxelModelTrace {
    pub local_position: Vector3<u32>,
    pub depth_t: f32,
}

pub trait VoxelModelImpl: Send + Sync + Any {
    // Returns the local voxel hit if it was hit.
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace>;

    fn set_voxel_range_impl(&mut self, range: &VoxelModelEdit);
    fn set_voxel_range(&mut self, range: &VoxelModelEdit) {
        // Asserts that the range's position with its length fits within this voxel model.
        assert!(range
            .data
            .side_length()
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
downcast!(dyn VoxelModelGpuImpl);

pub trait VoxelModelImplConcrete: VoxelModelImpl + Clone {
    /// The corresponding gpu management of this voxel model.
    type Gpu: VoxelModelGpuImplConcrete;

    fn model_type() -> Option<VoxelModelType> {
        None
    }
}

pub trait VoxelModelGpuImpl: Send + Sync + Any {
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

pub type VoxelModelSchema = u32;

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
    pub model: T,
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

#[derive(Clone)]
pub struct VoxelMaterialSet {
    data: Vec<u32>,
    name_map: HashMap<String, VoxelMaterialId>,
    // In u32s.
    material_size: u32,
}

impl VoxelMaterialSet {
    /// material_byte_size must be a multiple of 32.
    pub fn new(material_byte_size: u32) -> Self {
        assert_eq!(
            material_byte_size % 32,
            0,
            "material_byte_size must be a multiple of 32"
        );
        Self {
            data: Vec::new(),
            name_map: HashMap::new(),
            material_size: material_byte_size / 32,
        }
    }

    pub fn register_material(
        &mut self,
        name: Option<impl ToString>,
        data: &[u32],
    ) -> VoxelMaterialId {
        assert_eq!(data.len(), self.material_size as usize);
        let id = VoxelMaterialId(self.data.len() as u32 / self.material_size);
        if let Some(name) = name {
            let old = self.name_map.insert(name.to_string(), id);
            assert!(
                old.is_none(),
                "Overwrote previous material with same name, use replace_material instead.",
            );
        }
        self.data.extend_from_slice(data);

        return id;
    }

    pub fn replace_material(&mut self, id: VoxelMaterialId, data: &[u32]) {
        assert_eq!(data.len(), self.material_size as usize);
        let start = (id.0 * self.material_size) as usize;
        self.data[start..(start + self.material_size as usize)].copy_from_slice(data);
    }

    pub fn replace_material_with_name(&mut self, name: impl AsRef<str>, data: &[u32]) {
        assert_eq!(data.len(), self.material_size as usize);
        let id = self
            .name_map
            .get(name.as_ref())
            .expect("Material doesn't exist");
        let start = (id.0 * self.material_size) as usize;
        self.data[start..(start + self.material_size as usize)].copy_from_slice(data);
    }
}

#[derive(Clone, Copy)]
pub struct VoxelMaterialId(pub u32);

impl VoxelMaterialId {}

/// Buffer backed version of a VoxelMaterialSet, for an allocation
/// backed version, use VoxelMaterialSetAllocatedGpu.
pub struct VoxelMaterialSetGpu {
    name: String,
    material_data: Option<ResourceId<Buffer>>,
}

impl VoxelMaterialSetGpu {
    pub fn new(name: impl ToString) -> Self {
        Self {
            name: name.to_string(),
            material_data: None,
        }
    }

    pub fn update_gpu_objects(
        &mut self,
        material_data: &VoxelMaterialSet,
        device: &mut impl GraphicsBackendDevice,
    ) {
        let req_bytes = material_data.data.len() as u64 * 4;
        if req_bytes > 0 {
            match &mut self.material_data {
                Some(buffer) => {
                    let buffer_info = device.get_buffer_info(buffer);
                    if buffer_info.size < req_bytes {
                        // TODO: Delete old buffer.
                        let new_size =
                            req_bytes.max((buffer_info.size as f32 * 1.5).floor() as u64);
                        *buffer = device.create_buffer(GfxBufferCreateInfo {
                            name: format!("material_set_{}", self.name),
                            size: new_size,
                        });
                    }
                }
                None => {
                    const INITIAL_BYTES: u64 = 4 * 256;
                    self.material_data = Some(device.create_buffer(GfxBufferCreateInfo {
                        name: format!("material_set_{}", self.name),
                        size: INITIAL_BYTES,
                    }));
                }
            }
        }
    }

    pub fn write_render_data(&mut self, material_data: &VoxelMaterialSet) {}
}
