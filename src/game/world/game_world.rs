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
        graphics::camera::{Camera, MainCamera},
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

// Updates per second.
const TICKS_PER_SECOND: u64 = 10;
const MIN_SECONDS_PER_TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

#[derive(Resource)]
pub struct GameWorld {
    tick_timer: Timer,

    test_model_id: VoxelModelId,
    loaded_test_models: bool,
    i: u32,
}

impl GameWorld {
    pub fn new() -> Self {
        Self {
            tick_timer: Timer::new(Duration::from_secs_f32(MIN_SECONDS_PER_TICK)),
            test_model_id: VoxelModelId::null(),
            loaded_test_models: false,
            i: 0,
        }
    }

    pub fn try_tick(&mut self) -> bool {
        self.tick_timer.try_complete()
    }

    pub fn load_test_models(
        mut game_world: ResMut<GameWorld>,
        mut ecs_world: ResMut<ECSWorld>,
        mut voxel_world: ResMut<VoxelWorld>,
    ) {
        if !game_world.loaded_test_models {
            game_world.loaded_test_models = true;

            let i = Instant::now();
            let mut room_model = VoxelModelFlat::new_empty(Vector3::new(256, 256, 256));
            for (position, mut voxel) in room_model.xyz_iter_mut() {
                if position.y == 0 {
                    voxel.set_attachment(
                        Attachment::PTMATERIAL,
                        Some(Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                            Color::new_srgb(
                                (position.x as f32 / 128.0),
                                (position.z as f32 / 128.0),
                                rand::random::<f32>() * 0.1 + 0.45,
                            )
                            .into_color_space(),
                        ))),
                    );
                }
                // let is_floor = position.y == 0;
                // let is_right_wall = position.x == 31;
                // let is_left_wall = position.x == 0;
                // let is_back_wall = position.z == 31;
                // let is_ceiling = position.y == 31;
                // let is_light =
                //     position.x >= 10 && position.x <= 19 && position.z >= 10 && position.z <= 19;

                // if is_floor || is_right_wall || is_left_wall || is_back_wall || is_ceiling {
                //     let color = if is_floor || is_back_wall || is_ceiling {
                //         if is_ceiling && is_light {
                //             Color::new_srgb(1.0, 1.0, 1.0)
                //         } else {
                //             Color::new_srgb(0.5, 0.5, 0.5)
                //         }
                //     } else if is_left_wall {
                //         Color::new_srgb(1.0, 0.0, 0.0)
                //     } else if is_right_wall {
                //         Color::new_srgb(0.0, 1.0, 0.0)
                //     } else {
                //         unreachable!()
                //     };

                //     let normal: Vector3<f32> = if is_floor {
                //         Vector3::y()
                //     } else if is_ceiling {
                //         -Vector3::y()
                //     } else if is_left_wall {
                //         Vector3::x()
                //     } else if is_right_wall {
                //         -Vector3::x()
                //     } else if is_back_wall {
                //         -Vector3::z()
                //     } else {
                //         unreachable!()
                //     };

                //     voxel.set_attachment(
                //         Attachment::PTMATERIAL,
                //         Some(Attachment::encode_ptmaterial(&PTMaterial::diffuse(
                //             color.into_color_space(),
                //         ))),
                //     );
                //     voxel.set_attachment(
                //         Attachment::NORMAL,
                //         Some(Attachment::encode_normal(&normal)),
                //     );

                //     if is_ceiling && is_light {
                //         voxel.set_attachment(
                //             Attachment::EMMISIVE,
                //             Some(Attachment::encode_emmisive(
                //                 (100.0 * (vox_consts::VOXEL_WORLD_UNIT_LENGTH).powi(2)).floor()
                //                     as u32,
                //             )),
                //         );
                //     }
                // }
            }
            debug!(
                "Took {} seconds to generate flat model",
                i.elapsed().as_secs_f32()
            );

            // Green box 4x4
            let i = Instant::now();
            //let room_model = VoxelModel::<VoxelModelESVO>::new((&room_model).into());
            let room_model = VoxelModel::new(room_model);
            //let room_model = VoxelModel::<VoxelModelESVO>::new(VoxelModelESVO::empty(32, true));
            debug!("{:?}", room_model);
            //let room_model_id =
            //    voxel_world.register_renderable_voxel_model("room_model", room_model);
            //game_world.test_model_id = room_model_id;
            debug!(
                "Took {} seconds to convert flat model to an esvo model",
                i.elapsed().as_secs_f32()
            );

            //ecs_world.spawn(RenderableVoxelModel::new(
            //    VoxelModelTransform::with_position(
            //        Vector3::new(0.0, 0.0, 1.0),
            //        //UnitQuaternion::from_euler_angles(0.0, -45.0f32.to_radians(), 0.0),
            //        //
            //    ),
            //    room_model_id,
            //));

            let mut box_model = VoxelModelFlat::new_empty(Vector3::new(4, 8, 4));
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

    /// Operated only on every tick.
    pub fn update_test_models_position(
        mut ecs_world: ResMut<ECSWorld>,
        time: Res<Time>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut game_world: ResMut<GameWorld>,
    ) {
        //let voxel_model = voxel_world.get_dyn_model_mut(game_world.test_model_id);

        //let length = voxel_model.length();
        //let index = game_world.i as usize;
        //let curr_pos = Vector3::new(
        //    (index % length.x as usize) as u32,
        //    ((index / length.x as usize) % length.y as usize) as u32,
        //    (index / (length.x as usize * length.y as usize)) as u32,
        //);
        //let unit_voxel = VoxelModelUnit::with_data(
        //    VoxelData::empty().with_diffuse(Color::new_srgb(1.0, 0.0, 0.0)),
        //);
        ////voxel_model.set_voxel_range(VoxelRange::from_unit(curr_pos, unit_voxel));
        ////debug!(
        ////    "Set voxel at {} {} {} for i == {}",
        ////    curr_pos.x, curr_pos.y, curr_pos.z, game_world.i
        ////);
        //game_world.i = (game_world.i + 1) % voxel_model.volume() as u32;
        //// for (entity, (vox_transform)) in ecs_world
        ////     .query_mut::<(&mut VoxelModelTransform)>()
        ////     .into_iter()
        //// {
        ////     if
        ////     vox_transform.rotation *=
        ////         UnitQuaternion::from_euler_angles(0.0, -0.5f32.to_radians(), 0.0);
        //// }
    }

    pub fn spawn_player(mut ecs_world: ResMut<ECSWorld>, mut main_camera: ResMut<MainCamera>) {
        if ecs_world.query::<()>().with::<&Player>().iter().len() > 0 {
            panic!("Player already spawned.");
        }

        let player = ecs_world.spawn((
            Player::new(),
            Camera::new(90.0),
            Transform::with_translation(Translation3::new(0.0, 0.0, 0.0)),
        ));
        main_camera.set_camera(player, "player_camera");
    }
}
