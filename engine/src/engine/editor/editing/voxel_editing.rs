use rogue_macros::Resource;

#[derive(Resource)]
pub struct VoxelEditing {}

impl VoxelEditing {
    pub fn new() -> Self {
        Self {}
    }

    pub fn editor_update() {}
}
