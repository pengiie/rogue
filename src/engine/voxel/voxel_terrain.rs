use rogue_macros::Resource;

use super::voxel_world::VoxelModelId;

#[derive(Resource)]
pub struct VoxelTerrain {}

impl VoxelTerrain {
    pub fn new() -> Self {
        Self {}
    }
}

struct ChunkTree {
    chunks: Vec<Chunk>,
}

impl ChunkTree {}

struct Chunk {
    esvo: VoxelModelId,
}
