use core::f32;

use log::debug;
use nalgebra::Vector3;
use noise::NoiseFn;

use crate::{common::color::Color, consts};

use super::{
    attachment::{Attachment, PTMaterial},
    flat::VoxelModelFlat,
};

#[derive(Clone)]
pub struct ChunkGenerator {
    perlin: noise::Perlin,
}

impl ChunkGenerator {
    pub fn new(seed: u32) -> Self {
        Self {
            perlin: noise::Perlin::new(seed),
        }
    }

    pub fn generate_chunk(&mut self, chunk_position: Vector3<i32>) -> Option<VoxelModelFlat> {
        let mut flat = VoxelModelFlat::new_empty(Vector3::new(
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
        ));

        // if chunk_position.y < -2 || chunk_position.y > 3 {
        //     return None;
        // }

        let world_voxel_min = chunk_position * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32;
        let world_voxel_max = world_voxel_min
            + Vector3::new(
                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
            );

        for x in world_voxel_min.x..world_voxel_max.x {
            for y in world_voxel_min.y..world_voxel_max.y {
                for z in world_voxel_min.z..world_voxel_max.z {
                    let target_y = (f32::atan(f32::sin(x as f32 / 10.0)) * 16.0) as i32
                        + (f32::cos(z as f32 / 17.0) * 20.5) as i32;
                    let freq = 24.0;
                    let nx = x as f64 / freq;
                    let ny = y as f64 / (freq);
                    let nz = z as f64 / freq;
                    let noise_three = self.perlin.get([nx, ny, nz]);
                    //let target_y = (noise_y as i32).clamp(
                    //    -(consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32),
                    //    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                    //);
                    //if y >= target_y - 4 && y <= target_y {
                    //if noise_three > 0.3 {
                    //if y == (f32::sin(
                    //    (x as f32 * voxel_constants::VOXEL_WORLD_UNIT_LENGTH / 2.0)
                    //        * f32::consts::TAU,
                    //) * 4.0) as i32
                    //{
                    //if x % 2 == 0 {
                    let local_pos = Vector3::new(
                        x - world_voxel_min.x,
                        y - world_voxel_min.y,
                        z - world_voxel_min.z,
                    )
                    .map(|x| x as u32);
                    let mut voxel = flat.get_voxel_mut(local_pos);
                    let color = Color::new_srgb(
                        local_pos.x as f32 / 64.0,
                        local_pos.y as f32 / 64.0,
                        local_pos.z as f32 / 64.0,
                    );
                    voxel.set_attachment(
                        Attachment::PTMATERIAL,
                        Some(Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                            color.into(),
                        ))),
                    )
                    //}
                    //                   }
                }
            }
        }

        return Some(flat);
    }
}
