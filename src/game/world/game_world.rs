use std::time::{Duration, Instant};

use nalgebra::{Translation3, Vector3};
use rogue_macros::Resource;

use crate::{
    common::aabb::AABB,
    engine::{
        ecs::ecs_world::ECSWorld,
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{
            esvo::VoxelModelESVO,
            voxel::{RenderableVoxelModel, VoxelModel, VoxelModelSchema},
        },
        window::time::Time,
    },
    game,
};

// Updates per second.
const TICKS_PER_SECOND: u64 = 60;
const MIN_SECONDS_PER_TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

#[derive(Resource)]
pub struct GameWorld {
    last_tick: Instant,

    loaded_test_models: bool,
}

impl GameWorld {
    pub fn new() -> Self {
        Self {
            last_tick: Instant::now(),
            loaded_test_models: false,
        }
    }

    pub fn should_tick(&self) -> bool {
        self.last_tick.elapsed().as_secs_f32() >= MIN_SECONDS_PER_TICK
    }

    pub fn tick(mut game_world: ResMut<GameWorld>) {
        game_world.last_tick = Instant::now();
    }

    pub fn load_test_models(mut game_world: ResMut<GameWorld>, mut ecs_world: ResMut<ECSWorld>) {
        if !game_world.loaded_test_models {
            game_world.loaded_test_models = true;

            // Green box 4x4
            let voxel_model = VoxelModel::from_impl(VoxelModelESVO::new(4));

            ecs_world.spawn(RenderableVoxelModel {
                transform: Transform::with_translation(Translation3::new(1.0, 0.0, 1.0)),
                voxel_model: voxel_model.clone(),
            });

            ecs_world.spawn(RenderableVoxelModel {
                transform: Transform::with_translation(Translation3::new(-5.0, 0.0, 2.0)),
                voxel_model: voxel_model.clone(),
            });

            ecs_world.spawn(RenderableVoxelModel {
                transform: Transform::with_translation(Translation3::new(4.5, 6.0, -5.0)),
                voxel_model: voxel_model.clone(),
            });
        }
    }

    pub fn update_test_models_position(mut ecs_world: ResMut<ECSWorld>, time: Res<Time>) {
        for (entity, (transform, voxel_model)) in ecs_world
            .query_mut::<(&mut Transform, &VoxelModel)>()
            .into_iter()
        {
            transform.isometry.translation.y =
                ((6.28 * time.start_time().elapsed().as_secs_f32()) / 2.0).cos() * -4.0
                    - (voxel_model.length().y as f32 * 0.5);
        }
    }
}
