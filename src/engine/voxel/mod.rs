use crate::app::App;

use super::resource::ResourceBank;

mod voxel_constants;
pub mod vox_consts {
    pub use super::voxel_constants::*;
}
pub mod attachment;
pub mod esvo;
pub mod flat;
pub mod unit;
pub mod voxel;
pub mod voxel_allocator;
pub mod voxel_terrain;
pub mod voxel_transform;
pub mod voxel_world;

pub fn initialize_voxel_world_resources(rb: &mut ResourceBank) {
    let voxel_world = voxel_world::VoxelWorld::new();
    let voxel_world_gpu = voxel_world::VoxelWorldGpu::new();
    let voxel_terrain = voxel_terrain::VoxelTerrain::new();

    rb.insert(voxel_world);
    rb.insert(voxel_world_gpu);
    rb.insert(voxel_terrain);
}
