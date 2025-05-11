use core::f32;
use std::{
    borrow::{Borrow, BorrowMut},
    collections::HashSet,
    time::Duration,
};

use hecs::With;
use log::debug;
use nalgebra::{Translation3, UnitComplex, UnitQuaternion, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{aabb::AABB, color::Color},
    engine::{
        asset::asset::{AssetHandle, AssetPath, AssetStatus, Assets},
        entity::{ecs_world::ECSWorld, RenderableVoxelEntity},
        graphics::camera::{Camera, MainCamera},
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{
            attachment::{Attachment, PTMaterial},
            esvo::VoxelModelESVO,
            flat::VoxelModelFlat,
            thc::VoxelModelTHC,
            voxel::{VoxelData, VoxelModel, VoxelModelSchema},
            voxel_terrain::{self},
            voxel_transform::VoxelModelTransform,
            voxel_world::VoxelWorld,
        },
        window::time::{Instant, Time, Timer},
    },
    game::{self, entity::player::Player},
    settings::Settings,
};

pub struct GameWorldLoadState {
    terrain_handle: Option<AssetHandle>,
}

/// Handles the general state of the current world.
///  - Loading, Saving
#[derive(Resource)]
pub struct GameWorld {
    tick_timer: Timer,

    manual_save: bool,
    manual_load: bool,

    // Non-empty while the world is saving.
    waiting_assets: HashSet<AssetHandle>,
    load_state: Option<GameWorldLoadState>,
}

impl GameWorld {
    pub fn new(settings: &Settings) -> Self {
        Self {
            tick_timer: Timer::new(Duration::from_secs_f32(
                1.0 / settings.ticks_per_seconds as f32,
            )),

            manual_save: false,
            manual_load: false,

            waiting_assets: HashSet::new(),
            load_state: None,
        }
    }

    pub fn is_io_busy(&self) -> bool {
        !self.waiting_assets.is_empty()
    }

    pub fn save(&mut self) {
        self.manual_save = true;
    }

    pub fn load(&mut self) {
        self.manual_load = true;
    }

    pub fn update_io(
        mut game_world: ResMut<GameWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut assets: ResMut<Assets>,
        mut ecs_world: ResMut<ECSWorld>,
        time: Res<Time>,
    ) {
        let voxel_world = &mut voxel_world as &mut VoxelWorld;

        let can_save_or_load =
            game_world.load_state.is_none() && game_world.waiting_assets.is_empty();
        if game_world.manual_save && can_save_or_load {
            debug!("Performing game save.");
            game_world.manual_save = false;
            game_world.perform_game_save(&mut assets, &mut ecs_world, voxel_world);
        } else if game_world.manual_load && can_save_or_load {
        }

        let mut q = ecs_world.query::<With<&mut Transform, &RenderableVoxelEntity>>();
        for (entity, (mut transform)) in q.iter() {
            //  transform.rotation *= (&UnitQuaternion::from_axis_angle(
            //      &Vector3::y_axis(),
            //      f32::consts::PI * 2.0 * time.delta_time().as_secs_f32() * 0.25, // 4 seconds per
            //                                                                      // rot
            //  ));
        }

        // Check which assets successfully saved.
        game_world
            .waiting_assets
            .retain(|waiting_asset| assets.is_asset_loading(waiting_asset));
    }

    fn perform_game_save(
        &mut self,
        assets: &mut Assets,
        ecs_world: &mut ECSWorld,
        voxel_world: &mut VoxelWorld,
    ) {
        // let mut to_process = vec![voxel_world.chunks.chunk_tree()];
        // while !to_process.is_empty() {
        //     let curr_node = to_process.pop().unwrap();
        //     for child in curr_node.children.iter() {
        //         match child {
        //             ChunkTreeNode::Node(sub_tree) => {
        //                 to_process.push(sub_tree);
        //             }
        //             ChunkTreeNode::Leaf(chunk) => {
        //                 let chunk_model = voxel_world
        //                     .get_model::<voxel_terrain::ChunkModelType>(chunk.voxel_model_id);
        //                 // let chunk_save_handle = assets.save_asset(
        //                 //     AssetPath::new_user_dir(format!(
        //                 //         "terrain_chunks::chunk_{}::rog",
        //                 //         chunk.chunk_uuid.as_u128(),
        //                 //     )),
        //                 //     chunk_model.clone(),
        //                 // );
        //                 // self.waiting_assets.insert(chunk_save_handle);
        //             }
        //             ChunkTreeNode::Empty | ChunkTreeNode::Enqeueud | ChunkTreeNode::Unloaded => {}
        //         }
        //     }
        // }

        //let terrain_asset = VoxelTerrainAsset::from_terrain(&voxel_world.chunks);
        //let terrain_save_handle =
        //    assets.save_asset(AssetPath::new_user_dir("terrain::rog"), terrain_asset);
        //self.waiting_assets.insert(terrain_save_handle);
    }

    fn perform_game_load(&mut self, assets: &mut Assets, voxel_world: &mut VoxelWorld) {
        //let terrain_handle =
        //    assets.load_asset::<VoxelTerrainAsset>(AssetPath::new_user_dir("terrain::rog"));
        //self.load_state = Some(GameWorldLoadState {
        //    terrain_handle: Some(terrain_handle),
        //})
    }
    fn update_game_load(&mut self, assets: &mut Assets, voxel_world: &mut VoxelWorld) {
        let Some(load_state) = &mut self.load_state else {
            return;
        };

        if let Some(terrain_handle) = &load_state.terrain_handle {
            // let loaded_terrain: Option<anyhow::Result<Box<VoxelTerrainAsset>>> = match assets
            //     .get_asset_status(&load_state.terrain_handle)
            // {
            //     AssetStatus::InProgress => None,
            //     AssetStatus::Loaded => Some(anyhow::Result::Ok(
            //         assets
            //             .take_asset::<VoxelTerrainAsset>(&load_state.terrain_handle)
            //             .unwrap(),
            //     )),
            //     AssetStatus::NotFound => Some(anyhow::Result::Err(anyhow::anyhow!(
            //         "Couldn't find terrain asset in user dir."
            //     ))),
            //     AssetStatus::Error(err) => Some(anyhow::Result::Err(anyhow::anyhow!("{}", err))),
            // };
            // let Some(loaded_terrain_status) = loaded_terrain else {
            //     // Still loading.
            //     return;
            // };

            // let terrain_asset = match loaded_terrain_status {
            //     Ok(terrain) => terrain,
            //     Err(err) => {
            //         log::error!("Failed to load terrain asset: {}", err);
            //         self.load_state = None;
            //         return;
            //     }
            // };

            // if terrain_asset.side_length != voxel_world.terrain.chunk_tree.chunk_side_length {
            //     log::error!("Failed to load terrain asset since loaded size {} is not equal to the current size {}, TODO: Redo how terrain storage is handled.", terrain_asset.side_length, voxel_world.terrain.chunk_tree.chunk_side_length);
            //     self.load_state = None;
            //     return;
            // }

            // let tree_offset =
            // for morton in 0..voxel_world.terrain.chunk_tree.volume() {
            //     let loaded_uuid = terrain_asset.chunk_tree[voxel_world.termorton]
            // }

            // return;
        }
    }

    pub fn try_tick(&mut self) -> bool {
        self.tick_timer.try_complete()
    }
}
