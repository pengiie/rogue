use core::f32;
use std::{
    borrow::{Borrow, BorrowMut},
    collections::HashSet,
    time::Duration,
};

use log::debug;
use nalgebra::{Translation3, UnitQuaternion, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{aabb::AABB, color::Color},
    engine::{
        asset::{
            asset::{AssetHandle, AssetPath, Assets},
            world::VoxelTerrainAsset,
        },
        ecs::ecs_world::ECSWorld,
        graphics::camera::{Camera, MainCamera},
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{
            attachment::{Attachment, PTMaterial},
            esvo::VoxelModelESVO,
            flat::VoxelModelFlat,
            thc::VoxelModelTHC,
            unit::VoxelModelUnit,
            voxel::{
                RenderableVoxelModel, RenderableVoxelModelRef, VoxelData, VoxelModel,
                VoxelModelSchema,
            },
            voxel_terrain::ChunkTreeNode,
            voxel_transform::VoxelModelTransform,
            voxel_world::{VoxelModelId, VoxelWorld},
        },
        window::time::{Instant, Time, Timer},
    },
    game::{self, entity::player::Player},
    settings::Settings,
};

#[derive(Resource)]
pub struct GameWorld {
    tick_timer: Timer,

    manual_save: bool,
    manual_load: bool,
    // Non-empty while the world is saving or loading.
    waiting_assets: HashSet<AssetHandle>,
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
    ) {
        let voxel_world = &mut voxel_world as &mut VoxelWorld;
        if game_world.manual_save {
            game_world.manual_save = false;

            let mut to_process = vec![voxel_world.terrain.chunk_tree()];
            while !to_process.is_empty() {
                let curr_node = to_process.pop().unwrap();
                for child in curr_node.children.iter() {
                    match child {
                        ChunkTreeNode::Node(sub_tree) => {
                            to_process.push(sub_tree);
                        }
                        ChunkTreeNode::Leaf(chunk) => {
                            let chunk_model =
                                voxel_world.get_model::<VoxelModelTHC>(chunk.voxel_model_id);
                            let chunk_save_handle = assets.save_asset(
                                AssetPath::new_user_dir(format!(
                                    "terrain_chunks::chunk_{}::rog",
                                    chunk.chunk_uuid.as_u128(),
                                )),
                                chunk_model.clone(),
                            );
                            game_world.waiting_assets.insert(chunk_save_handle);
                        }
                        ChunkTreeNode::Empty
                        | ChunkTreeNode::Enqeueud
                        | ChunkTreeNode::Unloaded => {}
                    }
                }
            }

            let terrain_asset = VoxelTerrainAsset::from_terrain(&voxel_world.terrain);
            let terrain_save_handle =
                assets.save_asset(AssetPath::new_user_dir("terrain::rog"), terrain_asset);
            game_world.waiting_assets.insert(terrain_save_handle);
        }

        game_world
            .waiting_assets
            .retain(|waiting_asset| assets.is_asset_loading(waiting_asset));
    }

    fn perform_game_save(&mut self, assets: &mut Assets, ecs_world: &mut ECSWorld) {}

    pub fn try_tick(&mut self) -> bool {
        self.tick_timer.try_complete()
    }
}
