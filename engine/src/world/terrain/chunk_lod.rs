use crate::consts;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChunkLOD(pub u32);

impl ChunkLOD {
    /// Max LOD in this case is the lowest level of detail, naming is a bit unintuitive but is
    /// because full resolution starts at 0.
    pub const MAX_LOD: u32 = consts::voxel::TERRAIN_REGION_TREE_HEIGHT;
    /// AKA MIN lod;
    pub const FULL_RES_LOD: ChunkLOD = ChunkLOD::new_full_res();

    pub const fn new_full_res() -> Self {
        Self::new(0)
    }

    pub fn is_full_res(&self) -> bool {
        self.0 == 0
    }

    pub fn is_lowest_res(&self) -> bool {
        self.0 == Self::MAX_LOD
    }

    pub fn from_tree_height(tree_height: u32) -> Self {
        assert!(
            tree_height <= consts::voxel::TERRAIN_REGION_TREE_HEIGHT,
            "Cannot request an LOD which is higher (lower resolution) than the maximum region tree height, max is {} and requested {}",
            Self::MAX_LOD,
            tree_height
        );
        Self(Self::MAX_LOD - tree_height)
    }

    pub fn region_chunk_length(&self) -> u32 {
        consts::voxel::TERRAIN_REGION_CHUNK_LENGTH >> (self.0 * 2)
    }

    pub fn leaf_chunk_length(&self) -> u32 {
        1 << (self.0 * 2)
    }

    pub fn chunk_to_region_proportion(&self) -> f32 {
        1.0 / (self.region_chunk_length() as f32)
    }

    pub fn as_tree_height(&self) -> u32 {
        consts::voxel::TERRAIN_REGION_TREE_HEIGHT - self.0
    }

    /// LOD 0 is the highest detail level with each LOD fourthing
    /// the voxel resolution since we use 64-trees.
    pub const fn new(lod: u32) -> Self {
        assert!(lod <= Self::MAX_LOD);
        Self(lod)
    }

    pub fn new_lowest_res() -> Self {
        Self::new(Self::MAX_LOD)
    }

    pub fn max_tree_height(&self) -> u32 {
        (consts::voxel::TERRAIN_REGION_CHUNK_LENGTH.trailing_zeros() >> 1) - self.0
    }

    pub fn voxel_meter_size(&self) -> f32 {
        consts::voxel::VOXEL_METER_LENGTH * (4u32.pow(self.0) as f32)
    }
}