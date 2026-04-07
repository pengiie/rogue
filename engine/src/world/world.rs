use nalgebra::Vector3;
use crate::world::terrain::region_map::RegionMap;
use crate::entity::ecs_world::{ECSWorld, Entity};
use crate::voxel::voxel_registry::VoxelModelRegistry;

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
