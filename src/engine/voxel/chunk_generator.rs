use core::f32;

use nalgebra::Vector3;

use crate::common::color::Color;

use super::{
    attachment::{Attachment, PTMaterial},
    flat::VoxelModelFlat,
    voxel_constants,
};

pub struct ChunkGenerator {}

impl ChunkGenerator {
    pub fn new() -> Self {
        Self {}
    }

    pub fn generate_chunk(chunk_position: Vector3<i32>) -> Option<VoxelModelFlat> {
        let mut flat = VoxelModelFlat::new_empty(Vector3::new(
            voxel_constants::TERRAIN_CHUNK_LENGTH,
            voxel_constants::TERRAIN_CHUNK_LENGTH,
            voxel_constants::TERRAIN_CHUNK_LENGTH,
        ));

        if chunk_position.y < -1 || chunk_position.y > 0 {
            return None;
        }

        let world_voxel_min = chunk_position * voxel_constants::TERRAIN_CHUNK_LENGTH as i32;
        let world_voxel_max = world_voxel_min
            + Vector3::new(
                voxel_constants::TERRAIN_CHUNK_LENGTH as i32,
                voxel_constants::TERRAIN_CHUNK_LENGTH as i32,
                voxel_constants::TERRAIN_CHUNK_LENGTH as i32,
            );

        for x in world_voxel_min.x..world_voxel_max.x {
            for y in world_voxel_min.y..world_voxel_max.y {
                for z in world_voxel_min.z..world_voxel_max.z {
                    let target_y = (f32::atan(f32::sin(x as f32 / 10.0)) * 16.0) as i32
                        + (f32::cos(z as f32 / 17.0) * 20.5) as i32;
                    if y >= target_y - 5 && y <= target_y {
                        //if y == (f32::sin(
                        //    (x as f32 * voxel_constants::VOXEL_WORLD_UNIT_LENGTH / 2.0)
                        //        * f32::consts::TAU,
                        //) * 4.0) as i32
                        //{
                        let local_pos = Vector3::new(
                            x - world_voxel_min.x,
                            y - world_voxel_min.y,
                            z - world_voxel_min.z,
                        )
                        .map(|x| x as u32);
                        let mut voxel = flat.get_voxel_mut(local_pos);
                        voxel.set_attachment(
                            Attachment::PTMATERIAL,
                            Some(Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                                Color::new_srgb(0.5, 0.75, 1.0).into(),
                            ))),
                        )
                    }
                }
            }
        }

        return Some(flat);
    }
}
