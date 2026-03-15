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
use crate::{
    common::color::{
        Color, ColorSpace, ColorSpaceSrgb, ColorSpaceSrgbLinear, ColorSpaceTransitionFrom,
        ColorSpaceTransitionInto,
    },
    voxel::rvox_asset::RVOXAsset,
};

pub struct VoxelModelEdit {
    /// In local-coords.
    pub min: Vector3<u32>,
    pub max: Vector3<u32>,
    pub data: VoxelEditData,
}

#[derive(Clone)]
pub enum VoxelEditData {
    Fill {
        material: Option<VoxelMaterialData>,
    },
    Sphere {
        material: Option<VoxelMaterialData>,
        center: Vector3<i32>,
        radius: u32,
    },
}

pub struct VoxelModelTrace {
    pub local_position: Vector3<u32>,
    pub depth_t: f32,
}

pub struct MaterialPalette {
    palette: HashMap<u16, MaterialId>,
}

#[derive(Clone)]
pub enum VoxelMaterialData {
    Unbaked(MaterialId),
    Baked { color: Color<ColorSpaceSrgb> },
}

impl VoxelMaterialData {
    pub fn encode(&self) -> u32 {
        match self {
            VoxelMaterialData::Unbaked(free_list_handle) => {
                let material_id = free_list_handle.index() as u16;
                material_id as u32
            }
            VoxelMaterialData::Baked { color } => {
                let max = 2.0f32.powi(5) - 1.0;
                let r = (color.r() * max) as u32;
                let g = (color.g() * max) as u32;
                let b = (color.b() * max) as u32;
                0x4000_0000 | (r << 10) | (g << 5) | b
            }
        }
    }

    pub fn decode(encoded: u32) -> Self {
        if encoded & 0x4000_0000 == 0 {
            VoxelMaterialData::Unbaked(MaterialId::new((encoded & 0xFFFF) as u32, 0))
        } else {
            let r = ((encoded >> 10) & 0x1F) as f32 / 31.0;
            let g = ((encoded >> 5) & 0x1F) as f32 / 31.0;
            let b = (encoded & 0x1F) as f32 / 31.0;
            log::info!(
                "Decoded baked material {:b} with r: {}, g: {}, b: {}",
                encoded,
                r,
                g,
                b
            );
            VoxelMaterialData::Baked {
                color: Color::new_srgb(r, g, b),
            }
        }
    }
}

pub trait VoxelModelImpl: Clone + VoxelModelImplMethods {
    const NAME: &'static str;

    // Returns the local position of the hit voxel, if any.
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace>;

    fn set_voxel_range_impl(&mut self, range: &VoxelModelEdit) {
        panic!("Cannot set voxel range of this model type. ");
    }

    fn length(&self) -> Vector3<u32>;

    fn create_rvox_asset(&self) -> RVOXAsset {
        unimplemented!()
    }

    fn material_palette(&self) -> MaterialPalette {
        unimplemented!()
    }

    fn resize_model(&mut self, side_length: Vector3<u32>) {
        unimplemented!()
    }
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

    fn create_rvox_asset(&self) -> RVOXAsset;

    fn material_palette(&self) -> MaterialPalette;

    fn resize_model(&mut self, side_length: Vector3<u32>);
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

    fn create_rvox_asset(&self) -> RVOXAsset {
        VoxelModelImpl::create_rvox_asset(self)
    }

    fn material_palette(&self) -> MaterialPalette {
        VoxelModelImpl::material_palette(self)
    }

    fn resize_model(&mut self, side_length: Vector3<u32>) {
        VoxelModelImpl::resize_model(self, side_length)
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

    fn mark_for_invalidation(&mut self) {
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
