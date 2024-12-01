use core::panic;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use egui::ahash::HashSetExt;
use log::debug;
use nalgebra::{zero, Vector2, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{aabb::AABB, color::Color, morton, ring_queue::RingQueue},
    engine::{
        ecs::ecs_world::ECSWorld,
        event::Events,
        resource::{Res, ResMut},
        voxel::{
            attachment::{Attachment, PTMaterial},
            esvo::VoxelModelESVO,
            flat::VoxelModelFlat,
            voxel::{RenderableVoxelModel, VoxelModel},
        },
        window::time::Timer,
    },
    settings::Settings,
};

use super::{
    chunk_generator::ChunkGenerator,
    voxel_constants,
    voxel_transform::VoxelModelTransform,
    voxel_world::{VoxelModelId, VoxelWorld},
};

type ChunkModelType = VoxelModelESVO;

pub enum VoxelTerrainEvent {
    UpdateRenderDistance { chunk_render_distance: u32 },
}

#[derive(Hash, PartialEq, Eq)]
pub struct ChunkTicket {
    chunk_position: Vector3<i32>,
}

#[derive(Resource)]
pub struct VoxelTerrain {
    chunk_render_distance: u32,

    chunk_tree: Option<ChunkTree>,
    chunk_queue: ChunkProcessingQueue,
    chunk_loader: ChunkLoader,
    queue_timer: Timer,
}

impl VoxelTerrain {
    pub fn new(settings: &Settings) -> Self {
        Self {
            chunk_render_distance: settings.chunk_render_distance,

            chunk_tree: None,
            chunk_queue: ChunkProcessingQueue::new(settings),
            chunk_loader: ChunkLoader::new(Vector3::zeros(), settings.chunk_render_distance),
            queue_timer: Timer::new(Duration::from_millis(100)),
        }
    }

    fn initialize_chunk_tree(&mut self) {
        let chunk_tree = ChunkTree::new_with_center(
            Vector3::new(0, 0, 0),
            //self.chunk_render_distance.next_power_of_two(),
            16,
        );
        self.chunk_tree = Some(chunk_tree);
    }

    pub fn update_post_physics(
        mut terrain: ResMut<VoxelTerrain>,
        events: Res<Events>,
        settings: Res<Settings>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
    ) {
        let terrain: &mut VoxelTerrain = &mut terrain;

        if terrain.chunk_tree.is_none() {
            terrain.initialize_chunk_tree();
        };

        // Try enqueue any non enqueued chunks.
        if !terrain.chunk_queue.chunk_queue.is_full() {
            terrain.chunk_loader.enqueue_chunks(
                terrain.chunk_tree.as_mut().unwrap(),
                &mut terrain.chunk_queue,
            );
        }

        // Process next chunk.
        terrain.chunk_tree.as_mut().unwrap().is_dirty = false;
        terrain.chunk_queue.handle_enqueued_chunks();
        terrain
            .chunk_queue
            .handle_finished_chunks(terrain.chunk_tree.as_mut().unwrap(), &mut voxel_world);
    }

    pub fn try_enqueue_load_chunk(&mut self, chunk_position: Vector3<i32>) {
        let Some(chunk_tree) = &mut self.chunk_tree else {
            debug!("Chunk tree isn't loaded!!!");
            return;
        };

        if !chunk_tree.is_world_chunk_loaded(chunk_position)
            && !chunk_tree.is_world_chunk_enqueued(chunk_position)
        {
            if self.chunk_queue.try_enqueue_chunk(chunk_position) {
                self.chunk_tree
                    .as_mut()
                    .unwrap()
                    .set_world_chunk_enqued(chunk_position);
            }
        }
    }

    pub fn origin_render_range(&self) -> (Range<i32>, Range<i32>, Range<i32>) {
        assert!(self.chunk_tree.is_some());
        let chunk_tree = self.chunk_tree.as_ref().unwrap();
        let min = chunk_tree.chunk_origin;
        let max = chunk_tree
            .chunk_origin
            .map(|x| x + chunk_tree.chunk_side_length as i32);

        let ranges = min.zip_map(&max, |a, b| a..b);
        (ranges.x.clone(), ranges.y.clone(), ranges.z.clone())
    }

    pub fn chunk_tree(&self) -> &ChunkTree {
        self.chunk_tree
            .as_ref()
            .expect("Chunk tree should have been constructed by now.")
    }

    pub fn is_chunk_tree_dirty(&self) -> bool {
        self.chunk_tree.as_ref().is_some_and(|tree| tree.is_dirty)
    }
}

pub struct FinishedChunk {
    chunk_position: Vector3<i32>,
    esvo: Option<ChunkModelType>,
}

impl FinishedChunk {
    pub fn is_empty(&self) -> bool {
        self.esvo.is_none()
    }
}

pub struct ChunkProcessingQueue {
    chunk_queue: RingQueue<ChunkTicket>,
    chunk_generator: ChunkGenerator,
    chunk_handler_pool: rayon::ThreadPool,
    chunk_handler_count: u32,
    finished_chunk_recv: Receiver<FinishedChunk>,
    finished_chunk_send: Sender<FinishedChunk>,
}

impl ChunkProcessingQueue {
    pub fn new(settings: &Settings) -> Self {
        let (finished_chunk_send, finished_chunk_recv) = std::sync::mpsc::channel();
        Self {
            chunk_queue: RingQueue::with_capacity(settings.chunk_queue_capacity as usize),
            chunk_generator: ChunkGenerator::new(),
            chunk_handler_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(settings.chunk_queue_capacity as usize)
                .build()
                .unwrap(),
            chunk_handler_count: 0,
            finished_chunk_recv,
            finished_chunk_send,
        }
    }

    pub fn try_enqueue_chunk(&mut self, chunk_position: Vector3<i32>) -> bool {
        if self.chunk_queue.is_full() {
            return false;
        }

        self.chunk_queue.push(ChunkTicket { chunk_position });
        return true;
    }

    pub fn handle_finished_chunks(
        &mut self,
        chunk_tree: &mut ChunkTree,
        voxel_world: &mut VoxelWorld,
    ) {
        // Loops until the reciever is empty.
        'lp: loop {
            match self.finished_chunk_recv.try_recv() {
                Ok(finished_chunk) => {
                    self.chunk_handler_count -= 1;
                    if finished_chunk.is_empty() {
                        chunk_tree.set_world_chunk_empty(finished_chunk.chunk_position);
                        debug!("Recieved empty chunk {:?}", finished_chunk.chunk_position);
                    } else {
                        let chunk_name = format!(
                            "chunk_{}_{}_{}",
                            finished_chunk.chunk_position.x,
                            finished_chunk.chunk_position.y,
                            finished_chunk.chunk_position.z
                        );
                        let voxel_model_id = voxel_world.register_renderable_voxel_model(
                            chunk_name,
                            VoxelModel::new(finished_chunk.esvo.unwrap()),
                        );
                        chunk_tree.set_world_chunk_data(
                            finished_chunk.chunk_position,
                            ChunkData { voxel_model_id },
                        );
                        debug!(
                            "Recieved finished chunk {:?}",
                            finished_chunk.chunk_position
                        );
                    }
                }
                Err(err) => match err {
                    std::sync::mpsc::TryRecvError::Disconnected => {
                        panic!("Shouldn't be disconnected")
                    }
                    _ => break 'lp,
                },
            }
        }
    }

    pub fn handle_enqueued_chunks(&mut self) {
        if !self.chunk_queue.is_empty()
            && self.chunk_handler_count < self.chunk_handler_pool.current_num_threads() as u32
        {
            let ticket = self.chunk_queue.try_pop().unwrap();
            debug!("Enqueued chunk {:?}", ticket.chunk_position);
            let send = self.finished_chunk_send.clone();
            self.chunk_handler_count += 1;
            self.chunk_handler_pool.spawn(move || {
                let flat = ChunkGenerator::generate_chunk(ticket.chunk_position);
                send.send(FinishedChunk {
                    chunk_position: ticket.chunk_position,
                    esvo: flat.map(|flat| flat.into()),
                });
            });
        }
    }
}

enum ChunkTreeNode {
    Node(Box<ChunkTree>),
    Leaf(ChunkData),
    Empty,
    Enqeueud,
    Unloaded,
}

impl ChunkTreeNode {
    fn is_leaf(&self) -> bool {
        match self {
            ChunkTreeNode::Leaf(_) | ChunkTreeNode::Empty => true,
            ChunkTreeNode::Node(_) | ChunkTreeNode::Unloaded | ChunkTreeNode::Enqeueud => false,
        }
    }
}

/// Octree that holds chunks.
pub struct ChunkTree {
    children: [ChunkTreeNode; 8],
    chunk_side_length: u32,
    chunk_half_length: u32,

    /// Where the origin is the the -XYZ corner of the tree.
    chunk_origin: Vector3<i32>,
    is_dirty: bool,
}

impl ChunkTree {
    /// Constructed with the origin interpreted
    fn new_with_center(chunk_center_origin: Vector3<i32>, half_chunk_length: u32) -> Self {
        let corner_origin = chunk_center_origin.map(|x| x - half_chunk_length as i32);
        return Self::new(corner_origin, half_chunk_length * 2);
    }

    fn new(corner_origin: Vector3<i32>, chunk_side_length: u32) -> Self {
        assert!(chunk_side_length >= 2);
        assert!(chunk_side_length.is_power_of_two());
        Self {
            children: core::array::from_fn(|_| ChunkTreeNode::Unloaded),
            chunk_side_length,
            chunk_half_length: chunk_side_length >> 1,
            chunk_origin: corner_origin,
            is_dirty: false,
        }
    }

    fn in_bounds(&self, relative_position: &Vector3<i32>) -> bool {
        relative_position
            .iter()
            .all(|x| *x >= 0 && *x < self.chunk_side_length as i32)
    }

    fn world_pos_traversal_morton(&self, chunk_world_position: Vector3<i32>) -> u64 {
        let relative_position = chunk_world_position - self.chunk_origin;
        assert!(
            self.in_bounds(&relative_position),
            "Given chunk position is out of bounds and can't be accessed from this chunk tree."
        );
        let morton = morton::morton_encode(relative_position.map(|x| x as u32));
        morton::morton_traversal(morton, self.chunk_side_length.trailing_zeros())
    }

    fn is_world_chunk_loaded(&self, chunk_world_position: Vector3<i32>) -> bool {
        let morton = self.world_pos_traversal_morton(chunk_world_position);
        return self.is_loaded(morton);
    }

    fn is_world_chunk_enqueued(&self, chunk_world_position: Vector3<i32>) -> bool {
        let morton = self.world_pos_traversal_morton(chunk_world_position);
        return self.is_enqueued(morton);
    }

    fn set_world_chunk_enqued(&mut self, chunk_world_position: Vector3<i32>) {
        let morton = self.world_pos_traversal_morton(chunk_world_position);
        return self.set_chunk_enqueued(morton);
    }

    fn set_chunk_enqueued(&mut self, morton: u64) {
        let octant = (morton & 7) as usize;

        if self.is_pre_leaf() {
            self.children[octant] = ChunkTreeNode::Enqeueud;
        } else {
            if let ChunkTreeNode::Node(subtree) = &mut self.children[octant] {
                subtree.set_chunk_enqueued(morton >> 3);
            } else {
                assert!(!self.children[octant].is_leaf());
                let mask = Vector3::new(
                    (octant & 1) as i32,
                    ((octant >> 1) & 1) as i32,
                    ((octant >> 2) & 1) as i32,
                );
                let child_world_origin = self.chunk_origin + mask * self.chunk_half_length as i32;

                let mut subtree = ChunkTree::new(child_world_origin, self.chunk_half_length);
                subtree.set_chunk_enqueued(morton >> 3);
                self.children[octant] = ChunkTreeNode::Node(Box::new(subtree));
            }
        }
    }

    fn set_world_chunk_empty(&mut self, chunk_world_position: Vector3<i32>) {
        let morton = self.world_pos_traversal_morton(chunk_world_position);
        self.is_dirty = true;
        return self.set_chunk_empty(morton);
    }

    fn set_chunk_empty(&mut self, morton: u64) {
        let octant = (morton & 7) as usize;

        if self.is_pre_leaf() {
            self.children[octant] = ChunkTreeNode::Empty;
        } else {
            if let ChunkTreeNode::Node(subtree) = &mut self.children[octant] {
                subtree.set_chunk_empty(morton >> 3);
            } else {
                assert!(!self.children[octant].is_leaf());
                let mask = Vector3::new(
                    (octant & 1) as i32,
                    ((octant >> 1) & 1) as i32,
                    ((octant >> 2) & 1) as i32,
                );
                let child_world_origin = self.chunk_origin + mask * self.chunk_half_length as i32;

                let mut subtree = ChunkTree::new(child_world_origin, self.chunk_half_length);
                subtree.set_chunk_empty(morton >> 3);
                self.children[octant] = ChunkTreeNode::Node(Box::new(subtree));
            }
        }
    }

    fn set_world_chunk_data(&mut self, chunk_world_position: Vector3<i32>, chunk_data: ChunkData) {
        self.is_dirty = true;
        let morton = self.world_pos_traversal_morton(chunk_world_position);
        return self.set_chunk_data(morton, chunk_data);
    }

    fn set_chunk_data(&mut self, morton: u64, data: ChunkData) {
        let octant = (morton & 7) as usize;

        if self.is_pre_leaf() {
            self.children[octant] = ChunkTreeNode::Leaf(data);
        } else {
            if let ChunkTreeNode::Node(subtree) = &mut self.children[octant] {
                subtree.set_chunk_data(morton >> 3, data);
            } else {
                assert!(!self.children[octant].is_leaf());
                let mask = Vector3::new(
                    (octant & 1) as i32,
                    ((octant >> 1) & 1) as i32,
                    ((octant >> 2) & 1) as i32,
                );
                let child_world_origin = self.chunk_origin + mask * self.chunk_half_length as i32;

                let mut subtree = ChunkTree::new(child_world_origin, self.chunk_half_length);
                subtree.set_chunk_data(morton >> 3, data);
                self.children[octant] = ChunkTreeNode::Node(Box::new(subtree));
            }
        }
    }

    fn is_pre_leaf(&self) -> bool {
        self.chunk_side_length == 2
    }

    fn is_loaded(&self, morton: u64) -> bool {
        let octant = (morton & 7) as usize;
        let child = &self.children[octant];

        return match child {
            ChunkTreeNode::Node(subtree) => subtree.is_loaded(morton >> 3),
            ChunkTreeNode::Leaf(_) | ChunkTreeNode::Empty => true,
            ChunkTreeNode::Enqeueud | ChunkTreeNode::Unloaded => false,
        };
    }

    fn is_enqueued(&self, morton: u64) -> bool {
        let octant = (morton & 7) as usize;
        let child = &self.children[octant];

        return match child {
            ChunkTreeNode::Node(subtree) => subtree.is_enqueued(morton >> 3),
            ChunkTreeNode::Enqeueud => true,
            _ => false,
        };
    }

    pub fn volume(&self) -> u32 {
        self.chunk_side_length * self.chunk_side_length * self.chunk_side_length
    }

    pub fn chunk_origin(&self) -> Vector3<i32> {
        self.chunk_origin
    }

    pub fn chunk_side_length(&self) -> u32 {
        self.chunk_side_length
    }
}

struct ChunkData {
    voxel_model_id: VoxelModelId,
}

// LOD 0 is the highest resolution.
// Each subsequent LOD halves the voxel resolution, this still renders with the same
// chunk voxel length but the chunk will have double the scaling.
impl ChunkData {
    fn world_length(lod: u32) -> f32 {
        voxel_constants::TERRAIN_CHUNK_WORLD_UNIT_LENGTH * (lod + 1) as f32
    }
}

pub struct ChunkTreeGpuNode {}

/// This isn't actually a tree for now we just do a flat 3d array in morton order so we can dda
/// through the chunks. In the future I'll make this into a tree when render distances get large.
pub struct ChunkTreeGpu {
    pub data: Vec<u32>,
}

impl ChunkTreeGpu {
    pub fn build(chunk_tree: &ChunkTree, voxel_model_map: HashMap<VoxelModelId, u32>) -> Self {
        let volume = chunk_tree.volume();
        let mut data = vec![0xFFFF_FFFF; volume as usize];

        let mut to_process = vec![(Vector3::<u32>::new(0, 0, 0), chunk_tree)];
        while !to_process.is_empty() {
            let (curr_origin, curr_node) = to_process.pop().unwrap();
            for (octant, child) in curr_node.children.iter().enumerate() {
                let mask = Vector3::new(
                    (octant & 1) as u32,
                    ((octant >> 1) & 1) as u32,
                    ((octant >> 2) & 1) as u32,
                );
                let child_position = curr_origin + mask * curr_node.chunk_half_length;

                match child {
                    ChunkTreeNode::Node(sub_tree) => {
                        to_process.push((child_position, sub_tree));
                    }
                    ChunkTreeNode::Leaf(chunk) => {
                        // Chunk voxel model may not exist in the voxel model map yet if it is
                        // still updating the gpu-side buffer, so it is not ready to be rendered.
                        if let Some(chunk_ptr) = voxel_model_map.get(&chunk.voxel_model_id) {
                            let morton = morton::morton_encode(child_position);
                            data[morton as usize] = *chunk_ptr;
                        }
                    }
                    ChunkTreeNode::Empty | ChunkTreeNode::Enqeueud | ChunkTreeNode::Unloaded => {}
                }
            }
        }

        Self { data }
    }
}

pub struct ChunkLoader {
    max_radius: u32,
    curr_radius: u32,
    curr_index: u32,
    current_chunk_anchor: Vector3<i32>,
}

impl ChunkLoader {
    pub fn new(chunk_anchor: Vector3<i32>, render_distance: u32) -> Self {
        Self {
            max_radius: render_distance,
            curr_radius: 0,
            curr_index: 0,
            current_chunk_anchor: chunk_anchor,
        }
    }

    pub fn update_anchor(&mut self, new_chunk_anchor: Vector3<i32>) {
        todo!()
    }

    /// Enqueues chunks in an iterator fashion so we don't waste time rechecking chunks.
    pub fn enqueue_chunks(
        &mut self,
        chunk_tree: &mut ChunkTree,
        chunk_queue: &mut ChunkProcessingQueue,
    ) {
        if self.curr_radius == self.max_radius {
            return;
        }

        let curr_diameter = (self.curr_radius + 1) * 2;
        let curr_area = curr_diameter.pow(2);
        if self.curr_index >= curr_area * 6 {
            self.curr_radius += 1;
            self.curr_index = 0;
            return;
        }

        let face = self.curr_index / curr_area;
        debug!("face {} {} {}", face, self.curr_index, self.curr_radius);
        let local_index = self.curr_index % curr_area;
        let local_position = Vector2::new(
            (local_index % curr_diameter) as i32,
            (local_index / curr_diameter) as i32,
        );
        let mut chunk_position =
            self.current_chunk_anchor - Vector3::new(1, 1, 1) * (self.curr_radius + 1) as i32;
        match face {
            // Bottom Face
            0 => chunk_position += Vector3::new(local_position.x, 0, local_position.y),
            // Top Face
            1 => {
                chunk_position +=
                    Vector3::new(local_position.x, curr_diameter as i32 - 1, local_position.y)
            }
            // Front Face
            2 => chunk_position += Vector3::new(local_position.x, local_position.y, 0),
            // Back Face
            3 => {
                chunk_position +=
                    Vector3::new(local_position.x, local_position.y, curr_diameter as i32 - 1)
            }
            // Left Face
            4 => chunk_position += Vector3::new(0, local_position.x, local_position.y),
            // Right Face
            5 => {
                chunk_position +=
                    Vector3::new(curr_diameter as i32 - 1, local_position.x, local_position.y)
            }
            _ => unreachable!(),
        }

        self.try_enqueue_chunk(ChunkTicket { chunk_position }, chunk_tree, chunk_queue);
        self.curr_index += 1;
    }

    fn try_enqueue_chunk(
        &mut self,
        chunk_ticket: ChunkTicket,
        chunk_tree: &mut ChunkTree,
        chunk_queue: &mut ChunkProcessingQueue,
    ) {
        let chunk_position = chunk_ticket.chunk_position;

        if !chunk_tree.is_world_chunk_loaded(chunk_position)
            && !chunk_tree.is_world_chunk_enqueued(chunk_position)
        {
            if chunk_queue.try_enqueue_chunk(chunk_position) {
                chunk_tree.set_world_chunk_enqued(chunk_position);
            }
        }
    }
}
