use std::{any::TypeId, collections::HashSet, ops::Deref};

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

use super::{
    scripting::ScriptableEntity, EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
};

pub type Entity = hecs::Entity;

#[derive(Resource)]
pub struct ECSWorld {
    pub world: hecs::World,
}

impl ECSWorld {
    pub fn new() -> ECSWorld {
        ECSWorld {
            world: hecs::World::new(),
        }
    }

    pub fn clone_game_entities(&mut self) -> ECSWorld {
        let mut new = ECSWorld::new();
        for (entity, (game_entity, transform, parent, children, renderable, camera, scriptable)) in
            self.query_mut::<(
                &GameEntity,
                &Transform,
                Option<&EntityParent>,
                Option<&EntityChildren>,
                Option<&RenderableVoxelEntity>,
                Option<&Camera>,
                Option<&ScriptableEntity>,
            )>()
        {
            // Must use spawn_at so EntityParent and EntityChildren stay correct.
            new.spawn_at(entity, (game_entity.clone(), transform.clone()));
            if let Some(parent) = parent {
                new.insert_one(entity, parent.clone());
            }
            if let Some(children) = children {
                new.insert_one(entity, children.clone());
            }
            if let Some(renderable) = renderable {
                new.insert_one(entity, renderable.clone());
            }
            if let Some(camera) = camera {
                new.insert_one(entity, camera.clone());
            }
            if let Some(scriptable) = scriptable {
                new.insert_one(entity, scriptable.clone());
            }
        }

        return new;
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

    pub fn set_parent(&mut self, entity: Entity, parent: Entity) {
        self.world.insert_one(entity, EntityParent::new(parent));
        let contains_children = self.world.get::<&mut EntityChildren>(parent).is_ok();
        if contains_children {
            let mut children = self.world.get::<&mut EntityChildren>(parent).unwrap();
            children.children.insert(entity);
        } else {
            let mut children = HashSet::new();
            children.insert(entity);
            self.world.insert_one(parent, EntityChildren { children });
        }
    }

    pub fn get_world_transform(
        &self,
        entity: Entity,
        entity_local_transform: &Transform,
    ) -> Transform {
        let mut curr_transform = entity_local_transform.clone();

        let mut curr_parent = self.world.get::<&EntityParent>(entity);
        while let Ok(parent) = curr_parent {
            let Ok(parent_transform) = self.world.get::<&Transform>(parent.parent) else {
                break;
            };
            curr_transform.position =
                parent_transform.rotation * curr_transform.position + parent_transform.position;
            curr_transform.rotation = parent_transform.rotation * curr_transform.rotation;
            curr_transform.scale = curr_transform.scale.component_mul(&parent_transform.scale);
            curr_parent = self.world.get::<&EntityParent>(parent.parent);
        }

        return curr_transform;
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
