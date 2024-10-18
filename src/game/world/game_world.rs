use core::f32;
use std::borrow::Borrow;

use log::debug;
use nalgebra::{Translation3, Vector3};
use rogue_macros::Resource;

use crate::{
    common::{aabb::AABB, color::Color},
    engine::{
        ecs::ecs_world::ECSWorld,
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::{
            attachment::{Attachment, PTMaterial},
            esvo::VoxelModelESVO,
            flat::VoxelModelFlat,
            vox_consts,
            voxel::{RenderableVoxelModel, VoxelModel, VoxelModelSchema},
        },
        window::time::{Instant, Time},
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

            let i = Instant::now();
            let mut flat_model = VoxelModelFlat::new_empty(Vector3::new(32, 32, 32));
            for (position, mut voxel) in flat_model.xyz_iter_mut() {
                let is_floor = position.y == 0;
                let is_right_wall = position.x == 31;
                let is_left_wall = position.x == 0;
                let is_back_wall = position.z == 31;
                let is_ceiling = position.y == 31;
                let is_light =
                    position.x >= 10 && position.x <= 19 && position.z >= 10 && position.z <= 19;

                if is_floor || is_right_wall || is_left_wall || is_back_wall || is_ceiling {
                    let color = if is_floor || is_back_wall || is_ceiling {
                        if is_ceiling && is_light {
                            Color::new_srgb(1.0, 1.0, 1.0)
                        } else {
                            Color::new_srgb(0.5, 0.5, 0.5)
                        }
                    } else if is_left_wall {
                        Color::new_srgb(1.0, 0.0, 0.0)
                    } else if is_right_wall {
                        Color::new_srgb(0.0, 1.0, 0.0)
                    } else {
                        unreachable!()
                    };

                    let normal: Vector3<f32> = if is_floor {
                        Vector3::y()
                    } else if is_ceiling {
                        -Vector3::y()
                    } else if is_left_wall {
                        Vector3::x()
                    } else if is_right_wall {
                        -Vector3::x()
                    } else if is_back_wall {
                        -Vector3::z()
                    } else {
                        unreachable!()
                    };

                    voxel.set_attachment(
                        Attachment::PTMATERIAL,
                        Some(Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                            color.into_color_space(),
                        ))),
                    );
                    voxel.set_attachment(
                        Attachment::NORMAL,
                        Some(Attachment::encode_normal(normal)),
                    );

                    if is_ceiling && is_light {
                        voxel.set_attachment(
                            Attachment::EMMISIVE,
                            Some(Attachment::encode_emmisive(
                                100.0 * (vox_consts::VOXEL_WORLD_UNIT_LENGTH).powi(2),
                            )),
                        );
                    }
                }
            }
            debug!(
                "Took {} seconds to generate flat model",
                i.elapsed().as_secs_f32()
            );

            // Green box 4x4
            let i = Instant::now();
            let voxel_model = VoxelModel::<VoxelModelESVO>::new((&flat_model).into());
            debug!(
                "Took {} seconds to convert flat model to an esvo model",
                i.elapsed().as_secs_f32()
            );
            // debug!("{:?}", voxel_model);

            ecs_world.spawn(RenderableVoxelModel::new(
                Transform::with_translation(Translation3::new(0.0, 0.0, 1.0)),
                voxel_model.clone(),
            ));
            // ecs_world.spawn(RenderableVoxelModel::new(
            //     Transform::with_translation(Translation3::new(-5.0, 0.0, 2.0)),
            //     voxel_model.clone(),
            // ));
            // ecs_world.spawn(RenderableVoxelModel::new(
            //     Transform::with_translation(Translation3::new(4.5, 6.0, -5.0)),
            //     voxel_model.clone(),
            // ));
        }
    }

    pub fn update_test_models_position(mut ecs_world: ResMut<ECSWorld>, time: Res<Time>) {
        let q = ecs_world
            .query_mut::<(&mut Transform, &VoxelModel<VoxelModelESVO>)>()
            .into_iter();

        for (entity, (transform, voxel_model)) in q {
            transform.isometry.translation.y =
                ((f32::consts::TAU * time.start_time().elapsed().as_secs_f32()) / 2.0).cos() * -4.0
                    - (voxel_model.length().y as f32 * 0.5);
        }
    }
}
