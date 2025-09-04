use core::f32;

use log::debug;
use nalgebra::Vector3;
use noise::{MultiFractal, NoiseFn};

use crate::{
    common::color::Color,
    consts::{
        self,
        voxel::{VOXELS_PER_METER, VOXEL_METER_LENGTH},
    },
    engine::voxel::attachment::BuiltInMaterial,
};

use super::{
    attachment::{Attachment, AttachmentMap, PTMaterial},
    cursor::VoxelEditInfo,
    flat::VoxelModelFlat,
    voxel_world::VoxelWorld,
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

    pub fn generate_chunk(&self, voxel_world: &mut VoxelWorld, chunk_position: Vector3<i32>) {
        let mut used_attachments = AttachmentMap::new();
        used_attachments.insert(Attachment::BMAT_ID, Attachment::BMAT);

        let color_noise = noise::Fbm::<noise::Perlin>::new(0).set_octaves(3);
        let height_freq = 1.0 / (32.0 * consts::voxel::VOXELS_PER_METER as f64);
        let ground_height_noise_gen = noise::Fbm::<noise::Perlin>::new(0)
            .set_octaves(4)
            .set_frequency(height_freq);
        let structure_freq = 1.0 / (128.0 * consts::voxel::VOXELS_PER_METER as f64);
        let structure_noise_gen = noise::Fbm::<noise::Perlin>::new(0)
            .set_frequency(structure_freq)
            .set_octaves(4)
            .set_persistence(0.8);
        let perlin = self.perlin.clone();
        voxel_world.apply_voxel_edit_async(
            VoxelEditInfo {
                world_voxel_position: chunk_position
                    * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32,
                world_voxel_length: Vector3::new(
                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                ),
                attachment_map: used_attachments,
            },
            move |mut voxel, world_pos, local_pos| {
                let x = (world_pos.x - world_pos.x.rem_euclid(1)) as f64;
                let y = (world_pos.y - world_pos.y.rem_euclid(1)) as f64;
                let z = (world_pos.z - world_pos.z.rem_euclid(1)) as f64;

                if (y * consts::voxel::VOXEL_METER_LENGTH as f64).abs() >= 64.0 {
                    return;
                }

                let mut density = structure_noise_gen.get([x, y, z]);

                let height_noise = ground_height_noise_gen.get([x, z]);
                let height_range = 8.0 * VOXELS_PER_METER as f64;
                let structure_noise = structure_noise_gen.get([x, z]) * 0.5 + 0.3;
                let structure_range = 60.0 * VOXELS_PER_METER as f64;
                let base_ground = 0.0;
                let ground_height =
                    base_ground + height_noise * height_range + structure_noise * structure_range;
                let ground_bias = ((ground_height - y) / height_range);
                density += ground_bias;

                if (density > 0.0 && y > -3.0 * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as f64) {
                    let r_var = color_noise.get([x / 7.0, y / 7.0, z / 7.0]) as f32;
                    let dirting = ((density - 0.1) * 2.0).clamp(0.0, 1.0) as f32;

                    let material = BuiltInMaterial::new(if dirting < 0.5 {
                        consts::voxel::attachment::bt::GRASS_ID
                    } else {
                        consts::voxel::attachment::bt::DIRT_ID
                    });

                    voxel.set_attachment(Attachment::BMAT_ID, &[material.encode()])
                } else {
                    voxel.set_removed();
                }
            },
        );
    }
}
