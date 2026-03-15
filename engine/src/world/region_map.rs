use std::{
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    ops::{Add, Deref, Mul, Sub},
    path::PathBuf,
};

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::voxel::voxel::VoxelEditData;
use crate::world::{region::RegionTree, region_asset::WorldRegionAsset};
use crate::{asset::asset::GameAssetPath, resource::ResMut};
use crate::{
    asset::asset::{AssetHandle, AssetPath, AssetStatus, Assets},
    world::region::WorldRegion,
};
use crate::{common::geometry::ray::Ray, consts};
use crate::{
    common::morton,
    event::Events,
    voxel::{sft_compressed::VoxelModelSFTCompressed, voxel_registry::VoxelModelId},
    world::region::WorldRegionNode,
};

#[derive(Clone, Debug)]
pub struct RegionEvent {
    pub region_pos: RegionPos,
    pub event_type: RegionEventType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionEventType {
    Loaded,
    Unloaded,
    /// Some chunk within the region was updated, likely an editor or LOD change of the region.
    Updated,
}

pub struct ChunkEvent {
    pub chunk_id: ChunkId,
    pub event_type: ChunkEventType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChunkEventType {
    Loaded,
    Unloaded,
    Updated,
}

pub struct VoxelTerrainEdit {
    /// In world space voxel coordinates.
    pub min: Vector3<i32>,
    pub max: Vector3<i32>,
    pub data: VoxelEditData,
}

pub struct VoxelRegionEdit {
    /// In region-space voxel coordinates with the origin being
    /// the regions minimum point.
    pub min: Vector3<i32>,
    pub max: Vector3<i32>,
    pub data: VoxelEditData,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChunkId {
    pub chunk_pos: ChunkPos,
    pub chunk_lod: ChunkLOD,
}

impl ChunkId {
    pub fn neighbors(&self) -> Vec<ChunkId> {
        let mut neighbors = Vec::new();
        let chunk_length = self.chunk_lod.leaf_chunk_length() as i32;
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if x == 0 && y == 0 && z == 0 {
                        continue;
                    }
                    neighbors.push(ChunkId {
                        chunk_pos: self.chunk_pos
                            + ChunkPos::new(Vector3::new(x, y, z) * chunk_length),
                        chunk_lod: self.chunk_lod,
                    });
                }
            }
        }
        return neighbors;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChunkLOD(pub u32);

impl ChunkLOD {
    /// Max LOD in this case is the lowest level of detail, naming is a bit unintuitive but is
    /// because full resolution starts at 0.
    pub const MAX_LOD: u32 = consts::voxel::TERRAIN_REGION_TREE_HEIGHT;
    /// AKA MIN lod;
    pub const FULL_RES_LOD: ChunkLOD = ChunkLOD::new_full_res();

    pub const fn new_full_res() -> Self {
        Self::new(0)
    }

    pub fn is_full_res(&self) -> bool {
        self.0 == 0
    }

    pub fn is_lowest_res(&self) -> bool {
        self.0 == Self::MAX_LOD
    }

    pub fn from_tree_height(tree_height: u32) -> Self {
        assert!(
            tree_height <= consts::voxel::TERRAIN_REGION_TREE_HEIGHT,
            "Cannot request an LOD which is higher (lower resolution) than the maximum region tree height, max is {} and requested {}",
            Self::MAX_LOD,
            tree_height
        );
        Self(Self::MAX_LOD - tree_height)
    }

    pub fn region_chunk_length(&self) -> u32 {
        consts::voxel::TERRAIN_REGION_CHUNK_LENGTH >> (self.0 * 2)
    }

    pub fn leaf_chunk_length(&self) -> u32 {
        1 << (self.0 * 2)
    }

    pub fn chunk_to_region_proportion(&self) -> f32 {
        1.0 / (self.region_chunk_length() as f32)
    }

    pub fn as_tree_height(&self) -> u32 {
        consts::voxel::TERRAIN_REGION_TREE_HEIGHT - self.0
    }

    /// LOD 0 is the highest detail level with each LOD fourthing
    /// the voxel resolution since we use 64-trees.
    pub const fn new(lod: u32) -> Self {
        assert!(lod <= Self::MAX_LOD);
        Self(lod)
    }

    pub fn new_lowest_res() -> Self {
        Self::new(Self::MAX_LOD)
    }

    pub fn max_tree_height(&self) -> u32 {
        (consts::voxel::TERRAIN_REGION_CHUNK_LENGTH.trailing_zeros() >> 1) - self.0
    }

    pub fn voxel_meter_size(&self) -> f32 {
        consts::voxel::VOXEL_METER_LENGTH * (4u32.pow(self.0) as f32)
    }
}

pub struct LoadingRegion {
    pub asset_handle: Option<AssetHandle>,
}

impl LoadingRegion {
    pub fn new() -> Self {
        Self { asset_handle: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RegionPos(Vector3<i32>);

impl RegionPos {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self(Vector3::new(x, y, z))
    }

    pub fn new_vec(vec: Vector3<i32>) -> Self {
        Self(vec)
    }

    pub fn zeros() -> Self {
        Self(Vector3::zeros())
    }

    pub fn from_world_pos(world_pos: &Vector3<f32>) -> Self {
        Self::new_vec(
            (world_pos * (1.0 / consts::voxel::TERRAIN_REGION_METER_LENGTH))
                .map(|x| x.floor() as i32),
        )
    }

    pub fn into_chunk_pos(&self) -> ChunkPos {
        ChunkPos::new(self.map(|x| x * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32))
    }
}

impl Deref for RegionPos {
    type Target = Vector3<i32>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<RegionPos> for Vector3<i32> {
    fn from(region_pos: RegionPos) -> Self {
        region_pos.0
    }
}

impl From<Vector3<i32>> for RegionPos {
    fn from(vec: Vector3<i32>) -> Self {
        RegionPos(vec)
    }
}

impl Add<Vector3<i32>> for RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: Vector3<i32>) -> Self::Output {
        RegionPos(self.0 + rhs)
    }
}

impl Add<RegionPos> for Vector3<i32> {
    type Output = RegionPos;

    fn add(self, rhs: RegionPos) -> Self::Output {
        RegionPos(rhs.0 + self)
    }
}

impl Add<RegionPos> for RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: RegionPos) -> Self::Output {
        RegionPos(self.0 + rhs.0)
    }
}

impl Add<&RegionPos> for RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: &RegionPos) -> Self::Output {
        RegionPos(self.0 + rhs.0)
    }
}

impl Add<RegionPos> for &RegionPos {
    type Output = RegionPos;

    fn add(self, rhs: RegionPos) -> Self::Output {
        RegionPos(self.0 + rhs.0)
    }
}

impl Mul<i32> for RegionPos {
    type Output = RegionPos;

    fn mul(self, rhs: i32) -> Self::Output {
        RegionPos(self.0 * rhs)
    }
}

impl Sub<RegionPos> for RegionPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: RegionPos) -> Self::Output {
        self.0 - rhs.0
    }
}

impl Sub<Vector3<i32>> for RegionPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: Vector3<i32>) -> Self::Output {
        self.0 - rhs
    }
}

impl Sub<RegionPos> for Vector3<i32> {
    type Output = Vector3<i32>;

    fn sub(self, rhs: RegionPos) -> Self::Output {
        self - rhs.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChunkPos(Vector3<i32>);

impl ChunkPos {
    pub fn new(vec: Vector3<i32>) -> Self {
        Self(vec)
    }

    pub fn get_region_pos(&self) -> RegionPos {
        RegionPos::new_vec(
            self.map(|x| x.div_euclid(consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32)),
        )
    }

    pub fn get_chunk_traversal(&self) -> u64 {
        let local_pos = self
            .0
            .map(|x| (x.rem_euclid(consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32)) as u32);
        let morton = morton::morton_encode(local_pos);
        morton::morton_traversal_thc(morton, consts::voxel::TERRAIN_REGION_TREE_HEIGHT)
    }
}

impl Deref for ChunkPos {
    type Target = Vector3<i32>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<ChunkPos> for Vector3<i32> {
    fn from(region_pos: ChunkPos) -> Self {
        region_pos.0
    }
}

impl From<Vector3<i32>> for ChunkPos {
    fn from(vec: Vector3<i32>) -> Self {
        ChunkPos(vec)
    }
}

impl Add<Vector3<i32>> for ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: Vector3<i32>) -> Self::Output {
        ChunkPos(self.0 + rhs)
    }
}

impl Add<ChunkPos> for Vector3<i32> {
    type Output = ChunkPos;

    fn add(self, rhs: ChunkPos) -> Self::Output {
        ChunkPos(rhs.0 + self)
    }
}

impl Add<ChunkPos> for ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: ChunkPos) -> Self::Output {
        ChunkPos(self.0 + rhs.0)
    }
}

impl Add<&ChunkPos> for ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: &ChunkPos) -> Self::Output {
        ChunkPos(self.0 + rhs.0)
    }
}

impl Add<ChunkPos> for &ChunkPos {
    type Output = ChunkPos;

    fn add(self, rhs: ChunkPos) -> Self::Output {
        ChunkPos(self.0 + rhs.0)
    }
}

impl Mul<i32> for ChunkPos {
    type Output = ChunkPos;

    fn mul(self, rhs: i32) -> Self::Output {
        ChunkPos(self.0 * rhs)
    }
}

impl Sub<ChunkPos> for ChunkPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: ChunkPos) -> Self::Output {
        self.0 - rhs.0
    }
}

impl Sub<Vector3<i32>> for ChunkPos {
    type Output = Vector3<i32>;

    fn sub(self, rhs: Vector3<i32>) -> Self::Output {
        self.0 - rhs
    }
}

impl Sub<ChunkPos> for Vector3<i32> {
    type Output = Vector3<i32>;

    fn sub(self, rhs: ChunkPos) -> Self::Output {
        self - rhs.0
    }
}

#[derive(Resource)]
pub struct RegionMap {
    /// Only contains regions that have been attempted
    /// to load from disk.
    pub regions: HashMap<RegionPos, WorldRegion>,
    pub pending_region_edits: HashMap<RegionPos, VecDeque<VoxelRegionEdit>>,
    /// Regions that are in the process of loading, waiting on
    /// `Assets` to finish processing the region asset.
    pub loading_regions: HashMap<RegionPos, LoadingRegion>,
    pub region_events: Vec<RegionEvent>,
    pub chunk_events: Vec<ChunkEvent>,

    pub to_set_chunk_sfts: HashMap<RegionPos, Vec<(ChunkId, Option<VoxelModelId>)>>,

    /// The directory that contains all the region files for this RegionMap.
    pub regions_data_path: Option<PathBuf>,

    pub used_materials: HashSet<GameAssetPath>,
}

impl RegionMap {
    pub fn new(region_data_path: Option<PathBuf>) -> Self {
        Self {
            regions: HashMap::new(),
            pending_region_edits: HashMap::new(),
            loading_regions: HashMap::new(),
            region_events: Vec::new(),
            chunk_events: Vec::new(),

            to_set_chunk_sfts: HashMap::new(),

            regions_data_path: region_data_path,
            used_materials: HashSet::new(),
        }
    }

    /// Returns the world voxel that was hit.
    pub fn raycast_terrain(&self, ray: Ray) -> Option<Vector3<i32>> {
        todo!()
    }

    pub fn get_region(&self, region_pos: &RegionPos) -> Option<&WorldRegion> {
        return self.regions.get(region_pos);
    }

    pub fn get_region_mut(&mut self, region_pos: &RegionPos) -> Option<&mut WorldRegion> {
        return self.regions.get_mut(region_pos);
    }

    pub fn set_region(&mut self, region_pos: RegionPos, mut region: WorldRegion) {
        assert!(
            region.ref_count == 0,
            "Region being set should have a ref count of 0."
        );
        region.ref_count = 1;
        let old = self.regions.insert(region_pos, region);
        self.region_events.push(RegionEvent {
            region_pos,
            event_type: if old.is_some() {
                RegionEventType::Updated
            } else {
                RegionEventType::Loaded
            },
        });
        // TODO: Do something with the old region like deallocate voxel model data or something.
    }

    pub fn load_chunk(&mut self, region_pos: &RegionPos, chunk_traversal: u64, chunk_height: u32) {
        todo!("Load the chunk.");
    }

    pub fn get_chunk_model(&self, chunk_id: &ChunkId) -> Option<VoxelModelId> {
        let region_pos = chunk_id.chunk_pos.get_region_pos();
        let region = self.regions.get(&region_pos)?;
        let chunk_traversal = chunk_id.chunk_pos.get_chunk_traversal();
        let mut node_idx = 0;
        for i in 0..chunk_id.chunk_lod.as_tree_height() {
            let child_index = (chunk_traversal >> (i * 6)) & 0b111111;
            let child_bit = 1 << child_index;
            let node_data = &region.tree.nodes[node_idx];
            if node_data.child_mask & child_bit == 0 {
                return None;
            }
            let child_ptr = node_data
                .child_ptr()
                .expect("Child ptr should exist if child bit is set in child mask.")
                as usize;
            node_idx = child_ptr + child_index as usize;
        }
        region.tree.nodes[node_idx]
            .model_ptr()
            .map(|model_ptr| region.model_handles[model_ptr as usize])
    }

    /// Enqueues the chunk to be set, will be applied before rendering.
    pub fn set_chunk(&mut self, chunk_id: ChunkId, sft_id: Option<VoxelModelId>) {
        let region_pos = chunk_id.chunk_pos.get_region_pos();
        if !self.is_region_loaded(&region_pos) {
            self.load_region(&region_pos);
        }
        self.to_set_chunk_sfts
            .entry(region_pos)
            .or_insert_with(|| Vec::new())
            .push((chunk_id, sft_id));
    }

    /// Returns the previous chunks model if it existed.
    fn set_chunk_unchecked(
        regions: &mut HashMap<RegionPos, WorldRegion>,
        chunk_id: &ChunkId,
        sft_id: Option<VoxelModelId>,
    ) -> Option<VoxelModelId> {
        let ChunkId {
            chunk_pos,
            chunk_lod,
        } = chunk_id;
        let region_pos = chunk_pos.get_region_pos();
        let chunk_height = chunk_lod.as_tree_height();
        let chunk_traversal = chunk_pos.get_chunk_traversal();

        let mut region = regions
            .get_mut(&region_pos)
            .expect("Region should exist to set chunk.");
        let mut node_idx = 0;
        for i in 0..chunk_height {
            let child_index = (chunk_traversal >> (i * 6)) & 0b111111;
            let mut node_data = &mut region.tree.nodes[node_idx];
            let child_bit = 1 << child_index;
            if sft_id.is_some() {
                // Ensure child bit is set and allocate child if it doesn't exist.
                node_data.child_mask |= child_bit;
                let child_ptr = if let Some(child_ptr) = node_data.child_ptr() {
                    child_ptr
                } else {
                    let new_child_ptr = region.tree.nodes.len() as u32;
                    region.tree.nodes[node_idx].child_ptr = new_child_ptr;
                    for _ in 0..64 {
                        region
                            .tree
                            .nodes
                            .push(WorldRegionNode::new_empty(node_idx as u32));
                    }
                    new_child_ptr
                };
                node_idx = child_ptr as usize + child_index as usize;
            } else {
                if node_data.child_mask & child_bit != 0 {
                    if i == chunk_height - 1 {
                        node_data.child_mask &= !child_bit;
                        let child_ptr = node_data
                            .child_ptr()
                            .expect("Child ptr should exist if child bit is set in the child mask.")
                            as usize;
                        let parent_node = node_idx;
                        node_idx = child_ptr + child_index as usize;
                        if region.tree.nodes[node_idx].has_model_ptr() {
                            // TODO: Deallocate old model thing.
                        }
                        return None;
                    }
                } else {
                    // Noop since node chunk is already empty.
                    return None;
                }
            }
        }

        let node_data = &mut region.tree.nodes[node_idx];
        let Some(sft_id) = sft_id else {
            unreachable!("Deallocation should've happened in loop.");
        };
        if node_data.has_model_ptr() {
            // TODO: Deallocate old pointer.
        }
        let new_model_ptr = region.model_handles.len() as u32;
        region.model_handles.push(sft_id);
        node_data.model_ptr = new_model_ptr;
        region.active_leaves.insert(node_idx as u32);
        return node_data
            .model_ptr()
            .map(|old_model_ptr| region.model_handles[old_model_ptr as usize]);
    }

    pub fn apply_edit(&mut self, edit: VoxelTerrainEdit) {
        todo!()
        //let region_min = RegionMap::world_to_region_pos(&edit.min.map(|x| x as f31));
        //let region_max = RegionMap::world_to_region_pos(&edit.max.map(|x| x as f32));
        //for region_x in region_min.x..=region_max.x {
        //    for region_y in region_min.y..=region_max.y {
        //        for region_z in region_min.z..=region_max.z {
        //            let region_pos = Vector3::new(region_x, region_y, region_z);
        //            let region_voxel_min =
        //                region_pos.map(|x| x * consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32);
        //            let region_voxel_max = region_voxel_min
        //                + Vector3::new(
        //                    consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32 - 1,
        //                    consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32 - 1,
        //                    consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32 - 1,
        //                );

        //            // Calculate region-local bounds;
        //            let edit_min = Vector3::new(
        //                edit.min.x.max(region_voxel_min.x) - region_voxel_min.x,
        //                edit.min.y.max(region_voxel_min.y) - region_voxel_min.y,
        //                edit.min.z.max(region_voxel_min.z) - region_voxel_min.z,
        //            );
        //            let edit_max = Vector3::new(
        //                edit.max.x.min(region_voxel_max.x) - region_voxel_min.x,
        //                edit.max.y.min(region_voxel_max.y) - region_voxel_min.y,
        //                edit.max.z.min(region_voxel_max.z) - region_voxel_min.z,
        //            );
        //            let region_edit = VoxelRegionEdit {
        //                min: edit_min,
        //                max: edit_max,
        //                data: edit.data.clone(),
        //            };
        //            self.pending_region_edits
        //                .entry(region_pos)
        //                .or_insert_with(VecDeque::new)
        //                .push_back(region_edit);
        //        }
        //    }
        //}
    }

    pub fn is_region_loaded(&self, region_pos: &RegionPos) -> bool {
        self.regions.contains_key(region_pos)
    }

    pub fn load_region(&mut self, region_pos: &RegionPos) {
        assert!(
            !self.is_region_loaded(region_pos),
            "Should check if region is loaded before trying to load it."
        );
        if self.loading_regions.contains_key(region_pos) {
            return;
        }

        self.loading_regions
            .insert(*region_pos, LoadingRegion::new());
    }

    pub fn update_chunks(mut region_map: ResMut<RegionMap>, mut events: ResMut<Events>) {
        let region_map = &mut region_map as &mut RegionMap;

        for (region_pos, vec) in region_map.to_set_chunk_sfts.iter_mut() {
            if !region_map.regions.contains_key(region_pos) {
                continue;
            }
            for (chunk_id, sft_id) in vec.drain(..) {
                Self::set_chunk_unchecked(&mut region_map.regions, &chunk_id, sft_id);
                //region_map.chunk_events.push(ChunkEvent {
                //    region_pos: chunk_pos.get_region_pos(),
                //    chunk_height: chunk_lod.as_tree_height(),
                //    chunk_traversal: chunk_pos.get_chunk_traversal(),
                //    event_type: ChunkEventType::Updated,
                //});
                region_map.chunk_events.push(ChunkEvent {
                    chunk_id,
                    event_type: if sft_id.is_some() {
                        ChunkEventType::Updated
                    } else {
                        ChunkEventType::Unloaded
                    },
                });
                region_map.region_events.push(RegionEvent {
                    region_pos: *region_pos,
                    event_type: RegionEventType::Updated,
                });
            }
        }
    }

    pub fn update_region_loading(
        mut region_map: ResMut<RegionMap>,
        mut assets: ResMut<Assets>,
        mut events: ResMut<Events>,
    ) {
        let region_map = &mut region_map as &mut RegionMap;
        let assets = &mut assets as &mut Assets;

        let Some(assets_dir) = assets.project_assets_dir() else {
            return;
        };

        // Regions that were queued to load and have a valid region representation now, whether
        // from disk or manually set.
        let mut finished_loading = HashSet::new();
        // Regions that were loaded from disk and reside in Assets.
        for (region_pos, LoadingRegion { asset_handle }) in &mut region_map.loading_regions {
            if region_map.regions.contains_key(region_pos) {
                finished_loading.insert(*region_pos);
                continue;
            }

            let Some(asset_handle) = asset_handle else {
                let region_path = AssetPath::new_game_assets_dir(
                    assets_dir.clone(),
                    format!("region_{}_{}_{}", region_pos.x, region_pos.y, region_pos.z),
                );
                *asset_handle = Some(assets.load_asset::<WorldRegionAsset>(region_path));
                continue;
            };

            let mut make_empty_region = true;
            match assets.get_asset_status(&asset_handle) {
                AssetStatus::InProgress => {}
                AssetStatus::Saved => unreachable!(),
                AssetStatus::Loaded => {
                    make_empty_region = false;
                    todo!(
                        "Write loading region data from asset into region map and add to loaded regions list"
                    );
                }
                AssetStatus::NotFound => {
                    make_empty_region = true;
                }
                AssetStatus::Error(error) => {
                    log::error!(
                        "Error while loading region {} {} {}. Error: {}",
                        region_pos.x,
                        region_pos.y,
                        region_pos.z,
                        error
                    );
                    make_empty_region = true;
                }
            }
            if make_empty_region {
                region_map
                    .regions
                    .insert(*region_pos, WorldRegion::new_empty());
            }
            finished_loading.insert(*region_pos);
        }

        for region_pos in finished_loading {
            region_map.loading_regions.remove(&region_pos);
            region_map.region_events.push(RegionEvent {
                region_pos,
                event_type: RegionEventType::Loaded,
            });
            // Region was just loaded in so its ref count should be 1 since someone requested it to
            // be loaded.
            region_map.regions.get_mut(&region_pos).unwrap().ref_count = 1;
        }

        for event in region_map.region_events.drain(..) {
            events.push(event);
        }

        for event in region_map.chunk_events.drain(..) {
            events.push(event);
        }
    }

    pub fn update_region_edits(mut region_map: ResMut<RegionMap>) {
        todo!()
    }
}
