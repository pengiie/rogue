/// Since 8 voxels is 1 meter, this corresponds to 16x16 meter chunks.
pub const TERRAIN_CHUNK_LENGTH: u32 = 256;
pub const TERRAIN_CHUNK_VOLUME: u32 = TERRAIN_CHUNK_LENGTH.pow(3);
pub const TERRAIN_CHUNK_WORLD_UNIT_LENGTH: f32 =
    TERRAIN_CHUNK_LENGTH as f32 * VOXEL_WORLD_UNIT_LENGTH;

pub const VOXELS_PER_WORLD_UNIT: u32 = 8;
pub const VOXEL_WORLD_UNIT_LENGTH: f32 = 1.0 / VOXELS_PER_WORLD_UNIT as f32;

pub const MODEL_ESVO_SCHEMA: u32 = 1;
pub const MODEL_FLAT_SCHEMA: u32 = 2;
