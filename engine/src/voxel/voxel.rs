use std::{
    collections::HashMap,
    ops::{Deref, DerefMut, Range},
};

use downcast::{Any, downcast};
use nalgebra::Vector3;
use rogue_macros::Resource;

use super::{
    attachment::{Attachment, AttachmentId, PTMaterial},
    flat::VoxelModelFlat,
    voxel_allocator::VoxelDataAllocator,
    voxel_registry::VoxelModelId,
    voxel_transform::VoxelModelTransform,
};
use crate::common::color::{
    Color, ColorSpace, ColorSpaceSrgb, ColorSpaceSrgbLinear, ColorSpaceTransitionFrom,
    ColorSpaceTransitionInto,
};
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::ray::Ray;
use crate::graphics::{
    backend::{Buffer, GfxBufferCreateInfo, GraphicsBackendDevice, ResourceId},
    device::{DeviceResource, GfxDevice},
    gpu_allocator::GpuBufferAllocator,
};
use crate::material::MaterialId;
use crate::physics::transform::Transform;
use crate::physics::voxel_collider::{VoxelModelCollider, VoxelModelColliderData};

pub struct VoxelModelEdit {
    /// In local-coords.
    pub min: Vector3<u32>,
    pub max: Vector3<u32>,
    pub data: VoxelEditData,
}

#[derive(Clone)]
pub enum VoxelEditData {
    Fill { material: MaterialId },
}

pub struct VoxelModelTrace {
    pub local_position: Vector3<u32>,
    pub depth_t: f32,
}

pub trait VoxelModelImpl: VoxelModelImplMethods + Clone {
    const NAME: &'static str;

    // Returns the local position of the hit voxel, if any.
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace>;

    fn set_voxel_range_impl(&mut self, range: &VoxelModelEdit) {
        panic!("Cannot set voxel range of this model type. ");
    }

    fn length(&self) -> Vector3<u32>;
}

pub trait VoxelModelImplMethods: Send + Sync + Any {
    // Returns the local voxel hit if it was hit.
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace>;

    fn set_voxel_range_impl(&mut self, edit: &VoxelModelEdit);
    fn length(&self) -> Vector3<u32>;

    fn physics_model(&self) -> VoxelModelColliderData {
        unimplemented!()
    }

    fn volume(&self) -> u64 {
        self.length().map(|x| x as u64).product()
    }
}

impl<T: VoxelModelImpl> VoxelModelImplMethods for T {
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace> {
        VoxelModelImpl::trace(self, ray, aabb)
    }

    fn set_voxel_range_impl(&mut self, range: &VoxelModelEdit) {
        VoxelModelImpl::set_voxel_range_impl(self, range);
    }

    fn length(&self) -> Vector3<u32> {
        VoxelModelImpl::length(self)
    }
}

downcast!(dyn VoxelModelImplMethods);

/// Function for constructing a voxel model gpu impl.
pub type VoxelModelGpuConstructFnPtr = unsafe fn(/*dst_ptr: */ *mut u8);
pub trait VoxelModelGpuImpl: VoxelModelGpuImplMethods + Clone {
    const SCHEMA: u32;

    fn construct() -> Self;
}

pub trait VoxelModelGpuImplMethods: Send + Sync + Any {
    // Returns the pointers required to traverse this data structure.
    // Can encode other model specific data here as well.
    fn aggregate_model_info(&self) -> Option<Vec<u32>>;

    fn invalidate_material(&mut self) {
        unimplemented!()
    }

    /// Makes any necessary allocations for the model to work and returns true if the
    /// model info needs to be re-registered, i.e. model allocation pointers have changed.
    fn update_gpu_objects(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImplMethods,
    ) -> bool;

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImplMethods,
    );

    fn deallocate(&mut self, allocator: &mut VoxelDataAllocator);
}

downcast!(dyn VoxelModelGpuImplMethods);

#[derive(Clone)]
pub struct VoxelMaterialSet {
    data: Vec<u32>,
    name_map: HashMap<String, VoxelMaterialId>,
    // In u32s.
    material_size: u32,
}

impl VoxelMaterialSet {
    /// material_byte_size must be a multiple of 4.
    pub fn new(material_byte_size: u32) -> Self {
        assert_eq!(
            material_byte_size % 4,
            0,
            "material_byte_size must be a multiple of 4"
        );
        assert!(material_byte_size > 0,);
        Self {
            data: Vec::new(),
            name_map: HashMap::new(),
            material_size: material_byte_size / 4,
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
