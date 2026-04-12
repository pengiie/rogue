use crate::consts;
use crate::world::terrain::chunk_pos::ChunkPos;
use nalgebra::Vector3;
use std::ops::{Add, Deref, Mul, Sub};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RegionPos(Vector3<i32>);

impl RegionPos {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self(Vector3::new(x, y, z))
    }

    pub fn new_vec(vec: Vector3<i32>) -> Self {
        Self(vec)
    }

    pub fn zeros() -> Self {
        Self(Vector3::zeros())
    }

    pub fn from_world_pos(world_pos: &Vector3<f32>) -> Self {
        Self::new_vec(
            (world_pos * (1.0 / consts::voxel::TERRAIN_REGION_METER_LENGTH))
                .map(|x| x.floor() as i32),
        )
    }

    pub fn from_world_voxel_pos(world_voxel_pos: &Vector3<i32>) -> Self {
        Self::new_vec(
            world_voxel_pos
                .map(|x| x.div_euclid(consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32)),
        )
    }

    pub fn into_chunk_pos(&self) -> ChunkPos {
        ChunkPos::new(self.map(|x| x * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32))
    }
}

impl Deref for RegionPos {
    type Target = Vector3<i32>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<RegionPos> for Vector3<i32> {
    fn from(region_pos: RegionPos) -> Self {
        region_pos.0
    }
}

impl From<Vector3<i32>> for RegionPos {
    fn from(vec: Vector3<i32>) -> Self {
        RegionPos(vec)
    }
}

impl Add<Vector3<i32>> for RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: Vector3<i32>) -> Self::Output {
        RegionPos(self.0 + rhs)
    }
}

impl Add<RegionPos> for Vector3<i32> {
    type Output = RegionPos;

    fn add(self, rhs: RegionPos) -> Self::Output {
        RegionPos(rhs.0 + self)
    }
}

impl Add<RegionPos> for RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: RegionPos) -> Self::Output {
        RegionPos(self.0 + rhs.0)
    }
}

impl Add<&RegionPos> for RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: &RegionPos) -> Self::Output {
        RegionPos(self.0 + rhs.0)
    }
}

impl Add<RegionPos> for &RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: RegionPos) -> Self::Output {
        RegionPos(self.0 + rhs.0)
    }
}

impl Mul<i32> for RegionPos {
    type Output = RegionPos;

    fn mul(self, rhs: i32) -> Self::Output {
        RegionPos(self.0 * rhs)
    }
}

impl Sub<RegionPos> for RegionPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: RegionPos) -> Self::Output {
        self.0 - rhs.0
    }
}

impl Sub<Vector3<i32>> for RegionPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: Vector3<i32>) -> Self::Output {
        self.0 - rhs
    }
}

impl Sub<RegionPos> for Vector3<i32> {
    type Output = Vector3<i32>;

    fn sub(self, rhs: RegionPos) -> Self::Output {
        self - rhs.0
    }
}
