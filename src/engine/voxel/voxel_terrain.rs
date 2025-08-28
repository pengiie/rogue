use core::panic;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::{Deref, Range},
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use egui::ahash::HashSetExt;
use log::debug;
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
        asset::asset::{AssetHandle, AssetPath, AssetStatus, Assets},
        entity::ecs_world::ECSWorld,
        event::Events,
        graphics::{
            backend::{Buffer, GfxBufferCreateInfo, ResourceId},
            device::DeviceResource,
        },
        resource::{Res, ResMut},
        voxel::{
            attachment::{Attachment, PTMaterial},
            flat::VoxelModelFlat,
            sft::VoxelModelSFT,
            sft_compressed::VoxelModelSFTCompressed,
            voxel::VoxelModel,
        },
        window::time::Timer,
    },
    session::Session,
    settings::Settings,
};

use super::{
    chunk_generator::ChunkGenerator,
    voxel_registry::{VoxelModelId, VoxelModelInfo, VoxelModelRegistry},
    voxel_transform::VoxelModelTransform,
    voxel_world::{VoxelWorld, VoxelWorldModelGpuInfo},
};

pub enum VoxelTerrainEvent {
    UpdateRenderDistance { chunk_render_distance: u32 },
}

#[derive(Hash, PartialEq, Eq)]
pub struct ChunkTicket {
    chunk_position: Vector3<i32>,
}

pub enum VoxelChunkRegion {
    Loading,
    Data(VoxelChunkRegionData),
}

#[derive(Clone)]
pub struct VoxelChunkRegionData {
    pub region_pos: Vector3<i32>,
    pub region_chunk_anchor: Vector3<i32>,

    pub root_node: Box<VoxelChunkRegionNode>,
}

impl VoxelChunkRegionData {
    pub fn empty(region_pos: Vector3<i32>) -> Self {
        assert!(consts::voxel::TERRAIN_REGION_TREE_HEIGHT > 0);
        let root_node = if consts::voxel::TERRAIN_REGION_TREE_HEIGHT == 1 {
            VoxelChunkRegionNode::new_preleaf()
        } else {
            VoxelChunkRegionNode::new_internal()
        };

        Self {
            region_pos,
            region_chunk_anchor: region_pos * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32,
            root_node: Box::new(root_node),
        }
    }

    pub fn get_chunk_traversal(&self, world_chunk_pos: &Vector3<i32>) -> u64 {
        let local_chunk_pos = (world_chunk_pos - self.region_chunk_anchor).map(|x| x as u32);

        return morton::morton_traversal_octree(
            morton::morton_encode(local_chunk_pos),
            consts::voxel::TERRAIN_REGION_TREE_HEIGHT,
        );
    }

    pub fn get_chunk(&self, world_chunk_pos: &Vector3<i32>) -> &VoxelRegionLeafNode {
        let mut traversal = self.get_chunk_traversal(world_chunk_pos);
        let mut curr_node = self.root_node.as_ref();
        loop {
            let curr_child_idx = traversal as usize & 0b111;
            match curr_node {
                VoxelChunkRegionNode::Internal(child_nodes) => {
                    if let Some(child_node) = &child_nodes[curr_child_idx] {
                        curr_node = child_node.as_ref();
                        traversal = traversal >> 3;
                    } else {
                        return &const { VoxelRegionLeafNode::Empty };
                    }
                }
                VoxelChunkRegionNode::Preleaf(leaf_nodes) => {
                    return &leaf_nodes[curr_child_idx];
                }
            }
        }
    }

    pub fn get_existing_chunk_mut(
        &mut self,
        world_chunk_pos: &Vector3<i32>,
    ) -> Option<&mut VoxelRegionLeafNode> {
        let mut traversal = self.get_chunk_traversal(world_chunk_pos);
        let mut curr_node = self.root_node.as_mut();
        loop {
            let curr_child_idx = traversal as usize & 0b111;
            match curr_node {
                VoxelChunkRegionNode::Internal(child_nodes) => {
                    if let Some(child_node) = &mut child_nodes[curr_child_idx] {
                        curr_node = child_node.as_mut();
                        traversal = traversal >> 3;
                    } else {
                        return None;
                    }
                }
                VoxelChunkRegionNode::Preleaf(leaf_nodes) => {
                    let node = &mut leaf_nodes[curr_child_idx];
                    match node {
                        VoxelRegionLeafNode::Empty => return None,
                        VoxelRegionLeafNode::Existing { uuid, model } => {}
                    }
                    return Some(node);
                }
            }
        }
    }

    pub fn get_or_create_chunk_mut(
        &mut self,
        world_chunk_pos: &Vector3<i32>,
    ) -> &mut VoxelRegionLeafNode {
        let mut traversal = self.get_chunk_traversal(world_chunk_pos);
        let mut curr_node = self.root_node.as_mut();
        let mut curr_height = 0;
        loop {
            let curr_child_idx = traversal as usize & 0b111;
            match curr_node {
                VoxelChunkRegionNode::Internal(child_nodes) => {
                    if child_nodes[curr_child_idx].is_none() {
                        let new_node =
                            if curr_height + 2 < consts::voxel::TERRAIN_REGION_TREE_HEIGHT {
                                Box::new(VoxelChunkRegionNode::new_internal())
                            } else {
                                Box::new(VoxelChunkRegionNode::new_preleaf())
                            };
                        child_nodes[curr_child_idx] = Some(new_node);
                    }
                    curr_node = child_nodes[curr_child_idx].as_mut().unwrap().as_mut();
                    traversal = traversal >> 3;
                }
                VoxelChunkRegionNode::Preleaf(leaf_nodes) => {
                    return &mut leaf_nodes[curr_child_idx]
                }
            }
            curr_height += 1;
        }
    }

    pub fn set_chunk(&self, world_chunk_pos: &Vector3<i32>, data: VoxelRegionLeafNode) {
        todo!("impl set_chunk")
    }

    pub fn set_chunk_local(&self, local_chunk_pos: &Vector3<u32>, data: VoxelRegionLeafNode) {
        todo!("impl set_chunk_local")
    }
}

impl VoxelChunkRegion {
    pub fn empty(region_pos: Vector3<i32>) -> Self {
        Self::Data(VoxelChunkRegionData::empty(region_pos))
    }

    pub fn is_loading(&self) -> bool {
        match self {
            VoxelChunkRegion::Loading => true,
            VoxelChunkRegion::Data(_) => false,
        }
    }

    pub fn data(&self) -> &VoxelChunkRegionData {
        match &self {
            VoxelChunkRegion::Loading => {
                panic!("Tried to get data VoxelChunkRegion when it is loading.")
            }
            VoxelChunkRegion::Data(data) => data,
        }
    }

    pub fn data_mut(&mut self) -> &mut VoxelChunkRegionData {
        match self {
            VoxelChunkRegion::Loading => {
                panic!("Tried to get data VoxelChunkRegion when it is loading.")
            }
            VoxelChunkRegion::Data(data) => data,
        }
    }
}

#[derive(Clone)]
pub enum VoxelChunkRegionNode {
    Internal([Option<Box<VoxelChunkRegionNode>>; 8]),
    Preleaf([VoxelRegionLeafNode; 8]),
}

impl VoxelChunkRegionNode {
    pub fn new_internal() -> Self {
        Self::Internal([const { None }; 8])
    }

    pub fn new_preleaf() -> Self {
        Self::Preleaf([const { VoxelRegionLeafNode::Empty }; 8])
    }
}

#[derive(Debug, Clone)]
pub enum VoxelRegionLeafNode {
    Empty,
    Existing {
        uuid: uuid::Uuid,
        model: Option<VoxelModelId>,
    },
}

impl VoxelRegionLeafNode {
    pub fn new_with_model(model_id: VoxelModelId) -> Self {
        Self::Existing {
            uuid: uuid::Uuid::new_v4(),
            model: Some(model_id),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            VoxelRegionLeafNode::Empty => true,
            VoxelRegionLeafNode::Existing { .. } => false,
        }
    }
}

pub struct VoxelChunks {
    pub chunk_render_distance: u32,
    pub player_chunk_position: Option<Vector3<i32>>,
    pub chunk_load_iter: ChunkLoadIter,
    // Time between adding chunks to the chunk io queue.
    pub queue_timer: Timer,
    pub renderable_chunks: RenderableChunks,

    pub regions: HashMap<Vector3<i32>, VoxelChunkRegion>,

    // Edited means changed since the last save.
    pub edited_regions: HashSet<Vector3<i32>>,
    pub edited_chunks: HashSet<Vector3<i32>>,
    pub waiting_save_handles: HashSet<AssetHandle>,

    // pool that waits on io.
    pub waiting_io_regions: HashMap<Vector3<i32>, AssetHandle>,
    pub waiting_io_region_chunks: HashMap<Vector3<i32>, HashSet<Vector3<i32>>>,
    pub waiting_io_chunks: HashMap<Vector3<i32>, AssetHandle>,
}

impl VoxelChunks {
    pub fn new(settings: &Settings) -> Self {
        Self {
            chunk_render_distance: settings.chunk_render_distance,
            player_chunk_position: None,
            chunk_load_iter: ChunkLoadIter::new(Vector3::zeros(), settings.chunk_render_distance),
            renderable_chunks: RenderableChunks::new(settings.chunk_render_distance),

            regions: HashMap::new(),

            edited_regions: HashSet::new(),
            edited_chunks: HashSet::new(),
            waiting_save_handles: HashSet::new(),

            queue_timer: Timer::new(Duration::from_millis(5)),
            waiting_io_regions: HashMap::new(),
            waiting_io_region_chunks: HashMap::new(),
            waiting_io_chunks: HashMap::new(),
        }
    }

    pub fn is_saving(&self) -> bool {
        !self.waiting_save_handles.is_empty()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        !self.edited_chunks.is_empty() || !self.edited_regions.is_empty()
    }

    pub fn clear(&mut self) {
        self.edited_regions.clear();
        self.edited_chunks.clear();
        self.renderable_chunks.clear();
        self.regions.clear();
        self.chunk_load_iter.reset();
    }

    pub fn enqueue_save_all(&mut self) {
        for (region_pos, region) in &self.regions {
            match &region {
                VoxelChunkRegion::Loading => {}
                VoxelChunkRegion::Data(voxel_chunk_region_data) => {
                    self.edited_regions.insert(*region_pos);

                    let mut to_process =
                        vec![(/*traversal*/ 0u64, &voxel_chunk_region_data.root_node)];
                    while let Some((traversal, next)) = to_process.pop() {
                        match next.deref() {
                            VoxelChunkRegionNode::Internal(children) => {
                                for (i, child) in children.iter().enumerate() {
                                    let Some(child) = child else { continue };
                                    to_process.push(((traversal << 3) | i as u64, child));
                                }
                            }
                            VoxelChunkRegionNode::Preleaf(leaves) => {
                                for (i, leaf) in leaves.iter().enumerate() {
                                    match leaf {
                                        VoxelRegionLeafNode::Empty => {}
                                        VoxelRegionLeafNode::Existing { uuid, model } => {
                                            let morton = (traversal << 3) | i as u64;
                                            let local_chunk_pos =
                                                morton::morton_decode(morton).cast::<i32>();
                                            let world_chunk_pos = *region_pos
                                                * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32
                                                + local_chunk_pos;
                                            self.edited_chunks.insert(world_chunk_pos);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn save_terrain(
        &mut self,
        assets: &mut Assets,
        registry: &VoxelModelRegistry,
        session: &Session,
    ) {
        assert!(!self.is_saving());

        let Some(project_dir) = &session.project_save_dir else {
            return;
        };
        let Some(terrain_dir) = &session.terrain_dir else {
            return;
        };
        for region_pos in self.edited_regions.drain() {
            let region = self
                .regions
                .get(&region_pos)
                .expect("Region should exist if in edited_regions");
            let VoxelChunkRegion::Data(region) = region else {
                panic!("Region should be loaded if saving.");
            };
            let save_handle = assets.save_asset(
                Self::region_asset_path(project_dir.clone(), terrain_dir.clone(), region_pos),
                region.clone(),
            );
            self.waiting_save_handles.insert(save_handle);
        }

        for chunk_pos in self.edited_chunks.drain().collect::<Vec<_>>() {
            let chunk = self
                .get_chunk_node(chunk_pos)
                .expect("Chunk should exist if in edited_chunks");
            match chunk {
                VoxelRegionLeafNode::Empty => todo!("We shouldn't be hitting this todo, yet."),
                VoxelRegionLeafNode::Existing { uuid, model } => {
                    let model_id =
                        model.expect("Model should exist on the chunk if we are saving it");
                    let chunk_model = registry.get_model::<VoxelModelSFT>(model_id);
                    let save_handle = assets.save_asset(
                        Self::chunk_asset_path(project_dir.clone(), terrain_dir.clone(), uuid),
                        VoxelModelSFTCompressed::from(chunk_model),
                    );
                    self.waiting_save_handles.insert(save_handle);
                }
            }
        }
    }

    pub fn try_update_chunk_normal(
        renderable_chunks: &mut RenderableChunks,
        chunk_pos: &Vector3<i32>,
    ) {
        let min = chunk_pos.add_scalar(-1);
        let max = chunk_pos.add_scalar(1);
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    let chunk_pos = Vector3::new(x, y, z);
                    if renderable_chunks.in_bounds(&chunk_pos)
                        && renderable_chunks.chunk_exists(chunk_pos)
                    {
                        renderable_chunks.to_update_chunk_normals.insert(chunk_pos);
                    }
                }
            }
        }
    }

    pub fn update_player_position(&mut self, player_pos: Vector3<f32>) {
        let chunk_pos =
            player_pos.map(|x| (x / consts::voxel::TERRAIN_CHUNK_METER_LENGTH).floor() as i32);

        self.chunk_load_iter.update_anchor(chunk_pos);
        self.renderable_chunks.update_player_position(chunk_pos);
        self.player_chunk_position = Some(chunk_pos);
    }

    pub fn get_chunk_node(&self, world_chunk_pos: Vector3<i32>) -> Option<&VoxelRegionLeafNode> {
        let chunk_region = Self::chunk_to_region_pos(&world_chunk_pos);
        let Some(region) = self.regions.get(&chunk_region) else {
            return None;
        };

        let region = match region {
            VoxelChunkRegion::Loading => return None,
            VoxelChunkRegion::Data(region) => region,
        };

        Some(region.get_chunk(&world_chunk_pos))
    }

    pub fn get_or_create_chunk_node_mut(
        &mut self,
        world_chunk_pos: Vector3<i32>,
    ) -> Option<&mut VoxelRegionLeafNode> {
        let chunk_region = Self::chunk_to_region_pos(&world_chunk_pos);
        let Some(region) = self.regions.get_mut(&chunk_region) else {
            return None;
        };

        let VoxelChunkRegion::Data(region) = region else {
            return None;
        };

        return Some(region.get_or_create_chunk_mut(&world_chunk_pos));
    }

    pub fn ensure_chunk_loaded(
        &mut self,
        chunk_pos: Vector3<i32>,
        assets: &mut Assets,
        session: &Session,
    ) {
        let Some(chunk_node) = self.get_chunk_node(chunk_pos) else {
            let chunk_region = Self::chunk_to_region_pos(&chunk_pos);
            if let Some(terrain_dir) = &session.terrain_dir {
                if !self.waiting_io_regions.contains_key(&chunk_region) {
                    let region_asset_handle =
                        assets.load_asset::<VoxelChunkRegionData>(VoxelChunks::region_asset_path(
                            session.project_save_dir.clone().unwrap(),
                            terrain_dir.clone(),
                            chunk_region,
                        ));
                    self.regions.insert(chunk_region, VoxelChunkRegion::Loading);
                    self.waiting_io_regions
                        .insert(chunk_region, region_asset_handle);
                }
                self.waiting_io_region_chunks
                    .entry(chunk_region)
                    .or_default()
                    .insert(chunk_pos);
            } else {
                self.regions
                    .insert(chunk_region, VoxelChunkRegion::empty(chunk_region));
            }
            return;
        };

        match chunk_node {
            VoxelRegionLeafNode::Empty => {}
            VoxelRegionLeafNode::Existing { uuid, model } => {
                let Some(model_id) = model else {
                    if self.waiting_io_chunks.contains_key(&chunk_pos) {
                        return;
                    }

                    // Load the chunk model.
                    let chunk_asset_handle =
                        assets.load_asset::<VoxelModelSFTCompressed>(Self::chunk_asset_path(
                            session.project_save_dir.clone().unwrap(),
                            session.terrain_dir.clone().unwrap(),
                            uuid,
                        ));
                    self.waiting_io_chunks.insert(chunk_pos, chunk_asset_handle);
                    return;
                };
                if self.renderable_chunks.try_load_chunk(&chunk_pos, *model_id) {
                    Self::try_update_chunk_normal(&mut self.renderable_chunks, &chunk_pos);
                }
            }
        }
    }

    pub fn region_asset_path(
        project_dir: PathBuf,
        terrain_dir: PathBuf,
        region_pos: Vector3<i32>,
    ) -> AssetPath {
        let terrain_dir_path = AssetPath::from_project_dir_path(&project_dir, &terrain_dir);
        AssetPath::new_project_dir(
            project_dir,
            format!(
                "{}::region_{}_{}_{}::rog",
                terrain_dir_path.asset_path.unwrap(),
                region_pos.x,
                region_pos.y,
                region_pos.z
            ),
        )
    }

    pub fn chunk_asset_path(
        project_dir: PathBuf,
        terrain_dir: PathBuf,
        uuid: &uuid::Uuid,
    ) -> AssetPath {
        let terrain_dir_path = AssetPath::from_project_dir_path(&project_dir, &terrain_dir);
        AssetPath::new_project_dir(
            project_dir,
            format!(
                "{}::chunk_{}::rvox",
                terrain_dir_path.asset_path.unwrap(),
                uuid.to_string()
            ),
        )
    }

    pub fn process_waiting_io_regions(&mut self, assets: &mut Assets, session: &Session) {
        let mut to_remove_waiting_regions = Vec::new();
        for (region_pos, asset_handle) in self.waiting_io_regions.iter() {
            let status = assets.get_asset_status(asset_handle);
            let mut region = None;
            match status {
                AssetStatus::InProgress => {
                    continue;
                }
                AssetStatus::Loaded => {
                    region = Some(assets.take_asset::<VoxelChunkRegionData>(asset_handle).expect("If we got AssetStatus::Loaded but the asset isn't loaded, something went wrong."));
                }
                AssetStatus::NotFound => {
                    region = Some(Box::new(VoxelChunkRegionData::empty(*region_pos)));
                }
                AssetStatus::Error(err) => log::error!(
                    "Got an error while loading region_pos x: {}, y: {}, z: {}, {}",
                    region_pos.x,
                    region_pos.y,
                    region_pos.z,
                    err
                ),
                _ => unreachable!(),
            }

            to_remove_waiting_regions.push(*region_pos);
            if let Some(region) = region {
                for chunk_pos in self
                    .waiting_io_region_chunks
                    .entry(*region_pos)
                    .or_default()
                    .drain()
                {
                    let chunk_node = region.get_chunk(&chunk_pos);
                    if let VoxelRegionLeafNode::Existing { uuid, model } = chunk_node {
                        assert!(model.is_none(), "We shouldn't be loading this chunk if it already has an existing model.");
                        let chunk_asset_handle = assets.load_asset::<VoxelModelSFTCompressed>(
                            Self::chunk_asset_path(session.project_save_dir.clone().unwrap(), session.terrain_dir.clone().expect("If the region was loaded then a directory for the terrain must exist."), uuid),
                        );
                        self.waiting_io_chunks.insert(chunk_pos, chunk_asset_handle);
                    }
                }
                self.regions
                    .insert(*region_pos, VoxelChunkRegion::Data(*region));
            } else {
                log::error!("Failed to load chunks: ");
                for chunk_pos in self
                    .waiting_io_region_chunks
                    .entry(*region_pos)
                    .or_default()
                    .drain()
                {
                    log::error!(
                        "    X: {}, Y: {}, Z: {}",
                        chunk_pos.x,
                        chunk_pos.y,
                        chunk_pos.z
                    )
                }
            }
        }

        for region_pos in to_remove_waiting_regions {
            self.waiting_io_regions.remove(&region_pos);
        }
    }

    pub fn process_waiting_io_chunks(
        &mut self,
        assets: &mut Assets,
        registry: &mut VoxelModelRegistry,
    ) {
        let mut to_remove_waiting_chunks = Vec::new();
        for (i, (chunk_position, chunk_asset_handle)) in self.waiting_io_chunks.iter().enumerate() {
            let status = assets.get_asset_status(chunk_asset_handle);
            let mut chunk_model = None;
            match status {
                AssetStatus::InProgress => {
                    continue;
                }
                AssetStatus::Loaded => {
                    let loaded_model = assets
                        .take_asset::<VoxelModelSFTCompressed>(chunk_asset_handle)
                        .expect("If status says loaded then this should be loaded.");

                    chunk_model = Some(loaded_model);
                }
                AssetStatus::NotFound => {
                    log::error!(
                        "Tried to load chunk at X: {} Y: {}, Z: {}, path: {}, but it is not found.",
                        chunk_position.x,
                        chunk_position.y,
                        chunk_position.z,
                        chunk_asset_handle.asset_path().path_str()
                    );
                }
                AssetStatus::Error(err) => {
                    log::error!(
                        "Tried to load chunk at X: {} Y: {}, Z: {}, path: {}, but got an unexpected error: {}.",
                        chunk_position.x,
                        chunk_position.y,
                        chunk_position.z,
                        chunk_asset_handle.asset_path().path_str(),
                        err
                    );
                }
                _ => unreachable!(),
            }

            if let Some(chunk_model) = chunk_model {
                let model_id = registry.register_renderable_voxel_model(
                    format!(
                        "chunk_{}_{}_{}",
                        chunk_position.x, chunk_position.y, chunk_position.z
                    ),
                    VoxelModel::new(VoxelModelSFT::from(chunk_model.deref())),
                );
                let chunk_region = Self::chunk_to_region_pos(&chunk_position);
                let chunk_node = self
                    .regions
                    .get_mut(&chunk_region)
                    .expect("Region should be loaded by now.")
                    .data_mut()
                    .get_existing_chunk_mut(chunk_position)
                    .expect("Should should exist and not be empty.");
                let VoxelRegionLeafNode::Existing { model, .. } = chunk_node else {
                    unreachable!();
                };
                *model = Some(model_id);
                self.renderable_chunks
                    .try_load_chunk(chunk_position, model_id);
                Self::try_update_chunk_normal(&mut self.renderable_chunks, chunk_position);
            } else {
                let chunk_region = Self::chunk_to_region_pos(&chunk_position);
                let chunk_node = self
                    .regions
                    .get_mut(&chunk_region)
                    .expect("Region should be loaded by now.")
                    .data_mut()
                    .get_existing_chunk_mut(chunk_position)
                    .expect("Should should exist and not be empty.");
                *chunk_node = VoxelRegionLeafNode::Empty;
            }
            to_remove_waiting_chunks.push(*chunk_position);
        }

        // Since we remove in reverse order the indices stay accurate.
        for chunk_pos in to_remove_waiting_chunks.into_iter() {
            self.waiting_io_chunks.remove_entry(&chunk_pos);
        }
    }

    pub fn mark_chunk_edited(&mut self, chunk_pos: Vector3<i32>) {
        self.edited_chunks.insert(chunk_pos);
        // Mark dirty since the edited chunk may be a SFT which would have a new model ptr.
        self.renderable_chunks.is_dirty = true;
        Self::try_update_chunk_normal(&mut self.renderable_chunks, &chunk_pos);
    }

    pub fn mark_region_edited(&mut self, region_pos: Vector3<i32>) {
        self.edited_regions.insert(region_pos);
    }

    pub fn chunk_to_region_pos(chunk_pos: &Vector3<i32>) -> Vector3<i32> {
        chunk_pos.map(|x| x.div_euclid(consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32))
    }

    pub fn position_to_region_pos(pos: &Vector3<f32>) -> Vector3<i32> {
        pos.map(|x| (x / consts::voxel::TERRAIN_REGION_METER_LENGTH).floor() as i32)
    }

    pub fn update_chunk_queue(
        &mut self,
        assets: &mut Assets,
        registry: &mut VoxelModelRegistry,
        session: &Session,
    ) {
        let mut to_remove_handles = Vec::new();
        for handle in self.waiting_save_handles.iter() {
            if assets.get_asset_status(handle).is_saved() {
                to_remove_handles.push(handle.clone());
            }
        }
        for handle in to_remove_handles {
            self.waiting_save_handles.remove(&handle);
        }

        if self.player_chunk_position.is_none() || self.is_saving() {
            return;
        }

        // Try enqueue any not visited chunks if the current queue isn't full.
        if self.queue_timer.try_complete() {
            if let Some(next_chunk) = self.chunk_load_iter.next_chunk() {
                self.ensure_chunk_loaded(next_chunk, assets, session);
            }
        }

        self.process_waiting_io_regions(assets, session);
        self.process_waiting_io_chunks(assets, registry);
    }

    pub fn renderable_chunks_aabb(&self) -> AABB {
        let origin = self.renderable_chunks.chunk_anchor.cast::<f32>()
            * consts::voxel::TERRAIN_CHUNK_METER_LENGTH;
        let side_length =
            self.renderable_chunks.side_length as f32 * consts::voxel::TERRAIN_CHUNK_METER_LENGTH;
        return AABB::new_two_point(
            origin,
            origin + Vector3::new(side_length, side_length, side_length),
        );
    }

    pub fn renderable_chunks_dda(&self, ray: &Ray) -> RayDDA {
        let world_aabb = self.renderable_chunks_aabb();
        let side_length = self.renderable_chunks.side_length;
        return ray.begin_dda(
            &world_aabb,
            Vector3::new(side_length, side_length, side_length),
        );
    }
}

pub struct ChunkLoadIter {
    max_radius: u32,
    curr_radius: u32,
    curr_index: u32,
    /// Anchor is in the center with the iterator iterating around.
    current_chunk_anchor: Vector3<i32>,
}

impl ChunkLoadIter {
    pub fn new(chunk_anchor: Vector3<i32>, render_distance: u32) -> Self {
        Self {
            max_radius: render_distance,
            curr_radius: 0,
            curr_index: 0,
            current_chunk_anchor: chunk_anchor,
        }
    }

    pub fn reset(&mut self) {
        self.curr_radius = 0;
        self.curr_index = 0;
    }

    pub fn update_max_radius(&mut self, new_max_radius: u32) {
        self.max_radius = new_max_radius;
        if self.max_radius < self.curr_radius {
            self.curr_radius = self.max_radius;
        }
    }

    pub fn update_anchor(&mut self, new_chunk_anchor: Vector3<i32>) {
        if new_chunk_anchor == self.current_chunk_anchor {
            return;
        }

        let distance = ((new_chunk_anchor - self.current_chunk_anchor).abs().max()) as u32;
        self.curr_radius = self.curr_radius.saturating_sub(distance);
        self.curr_index = 0;
        self.current_chunk_anchor = new_chunk_anchor;
    }

    /// Enqueues chunks in an iterator fashion so we don't waste time rechecking chunks.
    pub fn next_chunk(&mut self) -> Option<Vector3<i32>> {
        if self.curr_radius == self.max_radius {
            return None;
        }

        let curr_diameter = (self.curr_radius + 1) * 2;
        let curr_area = curr_diameter.pow(2);
        if self.curr_index >= curr_area * 6 {
            self.curr_radius += 1;
            self.curr_index = 0;
            return None;
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

        self.curr_index += 1;
        return Some(chunk_position);
    }
}

pub struct RenderableChunks {
    pub side_length: u32,
    pub chunk_model_pointers: Vec<VoxelModelId>,

    pub window_offset: Vector3<u32>,
    pub chunk_anchor: Vector3<i32>,
    pub is_dirty: bool,

    pub to_update_chunk_normals: HashSet<Vector3<i32>>,
}

impl RenderableChunks {
    pub fn new(render_distance: u32) -> Self {
        let side_length = render_distance * 2;
        Self {
            side_length,
            chunk_model_pointers: vec![VoxelModelId::null(); side_length.pow(3) as usize],
            window_offset: Vector3::new(0, 0, 0),
            chunk_anchor: Vector3::new(0, 0, 0),
            is_dirty: false,
            to_update_chunk_normals: HashSet::new(),
        }
    }

    pub fn in_bounds(&self, world_chunk_pos: &Vector3<i32>) -> bool {
        let local_chunk_pos = world_chunk_pos - self.chunk_anchor;
        !(local_chunk_pos.x < 0
            || local_chunk_pos.y < 0
            || local_chunk_pos.z < 0
            || local_chunk_pos.x >= self.side_length as i32
            || local_chunk_pos.y >= self.side_length as i32
            || local_chunk_pos.z >= self.side_length as i32)
    }

    pub fn clear(&mut self) {
        self.to_update_chunk_normals.clear();
        self.chunk_model_pointers.fill(VoxelModelId::null());
        self.is_dirty = true;
    }

    pub fn try_load_chunk(
        &mut self,
        world_chunk_pos: &Vector3<i32>,
        model_id: VoxelModelId,
    ) -> bool {
        if !self.in_bounds(world_chunk_pos) {
            return false;
        }

        let local_chunk_pos = (world_chunk_pos - self.chunk_anchor).map(|x| x as u32);
        let window_chunk_pos =
            local_chunk_pos.zip_map(&self.window_offset, |x, y| (x + y) % self.side_length);
        let index = self.get_chunk_index(window_chunk_pos);

        if self.chunk_model_pointers[index as usize] != model_id {
            self.is_dirty = true;
            self.chunk_model_pointers[index as usize] = model_id;
            return true;
        }
        return false;
    }

    pub fn update_player_position(&mut self, player_chunk_position: Vector3<i32>) {
        let new_anchor = player_chunk_position.map(|x| x - (self.side_length as i32 / 2));
        if self.chunk_anchor == new_anchor {
            return;
        }
        let new_window_offset = new_anchor.map(|x| x.rem_euclid(self.side_length as i32) as u32);

        // TODO: Don't unload chunks if we are first initializing the player position.
        let translation = new_anchor - self.chunk_anchor;
        let ranges = translation.zip_zip_map(
            &self.window_offset.cast::<i32>(),
            &new_window_offset.cast::<i32>(),
            |translation, old_window_offset, new_window_offset| {
                if translation.is_positive() {
                    (new_window_offset - translation)..new_window_offset
                } else {
                    (old_window_offset + translation)..old_window_offset
                }
            },
        );

        for x in ranges.x.clone() {
            let x = x.rem_euclid(self.side_length as i32) as u32;
            for y in 0..self.side_length {
                for z in 0..self.side_length {
                    self.unload_chunk(Vector3::new(x, y, z));
                }
            }
        }
        for y in ranges.y.clone() {
            let y = y.rem_euclid(self.side_length as i32) as u32;
            for x in 0..self.side_length {
                for z in 0..self.side_length {
                    self.unload_chunk(Vector3::new(x, y, z));
                }
            }
        }
        for z in ranges.z.clone() {
            let z = z.rem_euclid(self.side_length as i32) as u32;
            for x in 0..self.side_length {
                for y in 0..self.side_length {
                    self.unload_chunk(Vector3::new(x, y, z));
                }
            }
        }

        if !ranges.x.is_empty() || !ranges.y.is_empty() || !ranges.z.is_empty() {
            self.is_dirty = true;
        }

        self.chunk_anchor = new_anchor;
        self.window_offset = new_window_offset;
    }

    pub fn update_render_distance(&mut self, new_render_distance: u32) {
        todo!()
    }

    fn unload_chunk(&mut self, local_chunk_pos: Vector3<u32>) {
        let index = self.get_chunk_index(local_chunk_pos) as usize;
        self.chunk_model_pointers[index] = VoxelModelId::null();
    }

    pub fn chunk_exists(&self, world_chunk_pos: Vector3<i32>) -> bool {
        let local_pos = world_chunk_pos - self.chunk_anchor;
        return self.get_chunk_model(local_pos.map(|x| x as u32)).is_some();
    }

    /// local_chunk_pos is local to self.chunk_anchor, with sliding window offset not taken into
    /// account.
    pub fn get_chunk_model(&self, local_chunk_pos: Vector3<u32>) -> Option<VoxelModelId> {
        let window_adjusted_pos = local_chunk_pos.zip_map(&self.window_offset, |x, y| {
            (x as u32 + y) % self.side_length
        });
        let index = self.get_chunk_index(window_adjusted_pos);
        let chunk_model_id = &self.chunk_model_pointers[index as usize];
        (!chunk_model_id.is_null()).then_some(*chunk_model_id)
    }

    pub fn get_chunk_index(&self, local_chunk_pos: Vector3<u32>) -> u32 {
        local_chunk_pos.x
            + local_chunk_pos.y * self.side_length
            + local_chunk_pos.z * self.side_length.pow(2)
    }
}

pub struct RenderableChunksGpu {
    pub terrain_acceleration_buffer: Option<ResourceId<Buffer>>,
    pub terrain_side_length: u32,
    pub terrain_anchor: Vector3<i32>,
    pub terrain_window_offset: Vector3<u32>,
}

impl RenderableChunksGpu {
    pub fn new() -> Self {
        Self {
            terrain_acceleration_buffer: None,
            terrain_side_length: 0,
            terrain_anchor: Vector3::new(0, 0, 0),
            terrain_window_offset: Vector3::new(0, 0, 0),
        }
    }

    pub fn update_gpu_objects(
        &mut self,
        device: &mut DeviceResource,
        renderable_chunks: &RenderableChunks,
    ) {
        let req_size = 4 * (renderable_chunks.side_length as u64).pow(3);
        if let Some(buffer) = self.terrain_acceleration_buffer {
            let buffer_info = device.get_buffer_info(&buffer);
            if buffer_info.size < req_size {
                todo!("Resize buffer due to render distance change.");
            }
        } else {
            self.terrain_acceleration_buffer = Some(device.create_buffer(GfxBufferCreateInfo {
                name: "world_terrain_acceleration_buffer".to_owned(),
                size: req_size,
            }));
        }
    }

    pub fn write_render_data(
        &mut self,
        device: &mut DeviceResource,
        renderable_chunks: &RenderableChunks,
        voxel_model_info_map: &HashMap<VoxelModelId, VoxelWorldModelGpuInfo>,
    ) {
        self.terrain_side_length = renderable_chunks.side_length;
        self.terrain_anchor = renderable_chunks.chunk_anchor;
        self.terrain_window_offset = renderable_chunks.window_offset;

        if renderable_chunks.is_dirty {
            // TODO: Copy incrementally with updates.
            let volume = renderable_chunks.side_length.pow(3) as usize;
            let mut buf = vec![0xFFFF_FFFFu32; volume];
            for i in 0..volume {
                let id = &renderable_chunks.chunk_model_pointers[i];
                if id.is_null() {
                    continue;
                }

                let Some(model_info) = voxel_model_info_map.get(id) else {
                    continue;
                };
                buf[i] = model_info.info_allocation.start_index_stride_dword() as u32;
            }

            device.write_buffer_slice(
                self.terrain_acceleration_buffer.as_ref().unwrap(),
                0,
                bytemuck::cast_slice::<u32, u8>(buf.as_slice()),
            );
        }
    }
}
