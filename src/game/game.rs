use hecs::With;
use rogue_macros::Resource;

use crate::engine::{
    entity::{
        ecs_world::{self, ECSWorld},
        ScriptableEntity,
    },
    resource::ResMut,
};

use super::entity::GameEntity;

#[derive(Resource)]
pub struct GameSession;

impl GameSession {
    pub fn update_scripts(ecs_world: ResMut<ECSWorld>) {
        let mut scriptables_query = ecs_world.query::<With<&ScriptableEntity, &GameEntity>>();
        for (entity_id, (scriptable)) in scriptables_query.iter() {}
    }
}
