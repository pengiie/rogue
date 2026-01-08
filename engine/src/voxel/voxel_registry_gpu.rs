use std::collections::HashMap;

use rogue_macros::Resource;

use crate::voxel::{
    voxel::VoxelModelGpuImpl, voxel_allocator::VoxelDataAllocator, voxel_registry::VoxelModelId,
};
use crate::graphics::gpu_allocator::GpuBufferAllocator;

/// Handles allocating and uploading voxel model data to the gpu and creating
/// gpu-based voxel model handles.
#[derive(Resource)]
pub struct VoxelModelRegistryGpu {
    gpu_models: HashMap<VoxelModelId, Box<dyn VoxelModelGpuImpl>>,
    gpu_model_types: HashMap</*VoxelModelType*/ String, fn() -> Box<dyn VoxelModelGpuImpl>>,

    /// The buffer for every unique voxel models info including for entities and terrain.
    /// The info includes the models type-specific descriptor with its associated length.
    voxel_model_info_allocator: Option<GpuBufferAllocator>,

    /// The allocator that owns and manages the voxel data buffers holding all
    /// the voxel model data, heterogenously allocated due to sparsity of different
    /// models with different attachments, sizes, and type.
    voxel_data_allocator: VoxelDataAllocator,
}

impl VoxelModelRegistryGpu {}
