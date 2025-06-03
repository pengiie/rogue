use std::any::TypeId;

use hecs::{Query, QueryBorrow, QueryIter, With};
use rogue_macros::Resource;

use crate::{
    engine::{
        graphics::camera::{Camera, MainCamera},
        physics::transform::Transform,
        system::SystemParam,
        voxel::voxel::{VoxelModel, VoxelModelImpl},
    },
    game::entity::player::Player,
};

pub type Entity = hecs::Entity;

#[derive(Resource)]
pub struct ECSWorld {
    world: hecs::World,
}

impl Clone for ECSWorld {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl ECSWorld {
    pub fn new() -> ECSWorld {
        ECSWorld {
            world: hecs::World::new(),
        }
    }

    pub fn world_mut(&mut self) -> &mut hecs::World {
        &mut self.world
    }

    pub fn player_query<'a, Q: Query>(&'a self) -> PlayerQuery<Q> {
        PlayerQuery::new(
            self.query::<Q>().with::<&'a Player>() as QueryBorrow<'a, With<Q, &'a Player>>
        )
    }

    pub fn get_main_camera(
        &self,
        main_camera: &MainCamera,
    ) -> hecs::QueryOne<'_, (&Transform, &Camera)> {
        self.world
            .query_one::<(&Transform, &Camera)>(
                main_camera
                    .camera()
                    .expect("Main camera has not been set yet."),
            )
            .expect("Supplied main camera doesnt have a Transform and Camera component.")
    }

    pub fn try_get_main_camera(
        &self,
        main_camera: &MainCamera,
    ) -> Option<hecs::QueryOne<'_, (&Transform, &Camera)>> {
        let Some(camera_entity) = main_camera.camera() else {
            return None;
        };
        self.world
            .query_one::<(&Transform, &Camera)>(camera_entity)
            .ok()
    }
}

impl std::ops::Deref for ECSWorld {
    type Target = hecs::World;

    fn deref(&self) -> &Self::Target {
        &self.world
    }
}

impl std::ops::DerefMut for ECSWorld {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.world
    }
}

pub struct PlayerQuery<'a, Q: Query>(QueryBorrow<'a, With<Q, &'a Player>>);

impl<'a, Q: Query> PlayerQuery<'a, Q> {
    pub fn new(query: QueryBorrow<'a, With<Q, &'a Player>>) -> Self {
        Self(query)
    }

    pub fn player<'b>(&'b mut self) -> (Entity, Q::Item<'b>) {
        if self.0.iter().len() > 1 {
            panic!("More than one player spawned?");
        }
        self.0.iter().next().expect("Player was not spawned.")
    }

    pub fn try_player<'b>(&'b mut self) -> Option<(Entity, Q::Item<'b>)> {
        if self.0.iter().len() > 1 {
            panic!("More than one player spawned?");
        }
        self.0.iter().next()
    }
}
