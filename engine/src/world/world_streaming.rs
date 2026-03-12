use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use rogue_macros::Resource;

use crate::{
    common::morton,
    consts,
    event::{EventReader, Events},
    resource::ResMut,
    world::region_map::{
        ChunkEvent, ChunkEventType, ChunkId, ChunkLOD, ChunkPos, RegionEvent, RegionEventType,
        RegionMap, RegionPos,
    },
};
use nalgebra::Vector3;

pub struct WorldStreamingOptions {
    pub origin_region: RegionPos,
    pub region_load_distance: i32,
    pub max_loaded_chunks: u32,
}

impl Default for WorldStreamingOptions {
    fn default() -> Self {
        Self {
            origin_region: RegionPos::new(0, 0, 0),
            region_load_distance: 2,
            max_loaded_chunks: 512,
        }
    }
}

#[derive(Resource)]
pub struct WorldChunkStreamer {
    options: WorldStreamingOptions,
    next_regions: VecDeque<RegionPos>,

    /// Priority queue for chunk load requests for rendering.
    chunk_queue: BinaryHeap<ChunkStreamRequest>,
    visited_regions: HashSet<RegionPos>,

    /// Chunks which have a present voxel model loaded in memory, empty chunks which are visible
    loaded_chunks: HashSet<ChunkId>,
    /// Chunks which the streamer requested to load but hasn't loaded yet.
    queued_chunks: HashSet<ChunkId>,

    region_event_reader: EventReader<RegionEvent>,
    chunk_event_reader: EventReader<ChunkEvent>,
}

#[derive(Debug, Clone)]
struct ChunkStreamRequest {
    chunk_id: ChunkId,
    /// Manhattan Distance in meters from camera to the edge of this chunk.
    distance_to_camera: f32,
}

impl ChunkStreamRequest {
    fn cost(&self) -> f32 {
        const full_res_radius: f32 = consts::voxel::TERRAIN_CHUNK_METER_LENGTH * 16.0;
        let full_res_t = self.distance_to_camera / full_res_radius;

        if self.chunk_id.chunk_lod.is_lowest_res() {
            //    const LOWEST_RES_RADIUS: f32 = full_res_radius * 10.0;
            return 10000.0;
        }

        // Cost from 0 to 1 for full resolution chunks.
        if self.chunk_id.chunk_lod.is_full_res() && full_res_t < 1.0 {}
        return full_res_t;

        //return full_res_t + (self.chunk_id.chunk_lod.as_tree_height() as f32 * 256.0) - 0.5;
    }
}

impl PartialEq for ChunkStreamRequest {
    fn eq(&self, other: &Self) -> bool {
        self.chunk_id == other.chunk_id
    }
}

impl Eq for ChunkStreamRequest {}

impl PartialOrd for ChunkStreamRequest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // We want the lowest cost to have the highest priority, so we reverse the ordering.
        other.cost().partial_cmp(&self.cost())
    }
}

impl Ord for ChunkStreamRequest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

/// Emitted whenever the chunk streamer requests a chunk to be loaded.
pub struct ChunkStreamEvent {
    pub chunk_id: ChunkId,
}

impl WorldChunkStreamer {
    pub fn new(options: WorldStreamingOptions) -> Self {
        let mut next_regions = VecDeque::new();
        next_regions.push_back(options.origin_region);
        Self {
            options,
            next_regions,
            visited_regions: HashSet::new(),

            queued_chunks: HashSet::new(),
            loaded_chunks: HashSet::new(),

            region_event_reader: EventReader::new(),
            chunk_event_reader: EventReader::new(),
            chunk_queue: BinaryHeap::new(),
        }
    }

    fn region_neighbors(region_pos: &RegionPos) -> Vec<RegionPos> {
        vec![
            region_pos + RegionPos::new(1, 0, 0),
            region_pos + RegionPos::new(-1, 0, 0),
            region_pos + RegionPos::new(0, 1, 0),
            region_pos + RegionPos::new(0, -1, 0),
            region_pos + RegionPos::new(0, 0, 1),
            region_pos + RegionPos::new(0, 0, -1),
        ]
    }

    pub fn update(
        mut streamer: ResMut<Self>,
        mut region_map: ResMut<RegionMap>,
        mut events: ResMut<Events>,
    ) {
        let streamer = &mut *streamer;

        let camera_pos = Vector3::new(0.0, 0.0, 0.0);
        let camera_region_pos = (camera_pos * (1.0 / consts::voxel::TERRAIN_REGION_METER_LENGTH))
            .map(|x| x.floor() as i32);
        let camera_chunk_pos = (camera_pos * (1.0 / consts::voxel::TERRAIN_CHUNK_METER_LENGTH))
            .map(|x| x.floor() as i32);

        for event in streamer.chunk_event_reader.read(&events) {
            match event.event_type {
                ChunkEventType::Loaded | ChunkEventType::Updated => {
                    if region_map.get_chunk_model(&event.chunk_id).is_some() {
                        streamer.loaded_chunks.insert(event.chunk_id);
                    }
                    streamer.queued_chunks.remove(&event.chunk_id);
                }
                ChunkEventType::Unloaded => {
                    streamer.loaded_chunks.remove(&event.chunk_id);
                }
            }
        }

        let can_stream = (streamer.loaded_chunks.len() + streamer.queued_chunks.len())
            < streamer.options.max_loaded_chunks as usize;
        if let Some(request) = streamer.chunk_queue.pop()
            && can_stream
        {
            events.push(ChunkStreamEvent {
                chunk_id: request.chunk_id,
            });
            streamer.queued_chunks.insert(request.chunk_id);
        }

        // Enqueue next chunks to be streamed into the priority queue.
        let streamer = &mut *streamer;
        if let Some(region_pos) = streamer.next_regions.pop_front() {
            if streamer.visited_regions.contains(&region_pos) {
                return;
            }
            streamer.visited_regions.insert(region_pos);

            let region_distance = (region_pos - camera_region_pos).cast::<f32>().norm() as i32;
            if region_distance > streamer.options.region_load_distance {
                // Don't load this region since it's outside our render distance.
                return;
            }

            let mut region_nodes = vec![(
                ChunkLOD::new_lowest_res(),
                /*Chunk pos*/ region_pos.into_chunk_pos(),
            )];
            while let Some((chunk_lod, chunk_pos)) = region_nodes.pop() {
                let hl = chunk_lod.leaf_chunk_length() as f32 * 0.5;
                let chunk_meter_pos = (chunk_pos.cast::<f32>() + Vector3::new(hl, hl, hl))
                    * consts::voxel::TERRAIN_CHUNK_METER_LENGTH;
                let distance_to_camera = chunk_meter_pos.metric_distance(&camera_pos);

                const LOD0_RENDER_DISTANCE: f32 = consts::voxel::TERRAIN_CHUNK_METER_LENGTH * 16.0;
                let visible_size = (chunk_lod.leaf_chunk_length() as f32
                    * consts::voxel::TERRAIN_CHUNK_METER_LENGTH)
                    / distance_to_camera;
                // Minimum portion of the screen the chunk should take up to be loaded.
                const MIN_CHUNK_SCREEN_SIZE: f32 =
                    consts::voxel::TERRAIN_CHUNK_METER_LENGTH / LOD0_RENDER_DISTANCE;
                if visible_size < MIN_CHUNK_SCREEN_SIZE && !chunk_lod.is_lowest_res() {
                    if chunk_lod.is_full_res() {
                        //log::info!(
                        //    "Chunk {:?} is visible but too small to justify full res, loading at lower LOD. {}, {}",
                        //    chunk_pos,
                        //    visible_size,
                        //    distance_to_camera
                        //);
                    }
                    continue;
                }

                if chunk_lod.is_full_res() {
                    streamer.chunk_queue.push(ChunkStreamRequest {
                        chunk_id: ChunkId {
                            chunk_pos,
                            chunk_lod,
                        },
                        distance_to_camera,
                    });
                    continue;
                }

                let node_data = region_map.get_region(&region_pos);
                let child_lod = ChunkLOD::new(chunk_lod.0 - 1);
                let child_chunk_size = child_lod.leaf_chunk_length();
                for i in 0..64u32 {
                    let child_pos = morton::morton_decode(i as u64);
                    region_nodes.push((
                        child_lod,
                        chunk_pos + child_pos.cast::<i32>() * child_chunk_size as i32,
                    ));
                }
            }

            for neighbor in Self::region_neighbors(&region_pos) {
                streamer.next_regions.push_back(neighbor);
            }
        }
    }
}
