use std::{
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    ops::{Add, Deref, Mul, Sub},
    path::PathBuf,
};

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::world::terrain::region::{RegionTree, WorldRegion, WorldRegionNode};
use crate::world::terrain::region_asset::WorldRegionAsset;
use crate::world::terrain::region_pos::RegionPos;
use crate::{
    asset::asset::GameAssetPath,
    common::geometry::ray::RayDDA,
    resource::ResMut,
    voxel::{voxel::VoxelModelEditMask, voxel_registry::VoxelModelRegistry},
};
use crate::{
    asset::asset::{AssetHandle, AssetPath, AssetStatus, Assets},
    voxel::voxel::VoxelModelEditOperator,
};
use crate::{common::geometry::ray::Ray, consts};
use crate::{
    common::morton,
    event::Events,
    voxel::{sft_compressed::VoxelModelSFTCompressed, voxel_registry::VoxelModelId},
};
use crate::{
    event::EventReader,
    voxel::voxel::{VoxelModelEditMaskLayer, VoxelModelTrace},
};
use crate::{voxel::attachment::Attachment, world::terrain::chunk_pos::ChunkPos};
use crate::{voxel::voxel::VoxelModelEdit, world::terrain::chunk_lod::ChunkLOD};

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

#[derive(Clone, Debug)]
pub enum RegionMapCommandEvent {
    SetTerrainAssetPath { asset_path: GameAssetPath },
}

pub struct VoxelTerrainRegion {
    /// In world space voxel coordinates.
    pub min: Vector3<i32>,
    pub max: Vector3<i32>,
}

impl VoxelTerrainRegion {
    pub fn new_rect(min: Vector3<i32>, max: Vector3<i32>) -> Self {
        Self { min, max }
    }

    pub fn get_affected_chunk_models(
        &self,
        region_map: &RegionMap,
    ) -> Vec<(ChunkId, Option<VoxelModelId>)> {
        let chunk_min = ChunkPos::from_world_voxel_pos(&self.min);
        let chunk_max = ChunkPos::from_world_voxel_pos(&self.max);
        let mut affected_chunks = Vec::new();
        for chunk_x in chunk_min.x..=chunk_max.x {
            for chunk_y in chunk_min.y..=chunk_max.y {
                for chunk_z in chunk_min.z..=chunk_max.z {
                    let chunk_pos = ChunkPos::new(Vector3::new(chunk_x, chunk_y, chunk_z));
                    let chunk_id = ChunkId {
                        chunk_pos,
                        chunk_lod: ChunkLOD::FULL_RES_LOD,
                    };
                    let region_pos = chunk_pos.get_region_pos();
                    let chunk_model = region_map
                        .get_region(&region_pos)
                        .and_then(|region| region.get_chunk_model(chunk_id));
                    affected_chunks.push((chunk_id, chunk_model));
                }
            }
        }
        return affected_chunks;
    }
}

pub struct VoxelTerrainEditMask {
    pub layers: Vec<VoxelTerrainEditMaskLayer>,
}

/// Intentially different since masks may need conversion with an offset for the model mask.
#[derive(Clone)]
pub struct VoxelTerrainEditMaskLayer(pub VoxelModelEditMaskLayer);

impl VoxelTerrainEditMaskLayer {
    pub fn as_chunk_model_mask_layer(
        &self,
        chunk_world_voxel_min_pos: &Vector3<i32>,
    ) -> VoxelModelEditMaskLayer {
        let chunk_voxel_pos = chunk_world_voxel_min_pos;
        let mut s = self.0.clone();
        match &mut s {
            VoxelModelEditMaskLayer::Presence => {}
            VoxelModelEditMaskLayer::Sphere { center, diameter } => *center -= chunk_voxel_pos,
        }
        return s;
    }
}

pub struct VoxelTerrainEdit {
    pub region: VoxelTerrainRegion,
    pub mask: VoxelTerrainEditMask,
    pub operator: VoxelModelEditOperator,
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

pub struct LoadingRegion {
    pub asset_handle: Option<AssetHandle>,
}

impl LoadingRegion {
    pub fn new() -> Self {
        Self { asset_handle: None }
    }
}

pub struct TerrainRaycastHit {
    pub world_voxel_pos: Vector3<i32>,
    pub model_trace: VoxelModelTrace,
}

#[derive(Resource)]
pub struct RegionMap {
    /// Only contains regions that have been attempted
    /// to load from disk.
    pub regions: HashMap<RegionPos, WorldRegion>,

    /// Regions that are in the process of loading, waiting on
    /// `Assets` to finish processing the region asset.
    pub loading_regions: HashMap<RegionPos, LoadingRegion>,

    pub region_events: Vec<RegionEvent>,
    pub chunk_events: Vec<ChunkEvent>,
    pub command_event_render: EventReader<RegionMapCommandEvent>,

    pub to_set_chunk_sfts: HashMap<RegionPos, Vec<(ChunkId, Option<VoxelModelId>)>>,
    pub to_apply_edits: Vec<VoxelTerrainEdit>,

    pub used_materials: HashSet<GameAssetPath>,

    pub save_dir: Option<GameAssetPath>,
}

impl RegionMap {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
            loading_regions: HashMap::new(),

            region_events: Vec::new(),
            chunk_events: Vec::new(),
            command_event_render: EventReader::new(),

            to_set_chunk_sfts: HashMap::new(),
            to_apply_edits: Vec::new(),
            save_dir: None,
            used_materials: HashSet::new(),
        }
    }

    /// Returns the world voxel that was hit.
    pub fn raycast_terrain(
        &self,
        voxel_registry: &VoxelModelRegistry,
        ray: &Ray,
        max_t: f32,
    ) -> Option<TerrainRaycastHit> {
        // DDA the region grid. Only really important when we are on a region boundry since max_t
        // is less than a region in most cases.
        let region_pos = ray
            .origin
            .map(|x| x / consts::voxel::TERRAIN_REGION_METER_LENGTH);
        let mut curr_grid = region_pos.map(|x| x.floor() as i32);
        let unit_grid = ray.dir.map(|x| x.signum() as i32);
        let next_point = curr_grid.cast::<f32>() + (unit_grid.cast::<f32>() * 0.5).add_scalar(0.5);
        let mut curr_t = ray
            .inv_dir
            .component_mul(&(next_point - region_pos))
            .map(|x| if x.is_infinite() { 10000.00 } else { x });
        let unit_t = ray
            .inv_dir
            .map(|x| if x.is_infinite() { 0.0 } else { x.abs() });
        let mut traversed_distance = 0.0;
        while (traversed_distance * consts::voxel::TERRAIN_REGION_METER_LENGTH < max_t) {
            if let Some(region) = self.get_region(&RegionPos::new_vec(curr_grid)) {
                if let Some(res) = region.raycast_region(voxel_registry, ray, max_t) {
                    return Some(res);
                }
            }

            // Step.
            let min_t = curr_t.min();
            let mask = curr_t.map(|x| if x == min_t { 1 } else { 0 });
            curr_grid += mask.component_mul(&unit_grid);
            curr_t += mask.cast::<f32>().component_mul(&unit_t);
            traversed_distance = min_t;
        }

        return None;
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
        self.ensure_region_loaded(&region_pos);
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
        let mut region = regions
            .get_mut(&chunk_id.chunk_pos.get_region_pos())
            .expect("Region should exist to set chunk.");
        return region.set_chunk_model(chunk_id, sft_id);
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
                    .insert(*region_pos, WorldRegion::new_empty(*region_pos));
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

    pub fn mark_chunk_updated(&mut self, chunk_id: &ChunkId) {
        self.chunk_events.push(ChunkEvent {
            chunk_id: *chunk_id,
            event_type: ChunkEventType::Updated,
        });
    }

    pub fn update_region_edits(
        mut region_map: ResMut<RegionMap>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
    ) {
        let region_map = &mut *region_map;
        let mut finished_edit_indices = Vec::new();
        for (edit_index, edit) in region_map.to_apply_edits.iter().enumerate() {
            let region_min = RegionPos::from_world_voxel_pos(&edit.region.min);
            let region_max = RegionPos::from_world_voxel_pos(&edit.region.max);
            let mut missing_region = false;
            for region_x in region_min.x..=region_max.x {
                if missing_region {
                    break;
                }
                for region_y in region_min.y..=region_max.y {
                    if missing_region {
                        break;
                    }
                    for region_z in region_min.z..=region_max.z {
                        if missing_region {
                            break;
                        }
                        let region_pos = RegionPos::new(region_x, region_y, region_z);
                        let region = match region_map.regions.get_mut(&region_pos) {
                            Some(region) => region,
                            None => {
                                missing_region = true;
                                continue;
                            }
                        };
                    }
                }
            }
            if missing_region {
                log::info!("Mising region");
                // Put off edit for now since regions are loading.
                continue;
            }

            finished_edit_indices.push(edit_index);
            let chunk_min = ChunkPos::from_world_voxel_pos(&edit.region.min);
            let chunk_max = ChunkPos::from_world_voxel_pos(&edit.region.max);
            for chunk_x in chunk_min.x..=chunk_max.x {
                for chunk_y in chunk_min.y..=chunk_max.y {
                    for chunk_z in chunk_min.z..=chunk_max.z {
                        let chunk_id = ChunkId {
                            chunk_pos: ChunkPos::new(Vector3::new(chunk_x, chunk_y, chunk_z)),
                            chunk_lod: ChunkLOD::FULL_RES_LOD,
                        };
                        let region_pos = chunk_id.chunk_pos.get_region_pos();
                        let region = region_map
                            .regions
                            .get_mut(&region_pos)
                            .expect("Region should be loaded.");
                        let chunk_model_id = match region.get_chunk_model(chunk_id) {
                            Some(model_id) => model_id,
                            None => {
                                let mut empty_chunk_model = VoxelModelSFTCompressed::new_empty(
                                    consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                );
                                empty_chunk_model.initialize_attachment_buffers(&Attachment::BMAT);
                                let voxel_model_id =
                                    voxel_registry.register_voxel_model(empty_chunk_model, None);
                                let res = region.set_chunk_model(&chunk_id, Some(voxel_model_id));
                                assert!(
                                    res.is_none(),
                                    "WorldRegion::get_chunk_model returned None so there shouldn't be an existing model, chunk id {:?}",
                                    chunk_id
                                );
                                region_map.region_events.push(RegionEvent {
                                    region_pos,
                                    event_type: RegionEventType::Updated,
                                });
                                voxel_model_id
                            }
                        };
                        let chunk_model = voxel_registry.get_dyn_model_mut(chunk_model_id);
                        let chunk_voxel_min = chunk_id.chunk_pos.get_min_world_voxel_pos();
                        let chunk_voxel_max = chunk_voxel_min
                            .add_scalar(consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH as i32 - 1);
                        let clamped_min =
                            edit.region.min.zip_map(&chunk_voxel_min, |x, y| x.max(y));
                        let clamped_max =
                            edit.region.max.zip_map(&chunk_voxel_max, |x, y| x.min(y));
                        let model_min = (clamped_min - chunk_voxel_min).map(|x| x as u32);
                        let model_max = (clamped_max - chunk_voxel_min).map(|x| x as u32);
                        assert!(
                            model_min
                                .zip_map(&model_max, |min, max| min <= max)
                                .iter()
                                .all(|x| *x),
                            "Clamped max should be greater than clamped min."
                        );
                        let model_edit = VoxelModelEdit {
                            region: crate::voxel::voxel::VoxelModelEditRegion::Rect {
                                min: model_min,
                                max: model_max,
                            },
                            mask: VoxelModelEditMask {
                                layers: edit
                                    .mask
                                    .layers
                                    .iter()
                                    .map(|layer| layer.as_chunk_model_mask_layer(&chunk_voxel_min))
                                    .collect::<Vec<_>>(),
                                mask_source: None,
                            },
                            operator: edit.operator.clone(),
                        };
                        chunk_model.set_voxel_range_impl(&model_edit);
                        region_map.chunk_events.push(ChunkEvent {
                            chunk_id,
                            event_type: ChunkEventType::Updated,
                        });
                    }
                }
            }
        }

        for edit_index in finished_edit_indices.into_iter().rev() {
            region_map.to_apply_edits.swap_remove(edit_index);
        }
    }

    pub fn ensure_region_loaded(&mut self, region_pos: &RegionPos) {
        if !self.is_region_loaded(region_pos) {
            self.load_region(region_pos);
        }
    }

    pub fn apply_voxel_edit(&mut self, edit: VoxelTerrainEdit) {
        let region_min = RegionPos::from_world_voxel_pos(&edit.region.min);
        let region_max = RegionPos::from_world_voxel_pos(&edit.region.max);
        for region_x in region_min.x..=region_max.x {
            for region_y in region_min.y..=region_max.y {
                for region_z in region_min.z..=region_max.z {
                    self.ensure_region_loaded(&RegionPos::new(region_x, region_y, region_z));
                }
            }
        }
        self.to_apply_edits.push(edit);
    }
}
