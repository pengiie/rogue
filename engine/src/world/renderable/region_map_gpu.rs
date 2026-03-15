use std::collections::HashMap;

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    consts,
    event::{EventReader, Events},
    graphics::{
        backend::{Buffer, ResourceId},
        device::DeviceResource,
        gpu_allocator::{Allocation, GpuBufferAllocator},
    },
    resource::{Res, ResMut},
    voxel::{
        baker_gpu::VoxelBakerGpu,
        voxel_registry_gpu::{VoxelModelGpuInvalidationInfo, VoxelModelRegistryGpu},
    },
    world::{
        region::{WorldRegion, WorldRegionNode},
        region_map::{ChunkEvent, ChunkEventType, ChunkLOD, RegionEvent, RegionMap, RegionPos},
        renderable::region_window::TerrainRenderableWindow,
    },
};

#[derive(Resource)]
pub struct RegionMapGpu {
    region_event_reader: EventReader<RegionEvent>,
    chunk_event_reader: EventReader<ChunkEvent>,
    to_write_regions: Vec<RegionPos>,

    region_window: TerrainRenderableWindow,

    region_gpu_allocations: HashMap<RegionPos, Allocation>,
    region_data_buffer: GpuBufferAllocator,
}

impl RegionMapGpu {
    pub fn new(device: &mut DeviceResource) -> Self {
        Self {
            region_event_reader: EventReader::new(),
            chunk_event_reader: EventReader::new(),
            to_write_regions: Vec::new(),
            region_gpu_allocations: HashMap::new(),
            region_data_buffer: GpuBufferAllocator::new(
                device,
                "region_data_buffer",
                1024 * 1024 * 64, // 64 MB cause idk
            ),

            region_window: TerrainRenderableWindow::new(RegionPos::new(0, 0, 0), 8),
        }
    }

    // Allocates and writes any necessary render data to the GPU.
    pub fn write_render_data(
        mut region_map_gpu: ResMut<RegionMapGpu>,
        mut device: ResMut<DeviceResource>,
        mut region_map: ResMut<RegionMap>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
    ) {
        region_map_gpu.write_region_render_data(&mut device, &region_map, &voxel_registry_gpu);
        region_map_gpu.region_window.update_gpu_objects(&mut device);
        region_map_gpu.region_window.write_render_data(&mut device);
    }

    /// Gets the GPU byte representation of this region tree with the given LOD.
    pub fn convert_region_gpu(
        voxel_registry_gpu: &VoxelModelRegistryGpu,
        region: &WorldRegion,
    ) -> Vec<u8> {
        const HEADER_SIZE: usize = 8; // 4 bytes for header
        const NODE_SIZE: usize = 16; // 16 bytes per node, see voxel/region.slang
        let mut bytes = Vec::with_capacity(region.tree.nodes.len() * NODE_SIZE);
        // The LOD we should render at.
        let mut nodes = vec![(&region.tree.nodes[0], 0u32)];
        let mut max_depth = 0;
        while let Some((node, depth)) = nodes.pop() {
            if node.model_ptr != u32::MAX {
                max_depth = max_depth.max(depth);
            }
            if node.is_child_ptr_valid() {
                let child_ptr = node.child_ptr as usize;
                for i in 0..64 {
                    nodes.push((&region.tree.nodes[child_ptr + i], depth + 1));
                }
            }
        }
        let lod = ChunkLOD::from_tree_height(max_depth);
        bytes.extend_from_slice(&lod.0.to_le_bytes());
        for WorldRegionNode {
            model_ptr,
            parent_ptr,
            child_ptr,
            child_mask,
        } in &region.tree.nodes
        {
            let model_handle =
                (*model_ptr != u32::MAX).then(|| &region.model_handles[*model_ptr as usize]);
            let gpu_model_ptr = model_handle
                .map(|handle| voxel_registry_gpu.get_model_gpu_ptr(handle))
                .flatten()
                .unwrap_or(0xFFFF_FFFF);
            bytes.extend_from_slice(&gpu_model_ptr.to_le_bytes());
            bytes.extend_from_slice(&child_ptr.to_le_bytes());
            bytes.extend_from_slice(&child_mask.to_le_bytes());
        }

        bytes
    }

    fn write_region_render_data(
        &mut self,
        device: &mut DeviceResource,
        region_map: &RegionMap,
        voxel_registry_gpu: &VoxelModelRegistryGpu,
    ) {
        for region_pos in self.to_write_regions.drain(..) {
            if !self.region_window.contains_region(region_pos) {
                // Region is outside our render distance.
                continue;
            }

            let region_data = region_map
                .get_region(&region_pos)
                .expect("If the region is in `to_write_regions` then it should be loaded");
            let gpu_region_data = Self::convert_region_gpu(voxel_registry_gpu, region_data);
            let mem_pos = TerrainRenderableWindow::local_pos_to_mem_pos(
                (region_pos - self.region_window.region_anchor).map(|x| x as u32),
                self.region_window.window_offset,
                self.region_window.side_length,
            );
            let Some(region_gpu_allocation) =
                (if let Some(old_allocation) = self.region_gpu_allocations.get(&region_pos) {
                    self.region_data_buffer
                        .reallocate(old_allocation, gpu_region_data.len() as u64)
                } else {
                    self.region_data_buffer
                        .allocate(gpu_region_data.len() as u64)
                })
            else {
                log::error!("Failed to allocate GPU memory for region.");
                continue;
            };

            self.region_data_buffer.write_allocation_data(
                device,
                &region_gpu_allocation,
                &gpu_region_data,
            );
            self.region_window.set_region_ptr(
                region_pos,
                region_gpu_allocation.start_index_stride_dword() as u32,
            );
            self.region_gpu_allocations
                .insert(region_pos, region_gpu_allocation);
        }
    }

    pub fn update_gpu_chunk_models(
        mut region_map_gpu: ResMut<RegionMapGpu>,
        mut region_map: ResMut<RegionMap>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
        mut baker_gpu: ResMut<VoxelBakerGpu>,
        events: Res<Events>,
    ) {
        let region_map_gpu = &mut *region_map_gpu;
        // Handle the chunk event, if loaded or updated we manually invalidate, invalidation
        // towards the registry doesn't affect the baking, that is manual so the registry doesn't
        // need to know about where the voxel model is from. And so in that case we just just
        // listen to the chunk event
        for event in region_map_gpu.chunk_event_reader.read(&events) {
            match event.event_type {
                ChunkEventType::Loaded | ChunkEventType::Updated => {
                    let Some(model_id) = region_map.get_chunk_model(&event.chunk_id) else {
                        continue;
                    };
                    baker_gpu.create_chunk_bake_request(
                        event.chunk_id,
                        crate::voxel::baker_gpu::ModelBakeRequest {
                            offset: Vector3::new(0, 0, 0),
                            size: Vector3::new(
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                            ),
                        },
                    );
                    voxel_registry_gpu.invalidate_gpu_model_material(
                        VoxelModelGpuInvalidationInfo {
                            model_id,
                            offset: Vector3::new(0, 0, 0),
                            size: Vector3::new(
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                            ),
                        },
                    );

                    for neighbor in event.chunk_id.neighbors() {
                        if let Some(neighbor_model_id) = region_map.get_chunk_model(&neighbor) {
                            voxel_registry_gpu.invalidate_gpu_model_material(
                                VoxelModelGpuInvalidationInfo {
                                    model_id: neighbor_model_id,
                                    offset: Vector3::new(0, 0, 0),
                                    size: Vector3::new(
                                        consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                        consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                        consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                    ),
                                },
                            );
                            baker_gpu.create_chunk_bake_request(
                                neighbor,
                                crate::voxel::baker_gpu::ModelBakeRequest {
                                    offset: Vector3::new(0, 0, 0),
                                    size: Vector3::new(
                                        consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                        consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                        consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                    ),
                                },
                            );
                        }
                    }
                }
                ChunkEventType::Unloaded => {}
            }
        }

        // Handle region events.
        for event in region_map_gpu.region_event_reader.read(&events) {
            region_map_gpu.to_write_regions.push(event.region_pos);
        }

        for region_pos in &region_map_gpu.to_write_regions {
            if !region_map_gpu.region_window.contains_region(*region_pos) {
                // Region is outside our render distance.
                continue;
            }

            let region_data = region_map
                .get_region(&region_pos)
                .expect("If the region is in `to_write_regions` then it should be loaded");
            for model in &region_data.model_handles {
                if let Some(gpu_model_ptr) = voxel_registry_gpu.get_model_gpu_ptr(model) {
                    // Model is already on the GPU, nothing to do.
                    continue;
                }
                voxel_registry_gpu.load_gpu_model(*model);
            }
        }
    }

    pub fn region_data_buffer(&self) -> &ResourceId<Buffer> {
        self.region_data_buffer.buffer()
    }

    pub fn region_window_side_length(&self) -> Vector3<u32> {
        Vector3::new(
            self.region_window.side_length,
            self.region_window.side_length,
            self.region_window.side_length,
        )
    }

    pub fn region_window_anchor(&self) -> RegionPos {
        self.region_window.region_anchor
    }

    pub fn region_window_offset(&self) -> Vector3<u32> {
        self.region_window.window_offset
    }

    pub fn region_window_buffer(&self) -> &ResourceId<Buffer> {
        self.region_window
            .region_window_buffer
            .as_ref()
            .expect("Region window buffer should exist by now")
    }
}

struct RegionAllocator {}
