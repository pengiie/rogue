use std::{
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    path::PathBuf,
};

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    common::geometry::ray::Ray,
    consts,
};
use crate::asset::asset::{AssetHandle, AssetPath, AssetStatus, Assets};
use crate::event::Events;
use crate::resource::ResMut;
use crate::voxel::voxel::VoxelEditData;
use crate::world::{
    region::{ChunkPos, WorldRegion},
    region_asset::WorldRegionAsset,
};

pub struct EventRegionLoaded {
    region_pos: RegionPos,
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

pub type RegionPos = Vector3<i32>;

#[derive(Resource)]
pub struct RegionMap {
    /// Only contains regions that have been attempted
    /// to load from disk.
    pub regions: HashMap<RegionPos, WorldRegion>,
    pub pending_region_edits: HashMap<RegionPos, VecDeque<VoxelRegionEdit>>,
    /// Regions that are in the process of loading, waiting on
    /// `Assets` to finish processing the region asset.
    pub loading_regions: HashMap<RegionPos, Option<AssetHandle>>,

    /// The directory that contains all the region files for this RegionMap.
    pub regions_data_path: Option<PathBuf>,
}

impl RegionMap {
    pub fn new(region_data_path: Option<PathBuf>) -> Self {
        Self {
            regions: HashMap::new(),
            pending_region_edits: HashMap::new(),
            loading_regions: HashMap::new(),

            regions_data_path: region_data_path,
        }
    }

    /// Returns the world voxel that was hit.
    pub fn raycast_terrain(&self, ray: Ray) -> Option<Vector3<i32>> {
        todo!()
    }

    pub fn apply_edit(&mut self, edit: VoxelTerrainEdit) {
        let region_min = RegionMap::world_to_region_pos(&edit.min.map(|x| x as f32));
        let region_max = RegionMap::world_to_region_pos(&edit.max.map(|x| x as f32));
        for region_x in region_min.x..=region_max.x {
            for region_y in region_min.y..=region_max.y {
                for region_z in region_min.z..=region_max.z {
                    let region_pos = Vector3::new(region_x, region_y, region_z);
                    let region_voxel_min =
                        region_pos.map(|x| x * consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32);
                    let region_voxel_max = region_voxel_min
                        + Vector3::new(
                            consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32 - 1,
                            consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32 - 1,
                            consts::voxel::TERRAIN_REGION_VOXEL_LENGTH as i32 - 1,
                        );

                    // Calculate region-local bounds;
                    let edit_min = Vector3::new(
                        edit.min.x.max(region_voxel_min.x) - region_voxel_min.x,
                        edit.min.y.max(region_voxel_min.y) - region_voxel_min.y,
                        edit.min.z.max(region_voxel_min.z) - region_voxel_min.z,
                    );
                    let edit_max = Vector3::new(
                        edit.max.x.min(region_voxel_max.x) - region_voxel_min.x,
                        edit.max.y.min(region_voxel_max.y) - region_voxel_min.y,
                        edit.max.z.min(region_voxel_max.z) - region_voxel_min.z,
                    );
                    let region_edit = VoxelRegionEdit {
                        min: edit_min,
                        max: edit_max,
                        data: edit.data.clone(),
                    };
                    self.pending_region_edits
                        .entry(region_pos)
                        .or_insert_with(VecDeque::new)
                        .push_back(region_edit);
                }
            }
        }
    }

    /// If returns None, then should listen to the region load event.
    pub fn get_or_load_region(&mut self, region_pos: &RegionPos) -> Option<&WorldRegion> {
        if let Some(region) = self.regions.get(region_pos) {
            return Some(region);
        }

        if !self.loading_regions.contains_key(region_pos) {
            self.loading_regions.insert(*region_pos, None);
        }
        return None;
    }

    pub fn update_region_streaming(
        mut region_map: ResMut<RegionMap>,
        mut assets: ResMut<Assets>,
        mut events: ResMut<Events>,
    ) {
        let region_map = &mut region_map as &mut RegionMap;
        let assets = &mut assets as &mut Assets;

        // Update region loading.
        let mut finished_loading = HashSet::new();
        for (region_pos, asset_handle) in &mut region_map.loading_regions {
            if region_map.regions.contains_key(region_pos) {
                finished_loading.insert(*region_pos);
                continue;
            }

            let Some(ref mut asset_handle) = asset_handle else {
                let region_path = AssetPath::new_game_assets_dir(
                    assets.project_assets_dir().clone().unwrap(),
                    format!("region_{}_{}_{}", region_pos.x, region_pos.y, region_pos.z),
                );
                *asset_handle = Some(assets.load_asset::<WorldRegionAsset>(region_path));
                continue;
            };

            let mut make_empty_region = false;
            match assets.get_asset_status(&asset_handle) {
                AssetStatus::InProgress => {}
                AssetStatus::Saved => unreachable!(),
                AssetStatus::Loaded => {}
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
                finished_loading.insert(*region_pos);
            }
        }
        for region_pos in finished_loading {
            region_map.loading_regions.remove(&region_pos);
        }
    }

    pub fn update_region_edits(mut region_map: ResMut<RegionMap>) {
        todo!()
    }

    pub fn chunk_to_region_pos(chunk_pos: &ChunkPos) -> RegionPos {
        chunk_pos.map(|x| x.div_euclid(consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32))
    }

    pub fn region_to_chunk_pos(region_pos: &RegionPos) -> RegionPos {
        region_pos.map(|x| x * consts::voxel::TERRAIN_REGION_CHUNK_LENGTH as i32)
    }

    pub fn world_to_region_pos(world_voxel_pos: &Vector3<f32>) -> RegionPos {
        world_voxel_pos.map(|x| (x / consts::voxel::TERRAIN_REGION_METER_LENGTH).floor() as i32)
    }
}
