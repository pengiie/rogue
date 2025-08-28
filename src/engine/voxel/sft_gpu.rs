use crate::engine::graphics::device::GfxDevice;

use super::{
    sft::VoxelModelSFT,
    sft_compressed::VoxelModelSFTCompressed,
    sft_compressed_gpu::VoxelModelSFTCompressedGpu,
    voxel::{VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl},
    voxel_allocator::VoxelDataAllocator,
};

pub struct VoxelModelSFTGpu {
    compressed_model: Option<VoxelModelSFTCompressed>,
    compressed_model_gpu: VoxelModelSFTCompressedGpu,

    initialized_data: bool,
    update_tracker: u32,
}

impl VoxelModelGpuImplConcrete for VoxelModelSFTGpu {
    fn new() -> Self {
        Self {
            compressed_model: None,
            compressed_model_gpu: VoxelModelSFTCompressedGpu::new(),

            initialized_data: false,
            update_tracker: 0,
        }
    }
}

impl VoxelModelGpuImpl for VoxelModelSFTGpu {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        self.compressed_model_gpu.aggregate_model_info()
    }

    fn update_gpu_objects(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool {
        let model = model.downcast_ref::<VoxelModelSFT>().unwrap();

        let mut did_allocate = false;
        if self.update_tracker != model.update_tracker || !self.initialized_data {
            self.initialized_data = true;
            self.update_tracker = model.update_tracker;
            let compressed_model = VoxelModelSFTCompressed::from(model);
            if self.compressed_model.is_some() {
                self.compressed_model_gpu.dealloc(allocator);
            }

            self.compressed_model = Some(compressed_model);
            self.compressed_model_gpu = VoxelModelSFTCompressedGpu::new();
        }

        if let Some(compressed_model) = &self.compressed_model {
            did_allocate =
                self.compressed_model_gpu
                    .update_gpu_objects(device, allocator, compressed_model);
        }

        return did_allocate;
    }

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        if let Some(compressed_model) = &self.compressed_model {
            self.compressed_model_gpu
                .write_gpu_updates(device, allocator, compressed_model);
        };
    }
}
