use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::engine::{
    entity::ecs_world::{ECSWorld, Entity},
    voxel::voxel_registry::VoxelModelRegistry,
    world::region_map::RegionMap,
};

pub enum WorldTraceInfo {
    Terrain { global_voxel_pos: Vector3<i32> },
    Entity { entity_id: Entity },
}

pub struct World;

impl World {
    pub fn trace(
        ecs_world: &ECSWorld,
        voxel_registry: &VoxelModelRegistry,
        region_map: &RegionMap,
    ) -> Option<WorldTraceInfo> {
        todo!()
    }
}
