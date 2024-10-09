use log::debug;

use crate::engine::graphics::device::DeviceResource;

const WORLD_DATA_BUFFER_SIZE: u64 = 1 << 29;

/// Handles allocation of contiguous blocks of memory for voxel models. Necessary so data can
/// easily be replicated with a large gpu voxel "heap" buffer.
pub struct VoxelAllocator {
    world_data_buffer: wgpu::Buffer,

    // TODO: create deallocation reciever so we can cleanup removed models.
    allocations: VoxelAllocatorTree,
}

impl VoxelAllocator {
    pub fn new(device: &wgpu::Device, initial_size: u64) -> Self {
        assert!(initial_size.is_power_of_two());
        let world_data_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world_buffer"),
            size: initial_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            world_data_buffer,
            allocations: VoxelAllocatorTree::new(0, 0, initial_size),
        }
    }

    pub fn allocate(&mut self, size: u64) -> Option<VoxelDataAllocation> {
        assert!(size.next_power_of_two() <= self.allocations.size);
        let allocation = self.allocations.allocate(size.next_power_of_two());
        debug!("Allocated {:?}", allocation);

        allocation
    }

    pub fn write_world_data(
        &self,
        device: &DeviceResource,
        allocation: &VoxelDataAllocation,
        data: &[u8],
    ) {
        assert_eq!(
            data.len() as u64,
            allocation.range.end - allocation.range.start
        );
        let offset = allocation.range.start;
        device
            .queue()
            .write_buffer(self.world_data_buffer(), allocation.range.start, data)
    }

    pub fn world_data_buffer(&self) -> &wgpu::Buffer {
        &self.world_data_buffer
    }
}

pub struct VoxelDataAllocation {
    /// Currently, used as a unique identifier hash for an allocation.
    traversal: u64,
    range: std::ops::Range<u64>,
}

impl VoxelDataAllocation {
    pub fn start_index(&self) -> u32 {
        self.range.start as u32
    }
}

impl std::fmt::Debug for VoxelDataAllocation {
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

struct VoxelAllocatorTree {
    traversal: u64,
    start_index: u64,
    size: u64,
    left: Option<Box<VoxelAllocatorTree>>,
    right: Option<Box<VoxelAllocatorTree>>,
    is_allocated: bool,
}

impl VoxelAllocatorTree {
    fn new(traversal: u64, start_index: u64, size: u64) -> Self {
        Self {
            traversal,
            start_index,
            size,
            left: None,
            right: None,
            is_allocated: false,
        }
    }

    fn allocate(&mut self, needed_size: u64) -> Option<VoxelDataAllocation> {
        assert!(needed_size.is_power_of_two());
        // This node is already allocated don't search any further.
        if self.is_allocated {
            return None;
        }

        // This node is free and it fits our needed size so allocate it.
        if needed_size == self.size {
            // Ensure it doesnt have any children, if it does then that mean something is allocated
            // within it's range.
            if self.left.is_some() || self.right.is_some() {
                return None;
            } else {
                return Some(self.make_allocated());
            }
        }

        let child_size = self.size >> 1;
        let new_child = |dir| {
            let mut new_child = Box::new(VoxelAllocatorTree::new(
                self.traversal | (dir << self.size.trailing_zeros()),
                self.start_index + child_size * dir,
                child_size,
            ));
            let allocation = new_child.allocate(needed_size).unwrap();

            (new_child, allocation)
        };

        if let Some(left) = &mut self.left {
            // The left node exists so traverse down to see if there is a free space.
            if let Some(found) = left.allocate(needed_size) {
                return Some(found);
            }
        } else {
            let (new_child, allocation) = new_child(0);
            self.left = Some(new_child);
            return Some(allocation);
        }

        if let Some(right) = &mut self.right {
            // The left node exists so traverse down to see if there is a free space.
            if let Some(found) = right.allocate(needed_size) {
                return Some(found);
            }
        } else {
            let (new_child, allocation) = new_child(1);
            self.right = Some(new_child);
            return Some(allocation);
        }

        return None;
    }

    fn make_allocated(&mut self) -> VoxelDataAllocation {
        assert!(!self.is_allocated);
        self.is_allocated = true;

        VoxelDataAllocation {
            traversal: self.traversal,
            range: self.start_index..(self.start_index + self.size),
        }
    }
}
