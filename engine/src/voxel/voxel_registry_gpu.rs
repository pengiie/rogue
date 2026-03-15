use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::graphics::backend::{Buffer, ResourceId};
use crate::graphics::device::DeviceResource;
use crate::graphics::gpu_allocator::GpuBufferAllocator;
use crate::resource::{Res, ResMut};
use crate::voxel::baker_gpu::{ModelBakeRequest, VoxelBakerGpu};
use crate::voxel::sft_compressed::VoxelModelSFTCompressed;
use crate::voxel::sft_compressed_gpu::VoxelModelSFTCompressedGpu;
use crate::voxel::voxel::{VoxelModelGpuImpl, VoxelModelImpl};
use crate::voxel::voxel_registry::VoxelModelRegistry;
use crate::voxel::{
    voxel::VoxelModelGpuImplMethods, voxel_allocator::VoxelDataAllocator,
    voxel_registry::VoxelModelId,
};
use crate::world::region_map::ChunkId;

struct VoxelModelGpuInfo {
    gpu_model: Box<dyn VoxelModelGpuImplMethods>,
    gpu_model_ptr: Option<u32>,
}

pub struct VoxelModelGpuInvalidationInfo {
    pub model_id: VoxelModelId,
    pub offset: Vector3<u32>,
    pub size: Vector3<u32>,
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
    /// Models which are already allocated but need their gpu material data invalidated.
    to_invalidate_models: Vec<VoxelModelGpuInvalidationInfo>,
    /// Any models which were updated/created which may require buffer allocations.
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

    pub fn load_gpu_model(&mut self, voxel_model_id: VoxelModelId) {
        assert!(!voxel_model_id.is_null());
        assert!(
            !self.gpu_models.contains_key(&voxel_model_id),
            "Model is already allocated on the gpu"
        );
        self.to_allocate_models.push(voxel_model_id);
    }

    pub fn mark_gpu_model_update(&mut self, voxel_model_id: &VoxelModelId) {
        assert!(self.gpu_models.contains_key(voxel_model_id));
        self.to_update_models.push(*voxel_model_id);
    }

    /// Mainly just used for invalidating materials which need to be calculated but not marking a
    /// whole update, TODO: I think i can combine tihis with to_update_models and make the update
    /// range optional but idk its really a different operation.
    pub fn invalidate_gpu_model_material(
        &mut self,
        invalidation_info: VoxelModelGpuInvalidationInfo,
    ) {
        assert!(!invalidation_info.model_id.is_null());
        self.to_invalidate_models.push(invalidation_info);
    }

    pub fn write_render_data(
        registry: Res<VoxelModelRegistry>,
        mut registry_gpu: ResMut<VoxelModelRegistryGpu>,
        mut device: ResMut<DeviceResource>,
        mut baker: ResMut<VoxelBakerGpu>,
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
            let side_length = registry.get_dyn_model(model_id).length();
            let construct_fn = registry_gpu
                .gpu_model_construct_fns
                .get(&model_info.model_type_id)
                .expect("Voxel model doesn't have gpu repr registered");

            let old = registry_gpu.gpu_models.insert(
                model_id,
                VoxelModelGpuInfo {
                    gpu_model: construct_fn(),
                    gpu_model_ptr: None,
                },
            );
            assert!(
                old.is_none(),
                "Gpu model already exists for model id which should means the model was allocated twice."
            );
            // New gpu model so info and data needs to be allocated.
            registry_gpu.to_update_models.push(model_id);
        }

        let mut non_ready_models = Vec::new();
        let mut to_write_models = HashSet::with_capacity(
            registry_gpu.to_invalidate_models.len() + registry_gpu.to_update_models.len(),
        );

        let mut invalidated_model_id = HashSet::new();
        // Any models which need their normals recalculated or had their material changed in some
        // way need only their raw attachment data re-uploaded.
        for VoxelModelGpuInvalidationInfo {
            model_id,
            offset,
            size,
        } in registry_gpu.to_invalidate_models.drain(..)
        {
            if invalidated_model_id.contains(&model_id) {
                continue;
            }
            invalidated_model_id.insert(model_id);
            let Some(gpu_model_info) = registry_gpu.gpu_models.get_mut(&model_id) else {
                continue;
            };
            // This only flags the gpu model for invalidation next render write.
            gpu_model_info.gpu_model.mark_for_invalidation();
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
                    log::info!(
                        "New allocation requested with old ptr pointing at {}",
                        old_ptr
                    );
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

        // Write any gpu render data the model needs updated, the model instance is responsible for
        // tracking what data in the model needs to be updated, which can be influenced by voxel
        // edits or material invalidation requests.
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
