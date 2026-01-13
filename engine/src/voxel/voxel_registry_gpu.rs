use std::any::TypeId;
use std::collections::HashMap;

use rogue_macros::Resource;

use crate::graphics::backend::{Buffer, ResourceId};
use crate::graphics::device::DeviceResource;
use crate::graphics::gpu_allocator::GpuBufferAllocator;
use crate::resource::ResMut;
use crate::voxel::sft_compressed::VoxelModelSFTCompressed;
use crate::voxel::sft_compressed_gpu::VoxelModelSFTCompressedGpu;
use crate::voxel::voxel::{VoxelModelGpuImpl, VoxelModelImpl};
use crate::voxel::{
    voxel::VoxelModelGpuImplMethods, voxel_allocator::VoxelDataAllocator,
    voxel_registry::VoxelModelId,
};

/// Handles allocating and uploading voxel model data to the gpu and creating
/// gpu-based voxel model handles.
#[derive(Resource)]
pub struct VoxelModelRegistryGpu {
    gpu_models: HashMap<VoxelModelId, Box<dyn VoxelModelGpuImplMethods>>,
    gpu_model_construct_fns:
        HashMap</*VoxelModelType*/ TypeId, fn() -> Box<dyn VoxelModelGpuImplMethods>>,

    /// The buffer for every unique voxel models info including for entities and terrain.
    /// The info includes the models type-specific descriptor with its associated length.
    voxel_model_info_allocator: GpuBufferAllocator,

    /// The allocator that owns and manages the voxel data buffers holding all
    /// the voxel model data, heterogenously allocated due to sparsity of different
    /// models with different attachments, sizes, and type.
    voxel_data_allocator: VoxelDataAllocator,
}

impl VoxelModelRegistryGpu {
    pub const VOXEL_MODEL_INFO_ALLOCATOR_INITIAL_SIZE: u64 = 8 * 1024 * 1024; // 8 MB

    pub fn new(device: &mut DeviceResource) -> Self {
        let mut s = Self {
            gpu_models: HashMap::new(),
            gpu_model_construct_fns: HashMap::new(),
            voxel_model_info_allocator: GpuBufferAllocator::new(
                device,
                "voxel_model_info_allocator",
                Self::VOXEL_MODEL_INFO_ALLOCATOR_INITIAL_SIZE,
            ),
            voxel_data_allocator: VoxelDataAllocator::new(),
        };

        s.register_gpu_model_type::<VoxelModelSFTCompressed, VoxelModelSFTCompressedGpu>();

        s
    }

    pub fn write_render_data(registry_gpu: ResMut<VoxelModelRegistryGpu>) {}

    pub fn voxel_model_info_buffer(&self) -> &ResourceId<Buffer> {
        self.voxel_model_info_allocator.buffer()
    }

    pub fn voxel_data_allocator(&self) -> &VoxelDataAllocator {
        &self.voxel_data_allocator
    }

    pub fn register_gpu_model_type<T: VoxelModelImpl, G: VoxelModelGpuImpl>(&mut self) {
        self.gpu_model_construct_fns
            .insert(std::any::TypeId::of::<T>(), || Box::new(G::construct()));
    }
}
