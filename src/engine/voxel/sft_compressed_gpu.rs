use std::collections::HashMap;

use crate::engine::graphics::device::GfxDevice;

use super::{
    attachment::{Attachment, AttachmentId},
    sft_compressed::{
        SFTAttachmentLookupNodeCompressed, SFTNodeCompressed, VoxelModelSFTCompressed,
    },
    voxel::{VoxelModelGpuImpl, VoxelModelGpuImplConcrete, VoxelModelImpl},
    voxel_allocator::{VoxelDataAllocation, VoxelDataAllocator},
};

pub struct VoxelModelSFTCompressedGpu {
    // Model side length in voxels.
    side_length: u32,
    nodes_allocation: Option<VoxelDataAllocation>,
    attachment_lookup_allocations: HashMap<AttachmentId, VoxelDataAllocation>,
    attachment_raw_allocations: HashMap<AttachmentId, VoxelDataAllocation>,

    initialized_data: bool,
    update_tracker: u32,
}

impl VoxelModelSFTCompressedGpu {
    pub fn dealloc(&mut self, allocator: &mut VoxelDataAllocator) {
        if let Some(nodes_alloc) = self.nodes_allocation.take() {
            allocator.free(&nodes_alloc);
        }
        for (_, alloc) in self.attachment_lookup_allocations.drain() {
            allocator.free(&alloc);
        }
        for (_, alloc) in self.attachment_raw_allocations.drain() {
            allocator.free(&alloc);
        }
    }

    // Returns true if a data pointer was updated, triggering an update in the model info.
    fn try_create_or_update_node_allocation(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        required_size: u64,
    ) -> bool {
        let allocation = &mut self.nodes_allocation;
        match allocation {
            Some(old_allocation) => {
                if old_allocation.length_bytes() < required_size {
                    let new_allocation = allocator
                        .reallocate(device, old_allocation, required_size)
                        .expect("Failed to reallocate voxel model data.");
                    if old_allocation.start_index_stride_bytes()
                        != new_allocation.start_index_stride_bytes()
                    {
                        *allocation = Some(new_allocation);
                        return true;
                    }
                }
                return false;
            }
            None => {
                log::info!("SFT node data allocation");
                let new_allocation = allocator
                    .allocate(device, required_size)
                    .expect("Failed to allocate voxel model data.");
                *allocation = Some(new_allocation);
                return true;
            }
        }
    }

    fn try_create_or_update_attachment_allocation(
        allocations: &mut HashMap<AttachmentId, VoxelDataAllocation>,
        attachment_id: &AttachmentId,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        required_size: u64,
    ) -> bool {
        match allocations.get(attachment_id) {
            Some(old_allocation) => {
                if old_allocation.length_bytes() < required_size {
                    let new_allocation = allocator
                        .reallocate(device, old_allocation, required_size)
                        .expect("Failed to reallocate attachment data.");
                    return true;
                }
                return false;
            }
            None => {
                log::info!("SFT attachment {:?} allocation data", attachment_id);
                let new_allocation = allocator
                    .allocate(device, required_size)
                    .expect("Failed to allocate attachment data.");
                allocations.insert(attachment_id.clone(), new_allocation);
                return true;
            }
        }
    }
}

impl VoxelModelGpuImplConcrete for VoxelModelSFTCompressedGpu {
    fn new() -> Self {
        Self {
            side_length: 0,
            nodes_allocation: None,
            attachment_lookup_allocations: HashMap::new(),
            attachment_raw_allocations: HashMap::new(),

            initialized_data: false,
            update_tracker: 0,
        }
    }
}

impl VoxelModelGpuImpl for VoxelModelSFTCompressedGpu {
    fn aggregate_model_info(&self) -> Option<Vec<u32>> {
        let Some(data_allocation) = &self.nodes_allocation else {
            return None;
        };
        if self.attachment_lookup_allocations.is_empty()
            || self.attachment_raw_allocations.is_empty()
        {
            log::info!("no attachments");
            return None;
        }
        if self.side_length == 0 {
            log::info!("no length");
            return None;
        }

        let mut attachment_lookup_indices =
            vec![u32::MAX; Attachment::MAX_ATTACHMENT_COUNT as usize];
        for (attachment, lookup_allocation) in &self.attachment_lookup_allocations {
            if *attachment > Attachment::MAX_ATTACHMENT_ID {
                continue;
            }

            attachment_lookup_indices[*attachment as usize] = lookup_allocation.ptr_gpu();
        }
        let mut attachment_raw_indices = vec![u32::MAX; Attachment::MAX_ATTACHMENT_COUNT as usize];
        for (attachment, raw_allocation) in &self.attachment_raw_allocations {
            if *attachment > Attachment::MAX_ATTACHMENT_ID {
                continue;
            }

            attachment_raw_indices[*attachment as usize] = raw_allocation.ptr_gpu();
        }

        let mut info = vec![
            self.side_length,
            // Node ptr (divide by 4 since 4 bytes in a u32)
            data_allocation.ptr_gpu(),
        ];
        info.append(&mut attachment_lookup_indices);
        info.append(&mut attachment_raw_indices);

        Some(info)
    }

    fn update_gpu_objects(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) -> bool {
        let model = model.downcast_ref::<VoxelModelSFTCompressed>().unwrap();
        let mut did_allocate = false;

        let nodes_allocation_size = model.node_data.len() as u64 * SFTNodeCompressed::BYTE_SIZE;
        did_allocate |=
            self.try_create_or_update_node_allocation(device, allocator, nodes_allocation_size);

        for (attachment_id, data) in model.attachment_lookup_data.iter() {
            did_allocate |= Self::try_create_or_update_attachment_allocation(
                &mut self.attachment_lookup_allocations,
                &attachment_id,
                device,
                allocator,
                data.len() as u64 * SFTAttachmentLookupNodeCompressed::BYTE_SIZE,
            );
        }

        for (attachment_id, data) in model.attachment_raw_data.iter() {
            let attachment = model.attachment_map.get_unchecked(attachment_id);
            did_allocate |= Self::try_create_or_update_attachment_allocation(
                &mut self.attachment_raw_allocations,
                &attachment_id,
                device,
                allocator,
                data.len() as u64 * attachment.byte_size() as u64,
            );
        }

        // Add implicit normal attachment if the model is using a ptmaterial.
        if !model.attachment_raw_data.contains(Attachment::NORMAL_ID)
            && model
                .attachment_lookup_data
                .contains(Attachment::PTMATERIAL_ID)
        {
            if model.attachment_map.contains(Attachment::NORMAL_ID) {
                assert!(
                    model.attachment_map.get_unchecked(Attachment::NORMAL_ID)
                        == &Attachment::NORMAL
                );
            }

            let implicit_normal_byte_size = Attachment::NORMAL.byte_size() as u64
                * model
                    .attachment_raw_data
                    .get(Attachment::PTMATERIAL_ID)
                    .unwrap()
                    .len() as u64;
            did_allocate |= Self::try_create_or_update_attachment_allocation(
                &mut self.attachment_raw_allocations,
                &Attachment::NORMAL_ID,
                device,
                allocator,
                implicit_normal_byte_size,
            );
        }

        if self.side_length != model.side_length {
            self.side_length = model.side_length;
            // We don't technically allocate anything if this changes, however we
            // return true so the model info entry is updated.
            did_allocate |= true;
        }

        return did_allocate;
    }

    fn write_gpu_updates(
        &mut self,
        device: &mut GfxDevice,
        allocator: &mut VoxelDataAllocator,
        model: &dyn VoxelModelImpl,
    ) {
        let model = model.downcast_ref::<VoxelModelSFTCompressed>().unwrap();

        // If data allocation is some and we haven't initialized yet, expected the attachment data
        // to also be ready.
        if !self.initialized_data && self.nodes_allocation.is_some() {
            {
                let mut node_data_packed = Vec::with_capacity(
                    model.node_data.len() * SFTNodeCompressed::U32_SIZE as usize,
                );
                for node in &model.node_data {
                    node_data_packed.push(node.child_ptr);
                    // Little endian.
                    node_data_packed.push((node.child_mask & 0xFFFF_FFFF) as u32);
                    node_data_packed.push((node.child_mask >> 32) as u32);
                    node_data_packed.push((node.leaf_mask & 0xFFFF_FFFF) as u32);
                    node_data_packed.push((node.leaf_mask >> 32) as u32);
                }

                let node_data_bytes = bytemuck::cast_slice::<u32, u8>(&node_data_packed);
                allocator.write_allocation_data(
                    device,
                    self.nodes_allocation.as_ref().unwrap(),
                    node_data_bytes,
                );
            }

            for (attachment_id, lookup_data) in model.attachment_lookup_data.iter() {
                assert_eq!(lookup_data.len(), model.node_data.len());
                let allocation = self
                    .attachment_lookup_allocations
                    .get(&attachment_id)
                    .expect("Lookup allocation should exist by now.");

                let mut lookup_data_packed = Vec::with_capacity(
                    lookup_data.len() * SFTAttachmentLookupNodeCompressed::U32_SIZE as usize,
                );
                for lookup in lookup_data {
                    lookup_data_packed.push(lookup.data_ptr);
                    // Little endian.
                    lookup_data_packed.push((lookup.attachment_mask & 0xFFFF_FFFF) as u32);
                    lookup_data_packed.push((lookup.attachment_mask >> 32) as u32);
                }
                let lookup_data_bytes = bytemuck::cast_slice::<u32, u8>(&lookup_data_packed);
                allocator.write_allocation_data(device, allocation, lookup_data_bytes);
            }

            for (attachment, raw_data) in model.attachment_raw_data.iter() {
                let allocation = self
                    .attachment_raw_allocations
                    .get(&attachment)
                    .expect("Raw allocation should exist by now.");

                allocator.write_allocation_data(
                    device,
                    allocation,
                    bytemuck::cast_slice::<u32, u8>(raw_data.as_slice()),
                );
            }

            self.initialized_data = true;
            return;
        }
    }

    fn deallocate(&mut self, allocator: &mut VoxelDataAllocator) {
        if let Some(nodes_alloc) = self.nodes_allocation.take() {
            allocator.free(&nodes_alloc);
        }
        for (_, alloc) in self.attachment_lookup_allocations.drain() {
            allocator.free(&alloc);
        }
        for (_, alloc) in self.attachment_raw_allocations.drain() {
            allocator.free(&alloc);
        }
    }
}
