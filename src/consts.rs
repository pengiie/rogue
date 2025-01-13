pub mod voxel {
    pub const TERRAIN_CHUNK_METER_LENGTH: f32 = 2.0;
    pub const TERRAIN_CHUNK_VOXEL_LENGTH: u32 =
        (TERRAIN_CHUNK_METER_LENGTH * VOXELS_PER_METER as f32) as u32;
    pub const TERRAIN_CHUNK_VOXEL_VOLUME: u32 = TERRAIN_CHUNK_VOXEL_LENGTH.pow(3);

    pub const VOXELS_PER_METER: u32 = 16;
    pub const VOXEL_METER_LENGTH: f32 = 1.0 / VOXELS_PER_METER as f32;

    pub const MODEL_ESVO_SCHEMA: u32 = 1;
    pub const MODEL_FLAT_SCHEMA: u32 = 2;
}

pub mod gfx {
    /// The # of milliseconds that have to pass between attempts to invalidate pipelines.
    /// Pipeline invalidation is just checking if any shader files were modified, and invalidating
    /// the the entire pipeline cache.
    pub const PIPELINE_INVALIDATION_TIMER_MS: u32 = 250;
}
