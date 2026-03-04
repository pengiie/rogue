use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use rogue_macros::Resource;

use crate::graphics::backend::{Buffer, ResourceId};
use crate::graphics::device::DeviceResource;
use crate::graphics::gpu_allocator::GpuBufferAllocator;
use crate::resource::{Res, ResMut};
use crate::voxel::sft_compressed::VoxelModelSFTCompressed;
use crate::voxel::sft_compressed_gpu::VoxelModelSFTCompressedGpu;
use crate::voxel::voxel::{VoxelModelGpuImpl, VoxelModelImpl};
use crate::voxel::voxel_registry::VoxelModelRegistry;
use crate::voxel::{
    voxel::VoxelModelGpuImplMethods, voxel_allocator::VoxelDataAllocator,
    voxel_registry::VoxelModelId,
};

struct VoxelModelGpuInfo {
    gpu_model: Box<dyn VoxelModelGpuImplMethods>,
    gpu_model_ptr: Option<u32>,
}

/// Handles allocating and uploading voxel model data to the gpu and creating
/// gpu-based voxel model handles.
#[derive(Resource)]
pub struct VoxelModelRegistryGpu {
    gpu_models: HashMap<VoxelModelId, VoxelModelGpuInfo>,
    gpu_model_construct_fns:
        HashMap</*VoxelModelType*/ TypeId, fn() -> Box<dyn VoxelModelGpuImplMethods>>,
    gpu_model_schemas: HashMap<TypeId, u32>,

    /// The buffer for every unique voxel models info including for entities and terrain.
    /// The info includes the models type-specific descriptor with its associated length.
    voxel_model_info_allocator: GpuBufferAllocator,

    /// The allocator that owns and manages the voxel data buffers holding all
    /// the voxel model data, heterogenously allocated due to sparsity of different
    /// models with different attachments, sizes, and type.
    voxel_data_allocator: VoxelDataAllocator,

    /// Models that should have a gpu representation if they don't already.
    to_allocate_models: Vec<VoxelModelId>,
    to_invalidate_models: Vec<VoxelModelId>,
    to_update_models: Vec<VoxelModelId>,
}

impl VoxelModelRegistryGpu {
    pub const VOXEL_MODEL_INFO_ALLOCATOR_INITIAL_SIZE: u64 = 64 * 1024 * 1024; // 8 MB

    pub fn new(device: &mut DeviceResource) -> Self {
        let mut s = Self {
            gpu_models: HashMap::new(),
            gpu_model_construct_fns: HashMap::new(),
            gpu_model_schemas: HashMap::new(),
            voxel_model_info_allocator: GpuBufferAllocator::new(
                device,
                "voxel_model_info_allocator",
                Self::VOXEL_MODEL_INFO_ALLOCATOR_INITIAL_SIZE,
            ),
            voxel_data_allocator: VoxelDataAllocator::new(),
            to_allocate_models: Vec::new(),
            to_invalidate_models: Vec::new(),
            to_update_models: Vec::new(),
        };

        s.register_gpu_model_type::<VoxelModelSFTCompressed, VoxelModelSFTCompressedGpu>();

        s
    }

    pub fn ensure_model_exists(&mut self, model_id: &VoxelModelId) {
        assert!(!model_id.is_null());
        self.to_allocate_models.push(*model_id);
        self.to_update_models.push(*model_id);
    }

    /// Rewrite the CPU material data back to the GPU, used for baking.
    pub fn invalidate_model_gpu_material(&mut self, model_id: &VoxelModelId) {
        assert!(!model_id.is_null());
        self.to_invalidate_models.push(*model_id);
    }

    pub fn write_render_data(
        registry: Res<VoxelModelRegistry>,
        mut registry_gpu: ResMut<VoxelModelRegistryGpu>,
        mut device: ResMut<DeviceResource>,
    ) {
        let registry_gpu = &mut *registry_gpu;
        // Allocate any new gpu models that have been requested.
        for model_id in registry_gpu.to_allocate_models.drain(..) {
            if registry_gpu.gpu_models.contains_key(&model_id) {
                continue;
            }

            let model_info = registry
                .voxel_model_info
                .get(model_id.handle)
                .expect("Voxel model id not found in the registry");
            let construct_fn = registry_gpu
                .gpu_model_construct_fns
                .get(&model_info.model_type_id)
                .expect("Voxel model doesn't have gpu repr registered");

            registry_gpu.gpu_models.insert(
                model_id,
                VoxelModelGpuInfo {
                    gpu_model: construct_fn(),
                    gpu_model_ptr: None,
                },
            );
        }

        let mut non_ready_models = Vec::new();
        let mut to_write_models = HashSet::with_capacity(
            registry_gpu.to_invalidate_models.len() + registry_gpu.to_update_models.len(),
        );

        // Any models which need their normals recalculated or had their material changed in some
        // way need only their raw attachment data re-uploaded.
        for model_id in registry_gpu.to_invalidate_models.drain(..) {
            let Some(gpu_model_info) = registry_gpu.gpu_models.get_mut(&model_id) else {
                continue;
            };
            gpu_model_info.gpu_model.invalidate_material();
            to_write_models.insert(model_id);
        }

        // Any models which have had their data updated in a constructive or destructive way which
        // may cause an allocation needs to have their buffers re-updated and gpu model info
        // rewritten if there are any new allocations. This initializes/updates the gpu_model_ptr for the
        // gpu model.
        for model_id in registry_gpu.to_update_models.drain(..) {
            let type_id = registry
                .voxel_model_info
                .get(model_id.handle)
                .expect("Voxel model id not found in the registry")
                .model_type_id;
            let gpu_model_info = registry_gpu
                .gpu_models
                .get_mut(&model_id)
                .expect("Voxel model gpu info not found for model id");

            let model = registry.get_dyn_model(model_id);
            // Allocate any necessary buffers the model needs for its representation.
            let mut needs_info_allocation = gpu_model_info.gpu_model.update_gpu_objects(
                &mut device,
                &mut registry_gpu.voxel_data_allocator,
                model,
            );

            let model_gpu_schema = *registry_gpu
                .gpu_model_schemas
                .get(&type_id)
                .expect("Voxel model doesn't have gpu repr registered");
            let Some(model_info_gpu_repr) = gpu_model_info.gpu_model.aggregate_model_info() else {
                log::warn!(
                    "Voxel model with id {:?} is not ready to have its gpu representation written",
                    model_id
                );
                // Model buffers are not allocated yet.
                non_ready_models.push(model_id);
                continue;
            };

            to_write_models.insert(model_id);
            needs_info_allocation |= gpu_model_info.gpu_model_ptr.is_none();

            if needs_info_allocation {
                if let Some(old_ptr) = gpu_model_info.gpu_model_ptr {
                    // TODO: Deallocate old info.
                }
                let allocation_size = (model_info_gpu_repr.len() + 1) as u64 * 4;
                let info_allocation = registry_gpu
                    .voxel_model_info_allocator
                    .allocate(allocation_size)
                    .expect("Failed to allocate voxel model info gpu buffer");
                let mut data = vec![model_gpu_schema];
                data.extend_from_slice(&model_info_gpu_repr);
                registry_gpu
                    .voxel_model_info_allocator
                    .write_allocation_data(
                        &mut device,
                        &info_allocation,
                        bytemuck::cast_slice(&data),
                    );
                gpu_model_info.gpu_model_ptr =
                    Some(info_allocation.start_index_stride_dword() as u32);
            }
        }

        // Write the render data of the model to the gpu.
        for model_id in to_write_models {
            let gpu_model_info = registry_gpu
                .gpu_models
                .get_mut(&model_id)
                .expect("Voxel model gpu info not found for model id");
            let model = registry.get_dyn_model(model_id);
            gpu_model_info.gpu_model.write_gpu_updates(
                &mut device,
                &mut registry_gpu.voxel_data_allocator,
                model,
            );
        }

        for model_id in non_ready_models {
            registry_gpu.to_update_models.push(model_id);
        }
    }

    pub fn voxel_model_info_buffer(&self) -> &ResourceId<Buffer> {
        self.voxel_model_info_allocator.buffer()
    }

    pub fn voxel_data_allocator(&self) -> &VoxelDataAllocator {
        &self.voxel_data_allocator
    }

    /// Ptr to the voxel model info butter when the voxel model's gpu representation lies.
    pub fn get_model_gpu_ptr(&self, model_id: &VoxelModelId) -> Option<u32> {
        assert!(!model_id.is_null());
        self.gpu_models
            .get(&model_id)
            .map(|info| info.gpu_model_ptr)
            .flatten()
    }

    pub fn register_gpu_model_type<T: VoxelModelImpl, G: VoxelModelGpuImpl>(&mut self) {
        self.gpu_model_construct_fns
            .insert(std::any::TypeId::of::<T>(), || Box::new(G::construct()));
        self.gpu_model_schemas
            .insert(std::any::TypeId::of::<T>(), G::SCHEMA);
    }
}
