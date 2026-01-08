use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::world::region_map::RegionPos;
use crate::graphics::{
    backend::{Buffer, ResourceId},
    device::DeviceResource,
    gpu_allocator::GpuBufferAllocator,
};
use crate::resource::ResMut;

/// 0 is no LOD and every additional is an i:1 ratio for voxels
/// along one axis.
struct ChunkLOD(u32);

impl ChunkLOD {
    pub fn new(lod: u32) -> Self {
        Self(lod)
    }

    pub fn voxel_scale(&self) -> u32 {
        // 2^i
        1 << self.0
    }
}

// Flat array of chunks which acts as a sliding window as the player.
struct TerrainRenderableLODLevel {
    pub side_length: u32,
    pub lod: ChunkLOD,
    pub gpu_region_ptr: Vec<u32>,

    pub window_offset: Vector3<u32>,
    pub region_anchor: RegionPos,

    pub region_data_buffer: Option<GpuBufferAllocator>,
    pub region_window_buffer: Option<ResourceId<Buffer>>,
}

impl TerrainRenderableLODLevel {}

#[derive(Resource)]
pub struct TerrainRenderable {
    lods: Vec<TerrainRenderableLODLevel>,

    // The center region we are rendering from.
    region_anchor: RegionPos,
    region_render_distance: u32,
}

impl TerrainRenderable {
    pub fn new() -> Self {
        Self {
            lods: Vec::new(),

            region_anchor: RegionPos::zeros(),
            region_render_distance: 0,
        }
    }

    /// Handles the iterator of which regions should be loaded next for rendering.
    pub fn update(terrain: ResMut<TerrainRenderable>) {}

    pub fn update_gpu_objects(terrain: ResMut<TerrainRenderable>, device: ResMut<DeviceResource>) {}

    pub fn write_render_data(terrain: ResMut<TerrainRenderable>, device: ResMut<DeviceResource>) {}
}
