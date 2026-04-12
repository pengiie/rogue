use crate::common::morton;
use crate::consts;
use crate::world::terrain::region_pos::RegionPos;
use nalgebra::Vector3;
use std::ops::{Add, Deref, Mul, Sub};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChunkPos(Vector3<i32>);

impl ChunkPos {
    pub fn new(vec: Vector3<i32>) -> Self {
        Self(vec)
    }

    pub fn get_region_pos(&self) -> RegionPos {
        RegionPos::new_vec(
            self.map(|x| x.div_euclid(consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32)),
        )
    }

    pub fn get_min_world_voxel_pos(&self) -> Vector3<i32> {
        self.0 * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32
    }

    pub fn from_world_voxel_pos(world_voxel_pos: &Vector3<i32>) -> Self {
        Self::new(
            world_voxel_pos.map(|x| x.div_euclid(consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32)),
        )
    }

    pub fn get_chunk_traversal(&self) -> u64 {
        let local_pos = self
            .0
            .map(|x| (x.rem_euclid(consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32)) as u32);
        let morton = morton::morton_encode(local_pos);
        morton::morton_traversal_thc(morton, consts::voxel::TERRAIN_REGION_TREE_HEIGHT)
    }
}

impl Deref for ChunkPos {
    type Target = Vector3<i32>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<ChunkPos> for Vector3<i32> {
    fn from(region_pos: ChunkPos) -> Self {
        region_pos.0
    }
}

impl From<Vector3<i32>> for ChunkPos {
    fn from(vec: Vector3<i32>) -> Self {
        ChunkPos(vec)
    }
}

impl Add<Vector3<i32>> for ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: Vector3<i32>) -> Self::Output {
        ChunkPos(self.0 + rhs)
    }
}

impl Add<ChunkPos> for Vector3<i32> {
    type Output = ChunkPos;

    fn add(self, rhs: ChunkPos) -> Self::Output {
        ChunkPos(rhs.0 + self)
    }
}

impl Add<ChunkPos> for ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: ChunkPos) -> Self::Output {
        ChunkPos(self.0 + rhs.0)
    }
}

impl Add<&ChunkPos> for ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: &ChunkPos) -> Self::Output {
        ChunkPos(self.0 + rhs.0)
    }
}

impl Add<ChunkPos> for &ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: ChunkPos) -> Self::Output {
        ChunkPos(self.0 + rhs.0)
    }
}

impl Mul<i32> for ChunkPos {
    type Output = ChunkPos;

    fn mul(self, rhs: i32) -> Self::Output {
        ChunkPos(self.0 * rhs)
    }
}

impl Sub<ChunkPos> for ChunkPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: ChunkPos) -> Self::Output {
        self.0 - rhs.0
    }
}

impl Sub<Vector3<i32>> for ChunkPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: Vector3<i32>) -> Self::Output {
        self.0 - rhs
    }
}

impl Sub<ChunkPos> for Vector3<i32> {
    type Output = Vector3<i32>;

    fn sub(self, rhs: ChunkPos) -> Self::Output {
        self - rhs.0
    }
}
