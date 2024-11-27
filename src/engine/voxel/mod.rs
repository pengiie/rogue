use crate::{app::App, settings::Settings};

use super::resource::ResourceBank;

pub mod attachment;
pub mod chunk_generator;
pub mod esvo;
pub mod flat;
pub mod unit;
pub mod voxel;
pub mod voxel_allocator;
pub mod voxel_constants;
pub mod voxel_terrain;
pub mod voxel_transform;
pub mod voxel_world;

pub fn initialize_voxel_world_resources(rb: &mut ResourceBank) {
    let voxel_world = voxel_world::VoxelWorld::new();
    let voxel_world_gpu = voxel_world::VoxelWorldGpu::new();
    let voxel_terrain = voxel_terrain::VoxelTerrain::new(&rb.get_resource::<Settings>());

    rb.insert(voxel_world);
    rb.insert(voxel_world_gpu);
    rb.insert(voxel_terrain);
}
