use crate::engine::voxel::cursor::VoxelCursor;

use super::entity::player::Player;

/// The graphics `DeviceResource` has been inserted before this.
pub fn init_post_graphics(app: &mut crate::app::App) {
    app.run_system(Player::spawn);
    app.insert_resource(VoxelCursor::new());
}
