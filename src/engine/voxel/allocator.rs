const WORLD_BUFFER_SIZE: u64 = 1 << 29;

/// Handles allocation of contiguous blocks of memory for voxel models. Necessary so data can
/// easily be replicated with a large gpu voxel "heap" buffer.
pub struct VoxelAllocator {
    world_buffer: wgpu::Buffer,
    world_buffer_size: u64,
}

impl VoxelAllocator {
    pub fn new(device: &wgpu::Device) -> Self {
        let world_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world_buffer"),
            size: WORLD_BUFFER_SIZE,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            world_buffer,
            world_buffer_size: WORLD_BUFFER_SIZE,
        }
    }
}
