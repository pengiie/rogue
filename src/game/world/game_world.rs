use core::f32;
use std::{
    borrow::{Borrow, BorrowMut},
    time::Duration,
};

use log::debug;
use nalgebra::{Translation3, UnitQuaternion, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{aabb::AABB, color::Color},
    engine::{
        ecs::ecs_world::ECSWorld,
        graphics::camera::Camera,
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{
            attachment::{Attachment, PTMaterial},
            esvo::VoxelModelESVO,
            flat::VoxelModelFlat,
            unit::VoxelModelUnit,
            voxel::{
                RenderableVoxelModel, RenderableVoxelModelRef, VoxelData, VoxelModel,
                VoxelModelSchema, VoxelRange,
            },
            voxel_transform::VoxelModelTransform,
            voxel_world::{VoxelModelId, VoxelWorld},
        },
        window::time::{Instant, Time, Timer},
    },
    game::{self, player::player::Player},
};

// -----------------------------------------------------------------------------
// Constants & Configurations
// -----------------------------------------------------------------------------

/// Number of updates per second (tick rate).
pub const TICKS_PER_SECOND: u64 = 10;

/// Minimum number of seconds per tick, derived from TICKS_PER_SECOND.
pub const MIN_SECONDS_PER_TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

/// For advanced expansions, consider dynamic adjustments based on game load or difficulty.
#[allow(dead_code)]
const FUTURE_DYNAMIC_TICK_RATE: bool = false;

// -----------------------------------------------------------------------------
// The main resource managing the game world
// -----------------------------------------------------------------------------

/// `GameWorld` holds state about the current voxel environment,
/// loaded models, and timing for tick updates. This is where we
/// can add expansions for AI behaviors, dynamic voxel changes,
/// or network replication in the future.
#[derive(Resource)]
pub struct GameWorld {
    /// A timer ensuring updates (ticks) happen at a consistent rate.
    tick_timer: Timer,

    /// A placeholder for an ID referencing a specific voxel model.
    test_model_id: VoxelModelId,

    /// Whether test models have been loaded into the voxel world.
    loaded_test_models: bool,

    /// A counter (e.g., iterating over voxel volume). Potentially used to
    /// demonstrate or test incremental voxel updates.
    i: u32,
}

impl GameWorld {
    /// Creates a new, uninitialized `GameWorld` instance.
    pub fn new() -> Self {
        Self {
            tick_timer: Timer::new(Duration::from_secs_f32(MIN_SECONDS_PER_TICK)),
            test_model_id: VoxelModelId::null(),
            loaded_test_models: false,
            i: 0,
        }
    }

    /// Attempts to complete the tick timer; returns `true` if a tick is due.
    pub fn try_tick(&mut self) -> bool {
        self.tick_timer.try_complete()
    }

    /// Loads a sample voxel model (e.g., a "room" or "box") into the voxel world.
    /// 
    /// - Creates a flat model sized 256x256x256
    /// - Potential expansions: Generating random caves, layered biomes, or large structures
    /// - Attaches colors or materials to voxel data
    pub fn load_test_models(
        mut game_world: ResMut<GameWorld>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
    ) {
        if game_world.loaded_test_models {
            // Already loaded, skip further init
            return;
        }
        game_world.loaded_test_models = true;

        // Example: generate a large flat model
        let start_time = Instant::now();
        let mut room_model = VoxelModelFlat::new_empty(Vector3::new(256, 256, 256));

        for (position, mut voxel) in room_model.xyz_iter_mut() {
            // Simple logic: if y=0, apply a random PTMaterial color
            if position.y == 0 {
                let color_srgb = Color::new_srgb(
                    position.x as f32 / 128.0,
                    position.z as f32 / 128.0,
                    rand::random::<f32>() * 0.1 + 0.45,
                );
                voxel.set_attachment(
                    Attachment::PTMATERIAL,
                    Some(Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                        color_srgb.into_color_space(),
                    ))),
                );
            }
            // Additional logic can go here for walls, ceilings, etc.
        }
        debug!(
            "Took {} seconds to generate flat model",
            start_time.elapsed().as_secs_f32()
        );

        let conversion_start = Instant::now();
        let room_model = VoxelModel::new(room_model);
        debug!("Created VoxelModel: {:?}", room_model);
        debug!(
            "Took {} seconds to convert flat model to an esvo model",
            conversion_start.elapsed().as_secs_f32()
        );

        // Example of future usage:
        // let model_id = voxel_world.register_renderable_voxel_model("room_model", room_model);
        // game_world.test_model_id = model_id;

        // We can spawn an entity with RenderableVoxelModel, if desired
        // ecs_world.spawn(RenderableVoxelModel::new(
        //     VoxelModelTransform::with_position(Vector3::new(0.0, 0.0, 1.0)),
        //     model_id,
        // ));

        // Optionally create a small box model (4x8x4) as a demonstration
        // let mut box_model = VoxelModelFlat::new_empty(Vector3::new(4, 8, 4));
        // Additional logic here for building or coloring the box model
    }

    /// Called every tick (or whenever the engine decides). 
    /// Demonstrates incremental voxel updates or model manipulation.
    /// 
    /// - Could be expanded with AI pathfinding, dynamic block removal, etc.
    pub fn update_test_models_position(
        mut ecs_world: ResMut<ECSWorld>,
        time: Res<Time>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut game_world: ResMut<GameWorld>,
    ) {
        // Pseudocode for incremental voxel changes:
        // let voxel_model = voxel_world.get_dyn_model_mut(game_world.test_model_id);
        // let length = voxel_model.length();
        // let idx = game_world.i as usize;
        // let curr_pos = Vector3::new(
        //     (idx % length.x as usize) as u32,
        //     ((idx / length.x as usize) % length.y as usize) as u32,
        //     (idx / (length.x as usize * length.y as usize)) as u32,
        // );
        // let unit_voxel = VoxelModelUnit::with_data(
        //     VoxelData::empty().with_diffuse(Color::new_srgb(1.0, 0.0, 0.0))
        // );
        // voxel_model.set_voxel_range(VoxelRange::from_unit(curr_pos, unit_voxel));
        //
        // debug!(
        //     "Set voxel at {} {} {} for i == {}",
        //     curr_pos.x, curr_pos.y, curr_pos.z, game_world.i
        // );
        // game_world.i = (game_world.i + 1) % voxel_model.volume() as u32;
        //
        // For rotation logic on ECS entities that hold VoxelModelTransform:
        // for (entity, mut vox_transform) in ecs_world.query_mut::<(&mut VoxelModelTransform)>() {
        //     vox_transform.rotation *= UnitQuaternion::from_euler_angles(0.0, -0.5f32.to_radians(), 0.0);
        // }
    }

    /// Spawns the main player character (ensuring only one exists).
    /// In the future, we could spawn multiple players for co-op or an AI-driven companion.
    pub fn spawn_player(mut ecs_world: ResMut<ECSWorld>) {
        // Ensure we only have one player
        let existing_players = ecs_world
            .query::<()>()
            .with::<&Player>()
            .iter()
            .count();

        if existing_players > 0 {
            panic!("Player already spawned.");
        }

        ecs_world.spawn((
            Player::new(),
            Camera::new(),
            Transform::with_translation(Translation3::new(0.0, 8.0, 0.0)),
        ));
    }
}
