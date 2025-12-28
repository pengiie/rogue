use crate::engine::graphics::{
    backend::{Buffer, ResourceId},
    device::GfxDevice,
    gpu_allocator::{Allocation, GpuBufferAllocator},
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoxelDataAllocation {
    // first 5 most signifigant bits are array index.
    // next 27 bits are for the index into the voxel data array (32 bits total).
    // We can address (2^(27+2) (2 at the end cause we index by u32s) = 536870912 bytes = 0.5
    // giga2<F3>3
    // according to gpuinfo, this is the minimum supported maxStorageBufferRange.
    // https://vulkan.gpuinfo.org/displaydevicelimit.php?name=maxStorageBufferRange&platform=all
    // TODO: Consolidate traversal, ptr, and size.
    ptr: u32,
    traversal: u64,
    size: u64,
}

impl VoxelDataAllocation {
    /// ptr in terms of a every stride of 4 bytes, size in terms of bytes.
    pub fn new(buffer_index: u32, traversal: u64, ptr: u32, size: u64) -> Self {
        assert!(buffer_index < (1 << 6));
        assert!(ptr < (1 << 28));

        Self {
            ptr: (buffer_index << 27) | ptr,
            traversal,
            size,
        }
    }

    /// The pointer to use for the gpu.
    pub fn ptr_gpu(&self) -> u32 {
        self.ptr
    }

    pub fn null() -> Self {
        Self {
            ptr: u32::MAX,
            traversal: 0,
            size: 0,
        }
    }

    pub fn as_buffer_allocation(&self) -> Allocation {
        let start = self.start_index_stride_bytes();
        let size = self.size;

        Allocation {
            // The traversal is the exact same as the range start.
            traversal: self.traversal,
            range: start..(start + size),
        }
    }

    pub fn buffer_ptr(&self) -> u32 {
        self.ptr & 0x07FF_FFFF
    }

    pub fn buffer_index(&self) -> u32 {
        self.ptr >> 27
    }

    pub fn start_index_stride_bytes(&self) -> u64 {
        (self.buffer_ptr() as u64) << 2
    }

    pub fn start_index_stride_dword(&self) -> u64 {
        self.buffer_ptr() as u64
    }

    pub fn length_bytes(&self) -> u64 {
        self.size
    }
}

pub struct VoxelDataAllocator {
    allocators: Vec<GpuBufferAllocator>,
    total_allocation_size: u64,
}

impl VoxelDataAllocator {
    // 27 bits available to index with a stride of 4 bytes.
    const ALLOCATION_BUFFER_SIZE: u64 = 1 << (27 + 2);

    pub fn new() -> Self {
        Self {
            allocators: Vec::new(),
            total_allocation_size: 0,
        }
    }

    pub fn create_allocator(&mut self, device: &mut GfxDevice) -> Option<u32> {
        let name = format!("voxel_data_allocator_{}", self.allocators.len());
        self.allocators.push(GpuBufferAllocator::new(
            device,
            &name,
            Self::ALLOCATION_BUFFER_SIZE,
        ));
        return Some(self.allocators.len() as u32 - 1);
    }

    pub fn allocate(&mut self, device: &mut GfxDevice, bytes: u64) -> Option<VoxelDataAllocation> {
        let allocation = 'alloc: {
            for (i, allocator) in self.allocators.iter_mut().enumerate() {
                if let Some(allocation) = allocator.allocate(bytes) {
                    break 'alloc Some(VoxelDataAllocation::new(
                        i as u32,
                        allocation.traversal,
                        allocation.start_index_stride_dword() as u32,
                        allocation.length_bytes(),
                    ));
                }
            }

            if let Some(allocator_idx) = self.create_allocator(device) {
                let mut allocator = &mut self.allocators[allocator_idx as usize];
                if let Some(allocation) = allocator.allocate(bytes) {
                    break 'alloc Some(VoxelDataAllocation::new(
                        allocator_idx,
                        allocation.traversal,
                        allocation.start_index_stride_dword() as u32,
                        allocation.length_bytes(),
                    ));
                }
            }
            break 'alloc None;
        };

        if let Some(allocation) = allocation {
            self.total_allocation_size += allocation.length_bytes();
        }

        return allocation;
    }

    pub fn reallocate(
        &mut self,
        device: &mut GfxDevice,
        old_allocation: &VoxelDataAllocation,
        bytes: u64,
    ) -> Option<VoxelDataAllocation> {
        todo!()
    }

    pub fn write_allocation_data(
        &mut self,
        device: &mut GfxDevice,
        allocation: &VoxelDataAllocation,
        data: &[u8],
    ) {
        let allocator = self
            .allocators
            .get_mut(allocation.buffer_index() as usize)
            .unwrap();
        allocator.write_allocation_data(device, &allocation.as_buffer_allocation(), data);
    }

    pub fn free(&mut self, allocation: &VoxelDataAllocation) {
        self.allocators
            .get_mut(allocation.buffer_index() as usize)
            .unwrap()
            .free(&allocation.as_buffer_allocation());
        self.total_allocation_size -= allocation.length_bytes();
    }

    pub fn buffers(&self) -> Vec<ResourceId<Buffer>> {
        self.allocators
            .iter()
            .map(|allocator| allocator.buffer().clone())
            .collect::<Vec<_>>()
    }

    pub fn total_allocation_size(&self) -> u64 {
        return self.total_allocation_size;
    }
}
