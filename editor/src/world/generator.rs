use std::{collections::HashSet, sync::Arc};

use nalgebra::Vector3;
use rand::{Rng, SeedableRng};
use rogue_engine::{
    common::morton,
    consts,
    event::{EventReader, Events},
    input::{keyboard::Key, Input},
    noise::{
        fbm::{Fbm, FbmOptions},
        perlin::PerlinNoise,
    },
    resource::{Res, ResMut},
    task::tasks::Tasks,
    voxel::{
        attachment::Attachment,
        flat::VoxelModelFlat,
        sft::VoxelModelSFT,
        sft_compressed::VoxelModelSFTCompressed,
        voxel_registry::{self, VoxelModelRegistry},
    },
    world::{
        region::{RegionTree, WorldRegion, WorldRegionNode},
        region_iter::RegionIter,
        region_map::{ChunkId, ChunkLOD, ChunkPos, RegionEvent, RegionMap, RegionPos},
        world_streaming::ChunkStreamEvent,
    },
};
use rogue_macros::Resource;
use wide::CmpGt;

#[derive(Resource)]
pub struct WorldGenerator {
    chunk_generator: Arc<ChunkGenerator>,

    generated_chunks: HashSet<ChunkId>,
    chunk_stream_event_reader: EventReader<ChunkStreamEvent>,

    /// The number of chunks currently being generated on background threads.
    currently_generating_chunks: u32,
    /// The maximum number of chunks that we can generate in the background at once. Should never
    /// be larger than the number of available background threads.
    max_generating_chunks: u32,
    generated_chunk_recv: std::sync::mpsc::Receiver<GeneratedChunkData>,
    generated_chunk_send: std::sync::mpsc::Sender<GeneratedChunkData>,

    pub paused: bool,
}

impl WorldGenerator {
    pub fn new(tasks: &Tasks) -> Self {
        let center = Vector3::new(0, 0, 0);

        let (generated_chunk_send, generated_chunk_recv) =
            std::sync::mpsc::channel::<GeneratedChunkData>();
        Self {
            chunk_generator: Arc::new(ChunkGenerator::new(0)),

            generated_chunks: HashSet::new(),
            chunk_stream_event_reader: EventReader::new(),

            currently_generating_chunks: 0,
            max_generating_chunks: tasks.total_thread_count().get() as u32,
            generated_chunk_recv,
            generated_chunk_send,

            paused: false,
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn update(
        mut generator: ResMut<WorldGenerator>,
        mut region_map: ResMut<RegionMap>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut tasks: ResMut<Tasks>,
        input: Res<Input>,
        events: Res<Events>,
    ) {
        let generator = &mut *generator;

        if input.is_key_pressed(Key::G) {
            generator.paused = !generator.paused;
        }

        for event in generator.chunk_stream_event_reader.read(&events) {
            let chunk_id = event.chunk_id;

            let threads_free =
                generator.currently_generating_chunks < generator.max_generating_chunks;
            if generator.paused || !threads_free {
                break;
            }

            if generator.generated_chunks.contains(&chunk_id) {
                continue;
            }
            generator.generated_chunks.insert(chunk_id);

            {
                let chunk_generator = generator.chunk_generator.clone();
                let generated_chunk_send = generator.generated_chunk_send.clone();
                tasks.spawn_background_process(move || {
                    let sft =
                        chunk_generator.generate_chunk_sft(chunk_id.chunk_pos, chunk_id.chunk_lod);
                    generated_chunk_send
                        .send(GeneratedChunkData {
                            generated_sft: sft,
                            chunk_id,
                        })
                        .expect("Failed to send generated chunk data to main thread.");
                });
            }
        }

        // Check for any generated chunks from background threads.
        while let Ok(GeneratedChunkData {
            generated_sft: sft,
            chunk_id,
        }) = generator.generated_chunk_recv.try_recv()
        {
            generator.currently_generating_chunks =
                generator.currently_generating_chunks.saturating_sub(1);

            // Non-empty chunk so update region and parent node accordingly.
            let sft_id = (!sft.is_empty()).then(|| voxel_registry.register_voxel_model(sft));
            region_map.set_chunk(chunk_id, sft_id);
        }
    }
}

struct GeneratedRegion {
    tree: RegionTree,
    models: Vec<VoxelModelSFTCompressed>,
}

impl GeneratedRegion {
    pub fn new() -> Self {
        Self {
            tree: RegionTree::new_empty(),
            models: Vec::new(),
        }
    }
}

pub struct GeneratedChunkData {
    // None if there cannot be any voxels in this chunk at the requested LOD.
    generated_sft: VoxelModelSFTCompressed,
    chunk_id: ChunkId,
}

pub struct ChunkGenerator {
    sample_offset: Vector3<f32>,
    height_noise: Fbm<PerlinNoise>,
    density_noise: Fbm<PerlinNoise>,
}

impl ChunkGenerator {
    pub fn new(seed: u64) -> Self {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let offset = Vector3::new(
            (rng.next_u32() % 25600) as f32 * 0.01,
            (rng.next_u32() % 25600) as f32 * 0.01,
            (rng.next_u32() % 25600) as f32 * 0.01,
        );
        Self {
            sample_offset: offset,
            height_noise: Fbm::new(
                PerlinNoise::new(seed),
                FbmOptions {
                    lacunarity: 1.7,
                    octaves: 16,
                    gain: 0.6,
                },
            ),
            density_noise: Fbm::new(
                PerlinNoise::new(seed),
                FbmOptions {
                    lacunarity: 2.0,
                    octaves: 4,
                    gain: 0.5,
                },
            ),
        }
    }

    pub fn lipshitz_constant() -> f32 {
        todo!()
    }

    pub fn sample_density(&self, world_voxel_pos: Vector3<f32>) -> f32 {
        let voxel_frequency = 0.01;
        let sample_pos = world_voxel_pos * voxel_frequency + self.sample_offset;
        let height = self.height_noise.noise_2d(sample_pos.x, sample_pos.z) * 50.0;

        let mut density = self
            .density_noise
            .noise_3d(sample_pos.x, sample_pos.y, sample_pos.z);

        // 10m of possible 3d noise, rest must be from y shaping.
        const SURFACE_FALLOFF: f32 = 10.0;
        density += -(world_voxel_pos.y - height) / SURFACE_FALLOFF;

        density
    }

    pub fn sample_chunk_density(&self, world_chunk_pos: ChunkPos, lod: ChunkLOD) -> f32 {
        let voxel_meter_size = lod.voxel_meter_size();
        let chunk_voxel_pos = world_chunk_pos.map(|x| {
            x as f32
                * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as f32
                * consts::voxel::VOXEL_METER_LENGTH
        });
        let half_chunk_meter_size =
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as f32 * voxel_meter_size * 0.5;
        return self.sample_density(
            chunk_voxel_pos
                + Vector3::new(
                    half_chunk_meter_size,
                    half_chunk_meter_size,
                    half_chunk_meter_size,
                ),
        );
    }

    pub fn generate_chunk_sft(
        &self,
        world_chunk_pos: ChunkPos,
        lod: ChunkLOD,
    ) -> VoxelModelSFTCompressed {
        let voxel_meter_size = lod.voxel_meter_size();
        let chunk_voxel_pos = world_chunk_pos.map(|x| {
            x as f32
                * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as f32
                * consts::voxel::VOXEL_METER_LENGTH
        });
        let mut flat = VoxelModelFlat::new_empty(Vector3::new(
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
        ));
        flat.initialize_attachment_buffers(&Attachment::BMAT);
        for local_z in 0..consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH {
            let voxel_z = chunk_voxel_pos.z + local_z as f32 * voxel_meter_size;
            for local_y in 0..consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH {
                let voxel_y = chunk_voxel_pos.y + local_y as f32 * voxel_meter_size;
                for local_x in 0..(consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH / 8) {
                    // Operating on 8 voxels along the x direction at a time with SIMD, targetting
                    // AVX2.
                    let voxel_x = wide::f32x8::splat(
                        chunk_voxel_pos.x + (local_x as f32 * 8.0 * voxel_meter_size),
                    ) + (wide::f32x8::new([0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
                        * voxel_meter_size);
                    let mut density = wide::f32x8::splat(0.0);
                    for i in 0..8 {
                        let d = self.sample_density(Vector3::new(
                            voxel_x.as_array()[i],
                            voxel_y,
                            voxel_z,
                        ));
                        density.as_mut_array()[i] = d;
                    }

                    let presence_bitmask = density.simd_gt(wide::f32x8::splat(0.0)).to_bitmask();
                    let index = flat.get_voxel_index(Vector3::new(local_x * 8, local_y, local_z));
                    flat.presence_data.set_bits(index, 8, presence_bitmask);
                    flat.attachment_presence_data
                        .get_mut(Attachment::BMAT_ID)
                        .unwrap()
                        .set_bits(index, 8, presence_bitmask);
                    flat.attachment_data.get_mut(Attachment::BMAT_ID).unwrap()[index..index + 8]
                        .fill(0);
                }
            }
        }
        let sft = VoxelModelSFTCompressed::from(&flat);
        sft
    }
}
