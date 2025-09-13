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
        color::Color,
        morton,
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
            terrain::{chunk_iter::ChunkIter, RenderableChunks},
            voxel::VoxelModel,
            voxel_registry::{VoxelModelId, VoxelModelRegistry},
        },
        window::time::Timer,
    },
    session::Session,
    settings::Settings,
};
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::ray::{Ray, RayDDA};

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

    pub fn new_air() -> Self {
        Self::Existing {
            uuid: uuid::Uuid::new_v4(),
            model: None,
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
    pub chunk_load_iter: ChunkIter,
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
            chunk_load_iter: ChunkIter::new(Vector3::zeros(), settings.chunk_render_distance),
            renderable_chunks: RenderableChunks::new(settings.chunk_render_distance),

            regions: HashMap::new(),

            edited_regions: HashSet::new(),
            edited_chunks: HashSet::new(),
            waiting_save_handles: HashSet::new(),

            queue_timer: Timer::new(Duration::from_millis(1)),
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

    pub fn try_update_chunk_render_distance(&mut self, settings: &Settings) {
        if settings.chunk_render_distance != self.chunk_render_distance {
            self.chunk_render_distance = settings.chunk_render_distance;
            self.renderable_chunks.resize(self.chunk_render_distance);
            self.chunk_load_iter
                .update_max_radius(self.chunk_render_distance);
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
            for _ in 0..32 {
                if let Some(next_chunk) = self.chunk_load_iter.next_chunk() {
                    self.ensure_chunk_loaded(next_chunk, assets, session);
                }
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
