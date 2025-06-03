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
        used_attachments.insert(Attachment::PTMATERIAL_ID, Attachment::PTMATERIAL);

        let color_noise = noise::Fbm::<noise::Perlin>::new(0).set_octaves(3);
        let ground_height_noise = noise::Fbm::<noise::Perlin>::new(0)
            .set_octaves(4)
            .set_frequency(VOXEL_METER_LENGTH as f64 / 16.0);
        let structure_nosie = noise::Fbm::<noise::Perlin>::new(0)
            .set_frequency(VOXEL_METER_LENGTH as f64 / 24.0)
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
                let freq = 24.0;
                let x = world_pos.x as f64;
                let y = world_pos.y as f64;
                let z = world_pos.z as f64;
                let nx = world_pos.x as f64 / freq;
                let ny = world_pos.y as f64 / freq;
                let nz = world_pos.z as f64 / freq;

                let mut density = structure_nosie.get([x, y, z]);

                let height_noise = ground_height_noise.get([x, z]);
                let height_range = 8.0 * VOXELS_PER_METER as f64;
                let base_ground = 0.0;
                let ground_height = base_ground + height_noise * height_range;
                let ground_bias = ((ground_height - y) / height_range);
                density += ground_bias;

                if (density > 0.0) {
                    let r_var = color_noise.get([x / 7.0, y / 7.0, z / 7.0]) as f32;
                    let dirting = ((density - 0.1) * 2.0).clamp(0.0, 1.0) as f32;

                    let grass = Color::new_srgb(0.5 + 0.2 * r_var, 0.9, 0.05);
                    let dirt = Color::new_srgb(0.17, 0.05 + r_var * 0.03, 0.01 + r_var * 0.02);

                    let color = grass.mix(&dirt, dirting);
                    voxel.set_attachment(
                        Attachment::PTMATERIAL_ID,
                        &[Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                            color.into(),
                        ))],
                    )
                } else {
                    voxel.set_removed();
                }
            },
        );
    }
}
