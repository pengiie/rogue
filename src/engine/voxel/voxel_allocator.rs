const WORLD_DATA_BUFFER_SIZE: u64 = 1 << 29;

/// Handles allocation of contiguous blocks of memory for voxel models. Necessary so data can
/// easily be replicated with a large gpu voxel "heap" buffer.
pub struct VoxelAllocator {
    world_data_buffer: wgpu::Buffer,
    world_data_buffer_size: u64,
}

impl VoxelAllocator {
    pub fn new(device: &wgpu::Device) -> Self {
        let world_data_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world_buffer"),
            size: WORLD_DATA_BUFFER_SIZE,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            world_data_buffer,
            world_data_buffer_size: WORLD_DATA_BUFFER_SIZE,
        }
    }

    pub fn allocate(&mut self, size: u32) -> std::ops::Range<u32> {
        todo!("Implement pow2 allocator.")
    }

    pub fn world_data_buffer(&self) -> &wgpu::Buffer {
        &self.world_data_buffer
    }
}
