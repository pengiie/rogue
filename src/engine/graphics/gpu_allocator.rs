use log::debug;

use crate::engine::graphics::{backend::GfxBufferCreateInfo, device::DeviceResource};

use super::{
    backend::{Buffer, GraphicsBackendDevice, ResourceId},
    device::GfxDevice,
};

/// Power of 2 allocator operating on gpu-only buffers.
pub struct GpuBufferAllocator {
    buffer: ResourceId<Buffer>,

    // TODO: create deallocation reciever so we can cleanup removed models.
    // TODO: track frame bandwidth so we reduce frame staggers when multiple
    // models or a large model uploads.
    allocations: AllocatorTree,
    total_allocated_size: u64,
}

impl GpuBufferAllocator {
    pub fn new(device: &mut GfxDevice, name: &str, size: u64) -> Self {
        assert!(size.is_power_of_two());
        let buffer = device.create_buffer(GfxBufferCreateInfo {
            name: name.to_owned(),
            size,
        });

        Self {
            buffer,
            allocations: AllocatorTree::new(0, 0, size),
            total_allocated_size: 0,
        }
    }

    pub fn allocate(&mut self, bytes: u64) -> Option<Allocation> {
        assert!(
            bytes.next_power_of_two() <= self.allocations.size,
            "Tried to allocate {} bytes but allocator can only hold {}",
            bytes.next_power_of_two(),
            self.allocations.size
        );
        let allocation_size = bytes.next_power_of_two();
        self.total_allocated_size += allocation_size;
        let allocation = self.allocations.allocate(allocation_size, 4);

        allocation
    }

    pub fn reallocate(&mut self, old_allocation: &Allocation, bytes: u64) -> Option<Allocation> {
        assert!(
            bytes.next_power_of_two() <= self.allocations.size,
            "Tried to allocate {} bytes but allocator can only hold {}",
            bytes.next_power_of_two(),
            self.allocations.size
        );
        let allocation_size = bytes.next_power_of_two();
        todo!("Perform reallocation");
    }

    pub fn write_allocation_data(
        &self,
        device: &mut GfxDevice,
        allocation: &Allocation,
        data: &[u8],
    ) {
        // Ensure we do not write out of bounds.
        assert!(data.len() as u64 <= allocation.range.end - allocation.range.start);

        let offset = allocation.range.start;
        device.write_buffer_slice(self.buffer(), allocation.range.start, data);
    }

    pub fn buffer(&self) -> &ResourceId<Buffer> {
        &self.buffer
    }

    pub fn total_allocated_size(&self) -> u64 {
        self.total_allocated_size
    }
}

#[derive(Clone)]
pub struct Allocation {
    /// Currently, used as a unique identifier hash for an allocation.
    traversal: u64,
    range: std::ops::Range<u64>,
}

impl Allocation {
    /// Interprests the start index if the array is represented as `array<u8>`.
    pub fn start_index_stride_bytes(&self) -> u64 {
        self.range.start
    }

    /// Interprests the start index if the array is represented as `array<u32>`.
    pub fn start_index_stride_dword(&self) -> u64 {
        (self.range.start >> 2)
    }

    pub fn length_bytes(&self) -> u64 {
        (self.range.end - self.range.start)
    }

    pub fn length_dword(&self) -> u64 {
        ((self.range.end >> 2) - (self.range.start >> 2))
    }
}

impl std::fmt::Debug for Allocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut traversal_str = String::new();
        for i in (0..64).rev() {
            traversal_str.push_str(if (self.traversal & (1 << i)) > 0 {
                "1"
            } else {
                "0"
            });
        }
        f.debug_struct("VoxelDataAllocation")
            .field("traversal", &traversal_str)
            .field("range", &self.range)
            .finish()
    }
}

pub struct AllocatorTree {
    traversal: u64,
    start_index: u64,
    size: u64,
    left: Option<Box<AllocatorTree>>,
    right: Option<Box<AllocatorTree>>,
    is_allocated: bool,
}

impl AllocatorTree {
    pub fn new_root(size: u64) -> Self {
        Self::new(0, 0, size)
    }

    fn new(traversal: u64, start_index: u64, size: u64) -> Self {
        assert!(size.is_power_of_two());

        Self {
            traversal,
            start_index,
            size,
            left: None,
            right: None,
            is_allocated: false,
        }
    }

    pub fn allocate(&mut self, needed_size: u64, required_alignment: u32) -> Option<Allocation> {
        assert!(needed_size.is_power_of_two());
        // This node is already allocated don't search any further.
        if self.is_allocated {
            return None;
        }

        // This node is free and it fits our needed size so allocate it.
        if needed_size == self.size {
            // Ensure it doesnt have any children, if it does then that mean something is allocated
            // within it's range.
            if self.left.is_some()
                || self.right.is_some()
                || self.start_index % required_alignment as u64 != 0
            {
                return None;
            } else {
                return Some(self.make_allocated());
            }
        }

        let child_size = self.size >> 1;
        let new_child = |dir| {
            let mut new_child = Box::new(AllocatorTree::new(
                self.traversal | (dir << self.size.trailing_zeros()),
                self.start_index + child_size * dir,
                child_size,
            ));
            let allocation = new_child.allocate(needed_size, required_alignment).unwrap();

            (new_child, allocation)
        };

        if let Some(left) = &mut self.left {
            // The left node exists so traverse down to see if there is a free space.
            if let Some(found) = left.allocate(needed_size, required_alignment) {
                return Some(found);
            }
        } else {
            let (new_child, allocation) = new_child(0);
            self.left = Some(new_child);
            return Some(allocation);
        }

        if let Some(right) = &mut self.right {
            // The left node exists so traverse down to see if there is a free space.
            if let Some(found) = right.allocate(needed_size, required_alignment) {
                return Some(found);
            }
        } else {
            let (new_child, allocation) = new_child(1);
            self.right = Some(new_child);
            return Some(allocation);
        }

        return None;
    }

    fn make_allocated(&mut self) -> Allocation {
        assert!(!self.is_allocated);
        self.is_allocated = true;

        Allocation {
            traversal: self.traversal,
            range: self.start_index..(self.start_index + self.size),
        }
    }
}
