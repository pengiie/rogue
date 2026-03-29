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
use crate::common::geometry::ray::Ray;
use crate::common::{color::ColorSrgba, geometry::aabb::AABB};
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

pub struct VoxelModelEditMaskModel<'a> {
    pub model: &'a dyn VoxelModelImplMethods,
    // Where the mask is relative to this models min corner.
    pub offset: Vector3<u32>,
}

pub struct VoxelModelEditMask<'a> {
    pub layers: Vec<VoxelModelEditMaskLayer>,
    /// None if the model uses itself as the mask.
    pub mask_model: Option<VoxelModelEditMaskModel<'a>>,
}

impl VoxelModelEditMask<'_> {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            mask_model: None,
        }
    }
}

#[derive(strum_macros::Display, Clone)]
pub enum VoxelModelEditMaskLayer {
    /// Apply the edit to only voxels that already exist.
    Presence,
    /// Apply the edit in a sphere.
    Sphere { center: Vector3<i32>, diameter: u32 },
}

impl VoxelModelEditMaskLayer {}

#[derive(Clone)]
pub enum VoxelModelEditRegion {
    Rect {
        min: Vector3<u32>,
        max: Vector3<u32>,
    },
    Intersect(Vec<VoxelModelEditRegion>),
}

impl VoxelModelEditRegion {
    pub fn with_intersect_rect(mut self, min: Vector3<u32>, max: Vector3<u32>) -> Self {
        match &mut self {
            VoxelModelEditRegion::Rect {
                min: min_s,
                max: max_s,
            } => {
                *min_s = min.zip_map(&min_s, |x, y| x.max(y));
                *max_s = max.zip_map(&max_s, |x, y| x.min(y));
            }
            VoxelModelEditRegion::Intersect(voxel_model_edit_regions) => todo!(),
        }
        self
    }
}

pub struct VoxelModelEdit<'a> {
    pub region: VoxelModelEditRegion,
    pub mask: VoxelModelEditMask<'a>,
    pub operator: VoxelModelEditOperator,
}

pub enum VoxelModelEditOperator {
    Replace(Option<VoxelMaterialData>),
}

pub struct VoxelModelTrace {
    pub local_position: Vector3<u32>,
    pub local_normal: Vector3<i32>,
    pub depth_t: f32,
}

pub struct MaterialPalette {
    palette: HashMap<u16, MaterialId>,
}

/// 64 bit material data, two halves:
/// Starting from MSB:
///
#[derive(Clone, strum_macros::EnumIs)]
pub enum VoxelMaterialData {
    Unbaked(u32),
    Baked { color: ColorSrgba },
}

impl VoxelMaterialData {
    /// Bakes material and normal.
    pub const NEEDS_MATERIAL_BAKE_FLAG: u64 = 0x8000_0000_0000_0000;
    /// Bakes normal.
    pub const NEED_NORMAL_FLAG: u64 = 0x4000_0000_0000_0000;
    pub fn encode(&self) -> u64 {
        match self {
            VoxelMaterialData::Unbaked(material_id) => {
                *material_id as u64 | Self::NEEDS_MATERIAL_BAKE_FLAG | Self::NEED_NORMAL_FLAG
            }
            VoxelMaterialData::Baked { color } => {
                let r = (color.r() * 255.0) as u64;
                let g = (color.g() * 255.0) as u64;
                let b = (color.b() * 255.0) as u64;
                let a = (color.a() * 255.0) as u64;
                let rgba = (r << 24) | (g << 16) | (b << 8) | a;
                Self::NEED_NORMAL_FLAG | rgba
            }
        }
    }

    pub fn decode(encoded: u64) -> Self {
        if (encoded & Self::NEEDS_MATERIAL_BAKE_FLAG) > 0 {
            VoxelMaterialData::Unbaked((encoded & 0xFFFF) as u32)
        } else {
            let rgba = encoded & 0xFFFF_FFFF;
            let a = (rgba & 0xFF) as f32 / 255.0;
            let b = ((rgba >> 8) & 0xFF) as f32 / 255.0;
            let g = ((rgba >> 16) & 0xFF) as f32 / 255.0;
            let r = ((rgba >> 24) & 0xFF) as f32 / 255.0;
            VoxelMaterialData::Baked {
                color: ColorSrgba::new(r, g, b, a),
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

    fn get_voxel(&self, position: Vector3<u32>) -> Option<VoxelMaterialData> {
        unimplemented!();
    }

    fn clear(&mut self) {
        unimplemented!()
    }

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

    fn in_bounds(&self, position: Vector3<i32>) -> bool {
        let length = self.length();
        return position.x >= 0
            && position.y >= 0
            && position.z >= 0
            && (position.x as u32) < length.x
            && (position.y as u32) < length.y
            && (position.z as u32) < length.z;
    }

    fn get_voxel(&self, position: Vector3<u32>) -> Option<VoxelMaterialData>;

    fn clear(&mut self);

    fn create_rvox_asset(&self) -> RVOXAsset;

    fn material_palette(&self) -> MaterialPalette;

    fn resize_model(&mut self, side_length: Vector3<u32>);
}

impl<T: VoxelModelImpl> VoxelModelImplMethods for T {
    fn trace(&self, ray: &Ray, aabb: &AABB) -> Option<VoxelModelTrace> {
        VoxelModelImpl::trace(self, ray, aabb)
    }

    fn get_voxel(&self, position: Vector3<u32>) -> Option<VoxelMaterialData> {
        VoxelModelImpl::get_voxel(self, position)
    }

    fn clear(&mut self) {
        VoxelModelImpl::clear(self)
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
