use core::panic;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use egui::ahash::HashSetExt;
use nalgebra::{zero, SimdValue, Vector2, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{
        aabb::AABB,
        color::Color,
        morton,
        ray::{Ray, RayDDA},
        ring_queue::RingQueue,
    },
    consts,
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
    thc::VoxelModelTHC,
    voxel_transform::VoxelModelTransform,
    voxel_world::{VoxelModelId, VoxelWorld},
};

type ChunkModelType = VoxelModelFlat;

pub enum VoxelTerrainEvent {
    UpdateRenderDistance { chunk_render_distance: u32 },
}

#[derive(Hash, PartialEq, Eq)]
pub struct ChunkTicket {
    chunk_position: Vector3<i32>,
}

pub struct VoxelTerrain {
    pub chunk_render_distance: u32,

    pub chunk_tree: ChunkTree,
    pub chunk_queue: ChunkProcessingQueue,
    pub chunk_loader: ChunkLoader,
    pub queue_timer: Timer,
}

impl VoxelTerrain {
    pub fn new(settings: &Settings) -> Self {
        let chunk_tree = ChunkTree::new_with_center(
            Vector3::new(0, 0, 0),
            settings.chunk_render_distance.next_power_of_two(),
        );

        Self {
            chunk_render_distance: settings.chunk_render_distance,

            chunk_tree,
            chunk_queue: ChunkProcessingQueue::new(settings),
            chunk_loader: ChunkLoader::new(Vector3::zeros(), settings.chunk_render_distance),
            queue_timer: Timer::new(Duration::from_millis(100)),
        }
    }

    pub fn try_enqueue_load_chunk(&mut self, chunk_position: Vector3<i32>) {
        if !self.chunk_tree.is_world_chunk_loaded(chunk_position)
            && !self.chunk_tree.is_world_chunk_enqueued(chunk_position)
        {
            if self.chunk_queue.try_enqueue_chunk(chunk_position) {
                self.chunk_tree.set_world_chunk_enqued(chunk_position);
            }
        }
    }

    pub fn origin_render_range(&self) -> (Range<i32>, Range<i32>, Range<i32>) {
        let min = self.chunk_tree.chunk_origin;
        let max = self
            .chunk_tree
            .chunk_origin
            .map(|x| x + self.chunk_tree.chunk_side_length as i32);

        let ranges = min.zip_map(&max, |a, b| a..b);
        (ranges.x.clone(), ranges.y.clone(), ranges.z.clone())
    }

    pub fn chunks_aabb(&self) -> AABB {
        let origin =
            self.chunk_tree.chunk_origin.cast::<f32>() * consts::voxel::TERRAIN_CHUNK_METER_LENGTH;
        let side_length =
            self.chunk_tree.chunk_side_length as f32 * consts::voxel::TERRAIN_CHUNK_METER_LENGTH;
        return AABB::new_two_point(
            origin,
            origin + Vector3::new(side_length, side_length, side_length),
        );
    }

    pub fn chunks_dda(&self, ray: &Ray) -> RayDDA {
        let world_aabb = self.chunks_aabb();
        let side_length = self.chunk_tree.chunk_side_length;
        return ray.begin_dda(
            &world_aabb,
            Vector3::new(side_length, side_length, side_length),
        );
    }

    pub fn chunk_tree(&self) -> &ChunkTree {
        &self.chunk_tree
    }

    pub fn is_chunk_tree_dirty(&self) -> bool {
        self.chunk_tree.is_dirty
    }
}

pub struct FinishedChunk {
    pub chunk_position: Vector3<i32>,
    pub esvo: Option<ChunkModelType>,
}

impl FinishedChunk {
    pub fn is_empty(&self) -> bool {
        self.esvo.is_none()
    }
}

pub struct ChunkProcessingQueue {
    pub chunk_queue: RingQueue<ChunkTicket>,
    pub chunk_generator: ChunkGenerator,
    pub chunk_handler_pool: rayon::ThreadPool,
    pub chunk_handler_count: u32,
    pub finished_chunk_recv: Receiver<FinishedChunk>,
    pub finished_chunk_send: Sender<FinishedChunk>,
}

impl ChunkProcessingQueue {
    pub fn new(settings: &Settings) -> Self {
        let (finished_chunk_send, finished_chunk_recv) = std::sync::mpsc::channel();
        Self {
            chunk_queue: RingQueue::with_capacity(settings.chunk_queue_capacity as usize),
            chunk_generator: ChunkGenerator::new(0),
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
    }

    pub fn handle_enqueued_chunks(&mut self) {
        if !self.chunk_queue.is_empty()
            && self.chunk_handler_count < self.chunk_handler_pool.current_num_threads() as u32
        {
            // TODO: Cache chunk generator perlin noise as some global thing or something idk.
            let mut generator = self.chunk_generator.clone();

            let ticket = self.chunk_queue.try_pop().unwrap();
            let send = self.finished_chunk_send.clone();
            self.chunk_handler_count += 1;
            self.chunk_handler_pool.spawn(move || {
                let flat = generator.generate_chunk(ticket.chunk_position);
                send.send(FinishedChunk {
                    chunk_position: ticket.chunk_position,
                    esvo: flat.map(|flat| flat.into()),
                });
            });
        }
    }
}

pub enum ChunkTreeNode {
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
    pub children: [ChunkTreeNode; 8],
    pub chunk_side_length: u32,
    pub chunk_half_length: u32,

    /// Where the origin is the the -XYZ corner of the tree.
    pub chunk_origin: Vector3<i32>,
    pub is_dirty: bool,
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
            "Given chunk position {:?} is out of bounds and can't be accessed from this chunk tree.", chunk_world_position
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

    pub fn get_world_chunk_data(&self, chunk_world_position: Vector3<i32>) -> Option<&ChunkData> {
        let morton = self.world_pos_traversal_morton(chunk_world_position);
        return self.get_chunk_data(morton);
    }

    pub fn get_chunk_data(&self, morton: u64) -> Option<&ChunkData> {
        let octant = (morton & 7) as usize;
        if self.is_pre_leaf() {
            return match &self.children[octant] {
                ChunkTreeNode::Leaf(chunk_data) => Some(chunk_data),
                ChunkTreeNode::Empty | ChunkTreeNode::Enqeueud | ChunkTreeNode::Unloaded => None,
                ChunkTreeNode::Node(_) => unreachable!(),
            };
        } else {
            if let ChunkTreeNode::Node(subtree) = &self.children[octant] {
                return subtree.get_chunk_data(morton >> 3);
            } else {
                return None;
            }
        }
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

    pub fn set_world_chunk_empty(&mut self, chunk_world_position: Vector3<i32>) {
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

    pub fn set_world_chunk_data(
        &mut self,
        chunk_world_position: Vector3<i32>,
        chunk_data: ChunkData,
    ) {
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

    pub fn visit(&self, mut f: impl FnMut(ChunkTreeVisitorItem) -> ()) {
        let mut to_process = vec![(Vector3::<u32>::new(0, 0, 0), self)];
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
                    ChunkTreeNode::Leaf(chunk) => f(ChunkTreeVisitorItem::Some(chunk)),
                    ChunkTreeNode::Empty => f(ChunkTreeVisitorItem::Empty),
                    ChunkTreeNode::Enqeueud | ChunkTreeNode::Unloaded => {
                        f(ChunkTreeVisitorItem::Unloaded)
                    }
                }
            }
        }
    }
}

pub enum ChunkTreeVisitorItem<'a> {
    Unloaded,
    Empty,
    Some(&'a ChunkData),
}

pub struct ChunkData {
    pub chunk_uuid: uuid::Uuid,
    pub voxel_model_id: VoxelModelId,
}

// LOD 0 is the highest resolution.
// Each subsequent LOD halves the voxel resolution, this still renders with the same
// chunk voxel length but the chunk will have double the scaling.
impl ChunkData {
    fn world_length(lod: u32) -> f32 {
        consts::voxel::TERRAIN_CHUNK_METER_LENGTH * (lod + 1) as f32
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
    pub fn enqueue_next_chunk(
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
