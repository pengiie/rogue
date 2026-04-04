use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use nalgebra::Vector3;
use rand::{Rng, SeedableRng};
use rogue_engine::{
    asset::asset::GameAssetPath,
    common::morton,
    consts,
    event::{EventReader, Events},
    input::{Input, keyboard::Key},
    material::{MaterialBank, MaterialUpdateEvent},
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
        voxel::VoxelMaterialData,
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

struct GeneratorMaterials;

impl GeneratorMaterials {
    const GRASS: &'static str = "./materials/grass/grass_mat.rmat";
    const DIRT: &'static str = "./materials/dirt/dirt_mat.rmat";

    const MATERIALS: [&'static str; 2] = [Self::GRASS, Self::DIRT];
}

#[derive(Resource)]
pub struct WorldGenerator {
    chunk_generator: Option<Arc<ChunkGenerator>>,
    requested_materials: bool,

    generated_chunks: HashSet<ChunkId>,
    chunk_stream_event_reader: EventReader<ChunkStreamEvent>,

    /// The number of chunks currently being generated on background threads.
    currently_generating_chunks: u32,
    /// The maximum number of chunks that we can generate in the background at once. Should never
    /// be larger than the number of available background threads.
    max_generating_chunks: u32,
    generated_chunk_recv: std::sync::mpsc::Receiver<GeneratedChunkData>,
    generated_chunk_send: std::sync::mpsc::Sender<GeneratedChunkData>,

    update_material_event_reader: EventReader<MaterialUpdateEvent>,

    pub paused: bool,
}

impl WorldGenerator {
    pub fn new(tasks: &Tasks) -> Self {
        let center = Vector3::new(0, 0, 0);

        let (generated_chunk_send, generated_chunk_recv) =
            std::sync::mpsc::channel::<GeneratedChunkData>();
        Self {
            requested_materials: false,
            chunk_generator: None,

            generated_chunks: HashSet::new(),
            chunk_stream_event_reader: EventReader::new(),

            currently_generating_chunks: 0,
            max_generating_chunks: tasks.total_thread_count().get() as u32,
            generated_chunk_recv,
            generated_chunk_send,

            update_material_event_reader: EventReader::new(),

            paused: true,
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
        mut material_bank: ResMut<MaterialBank>,
        input: Res<Input>,
        events: Res<Events>,
    ) {
        let generator = &mut *generator;

        if input.is_key_pressed(Key::G) {
            generator.paused = !generator.paused;
        }

        for event in generator.update_material_event_reader.read(&events) {
            generator.chunk_generator = None;
        }

        if generator.chunk_generator.is_none() {
            if !generator.requested_materials {
                generator.requested_materials = true;
                for material in GeneratorMaterials::MATERIALS {
                    material_bank
                        .request_material(GameAssetPath::from_relative_path(Path::new(material)));
                }
            }

            let mut all_loaded = true;
            for material in GeneratorMaterials::MATERIALS {
                if material_bank
                    .asset_path_map
                    .get(&GameAssetPath::from_relative_path(Path::new(material)))
                    .is_none()
                {
                    all_loaded = false;
                    break;
                }
            }

            if all_loaded {
                generator.chunk_generator = Some(Arc::new(ChunkGenerator::new(0, &material_bank)));
            }
        }

        let can_generate_chunks = generator.currently_generating_chunks
            < generator.max_generating_chunks
            && !generator.paused
            && generator.chunk_generator.is_some();
        if !can_generate_chunks {
            return;
        }

        for event in generator.chunk_stream_event_reader.read(&events) {
            let chunk_id = event.chunk_id;

            if generator.generated_chunks.contains(&chunk_id) {
                continue;
            }
            generator.generated_chunks.insert(chunk_id);

            {
                let chunk_generator = generator.chunk_generator.clone().unwrap();
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
            let sft_id = (!sft.is_empty()).then(|| voxel_registry.register_voxel_model(sft, None));
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
    material_map: HashMap<String, u32>,
}

impl ChunkGenerator {
    pub fn new(seed: u64, material_bank: &MaterialBank) -> Self {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let offset = Vector3::new(
            (rng.next_u32() % 25600) as f32 * 0.01,
            (rng.next_u32() % 25600) as f32 * 0.01,
            (rng.next_u32() % 25600) as f32 * 0.01,
        );

        let mut material_map = HashMap::new();
        for material in GeneratorMaterials::MATERIALS {
            material_map.insert(
                material.to_owned(),
                material_bank
                    .asset_path_map
                    .get(&GameAssetPath::from_relative_path(Path::new(material)))
                    .clone()
                    .unwrap()
                    .index(),
            );
        }

        Self {
            sample_offset: offset,
            height_noise: Fbm::new(
                PerlinNoise::new(seed),
                FbmOptions {
                    lacunarity: 1.7,
                    octaves: 8,
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
            material_map,
        }
    }

    pub fn lipshitz_constant() -> f32 {
        todo!()
    }

    pub fn sample_shaping(
        &self,
        world_voxel_pos: Vector3<f32>,
    ) -> (/*density*/ f32, /*height*/ f32) {
        let height_frequency = 0.003;
        let height_sample_pos = world_voxel_pos * height_frequency + self.sample_offset;
        let height = self
            .height_noise
            .noise_2d(height_sample_pos.x, height_sample_pos.z)
            * 0.0;

        let density_frequency = 0.01;
        let density_sample_pos = world_voxel_pos * density_frequency + self.sample_offset;
        let mut density = self.density_noise.noise_3d(
            density_sample_pos.x,
            density_sample_pos.y,
            density_sample_pos.z,
        );

        // 10m of possible 3d noise, rest must be from y shaping.
        const SURFACE_FALLOFF: f32 = 0.0;
        density += -(world_voxel_pos.y - height) / SURFACE_FALLOFF;

        (density, height)
    }

    pub fn sample_material(&self, world_voxel_pos: Vector3<f32>, density: f32, height: f32) -> u64 {
        let mut material_id = 0;
        if density < 0.1 {
            material_id = *self.material_map.get(GeneratorMaterials::GRASS).unwrap();
        }
        material_id = *self.material_map.get(GeneratorMaterials::DIRT).unwrap();
        return VoxelMaterialData::Unbaked(material_id).encode();
    }

    pub fn sample_chunk_shaping(&self, world_chunk_pos: ChunkPos, lod: ChunkLOD) -> (f32, f32) {
        let voxel_meter_size = lod.voxel_meter_size();
        let chunk_voxel_pos = world_chunk_pos.map(|x| {
            x as f32
                * consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as f32
                * consts::voxel::VOXEL_METER_LENGTH
        });
        let half_chunk_meter_size =
            consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as f32 * voxel_meter_size * 0.5;
        return self.sample_shaping(
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
                    let mut material = wide::u64x8::splat(0);
                    for i in 0..8 {
                        let (d, h) = self.sample_shaping(Vector3::new(
                            voxel_x.as_array()[i],
                            voxel_y,
                            voxel_z,
                        ));
                        density.as_mut_array()[i] = d;
                        if d > 0.0 {
                            material.as_mut_array()[i] = self.sample_material(
                                Vector3::new(voxel_x.as_array()[i], voxel_y, voxel_z),
                                d,
                                h,
                            );
                        }
                    }

                    let presence_bitmask = density.simd_gt(wide::f32x8::splat(0.0)).to_bitmask();
                    let index = flat.get_voxel_index(Vector3::new(local_x * 8, local_y, local_z));
                    flat.presence_data.set_bits(index, 8, presence_bitmask);
                    flat.attachment_presence_data
                        .get_mut(Attachment::BMAT_ID)
                        .unwrap()
                        .set_bits(index, 8, presence_bitmask);
                    let attachment_offset = index * Attachment::BMAT.size() as usize;
                    flat.attachment_data.get_mut(Attachment::BMAT_ID).unwrap()[attachment_offset
                        ..(attachment_offset + 8 * Attachment::BMAT.size() as usize)]
                        .copy_from_slice(bytemuck::cast_slice::<u64, u32>(material.as_array()));
                    for i in index..(index + 8) {
                        if density.as_array()[i - index] > 0.0 {
                            let attachment_value =
                                flat.attachment_data.get_mut(Attachment::BMAT_ID).unwrap()
                                    [i * Attachment::BMAT.size() as usize];
                            let attachment_value_b =
                                flat.attachment_data.get_mut(Attachment::BMAT_ID).unwrap()
                                    [i * Attachment::BMAT.size() as usize + 1];
                            //log::info!(
                            //    "Attachment data is {:032b} {:032b}",
                            //    attachment_value,
                            //    attachment_value_b
                            //);
                        }
                    }
                }
            }
        }
        let sft = VoxelModelSFTCompressed::from(&flat);
        sft
    }
}
