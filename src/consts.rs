pub mod voxel {
    pub use crate::engine::voxel::voxel_constants::*;
}

pub mod gfx {
    /// The # of milliseconds that have to pass between attempts to invalidate pipelines.
    /// Pipeline invalidation is just checking if any shader files were modified, and invalidating
    /// the the entire pipeline cache.
    pub const PIPELINE_INVALIDATION_TIMER_MS: u32 = 250;
}
