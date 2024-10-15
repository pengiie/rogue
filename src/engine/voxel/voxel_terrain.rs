use rogue_macros::Resource;

#[derive(Resource)]
pub struct VoxelTerrain {}

impl VoxelTerrain {
    pub fn new() -> Self {
        Self {}
    }
}
