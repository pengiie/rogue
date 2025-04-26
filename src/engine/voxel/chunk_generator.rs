use core::f32;

use log::debug;
use nalgebra::Vector3;
use noise::{MultiFractal, NoiseFn};

use crate::{common::color::Color, consts};

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
                let ny = y / (freq);
                let nz = world_pos.z as f64 / freq;

                let mut density = perlin.get([nx, ny, nz]);

                const SHAPING_ANCHOR: f64 = -50.0;
                // Higher y meaning higher shaping, aka. less density.
                let shaping = (((y - SHAPING_ANCHOR) / 64.0) * 0.5 + 0.5).clamp(0.0, 1.0);
                // Smoothstep
                let shaping = 3.0 * (shaping * shaping) - 2.0 * shaping * shaping * shaping;
                // Higher shaping value correlates to more dense.
                let shaping = 1.0 - shaping;

                if (shaping * shaping * density + shaping > 0.5) {
                    let r_var = color_noise.get([x / 7.0, y / 7.0, z / 7.0]);
                    let color = Color::new_srgb(0.5 + 0.2 * r_var as f32, 0.9, 0.05);
                    voxel.set_attachment(
                        Attachment::PTMATERIAL_ID,
                        &[Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                            color.into(),
                        ))],
                    )
                }
            },
        );
    }
}
