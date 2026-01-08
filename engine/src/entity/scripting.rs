use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use mlua::{Lua, ObjectLike};
use parking_lot::RwLock;
use rogue_macros::Resource;
use crate::asset::{
    asset::{AssetHandle, AssetPath, AssetStatus, Assets},
    repr::TextAsset,
};
use crate::resource::{Res, ResMut};
use crate::entity::GameEntity;
use crate::physics::transform::Transform;
use super::ecs_world::{ECSWorld, Entity};

#[derive(Clone)]
pub struct ScriptableEntity {
    pub scripts: Vec<AssetPath>,
}

enum ScriptEvent {
    LogMessage(/*message=*/ String),
}

impl ScriptableEntity {
    pub fn new() -> Self {
        Self {
            scripts: Vec::new(),
        }
    }
}

pub struct ScriptingWorldState {
    pub world: ECSWorld,
}

impl ScriptingWorldState {
    pub fn new() -> Self {
        Self {
            world: ECSWorld::new(),
        }
    }
}

#[derive(Resource)]
pub struct Scripts {
    lua: Lua,
    scripts: HashMap<AssetPath, String>,
    to_load_scripts: HashSet<AssetPath>,
    loading_scripts: HashMap<AssetPath, AssetHandle>,
    world_state: Arc<RwLock<ScriptingWorldState>>,
    script_events: Arc<RwLock<Vec<ScriptEvent>>>,
}

impl Scripts {
    pub fn new() -> Self {
        let script_events = Arc::new(RwLock::new(Vec::new()));
        let world_state = Arc::new(RwLock::new(ScriptingWorldState::new()));

        let lua = Lua::new();
        let script_events_ref = script_events.clone();
        lua.globals().set(
            "log_bar",
            lua.create_function(move |lua, message: String| {
                script_events_ref
                    .write()
                    .push(ScriptEvent::LogMessage(message));
                Ok(())
            })
            .unwrap(),
        );

        Self {
            lua,
            scripts: HashMap::new(),
            to_load_scripts: HashSet::new(),
            loading_scripts: HashMap::new(),
            world_state,
            script_events,
        }
    }

    pub fn refresh(&mut self) {
        self.to_load_scripts.extend(
            self.scripts
                .drain()
                .map(|(path, lua)| path)
                .collect::<Vec<_>>(),
        );
        self.to_load_scripts
            .extend(self.loading_scripts.drain().map(|(path, handle)| path));
    }

    pub fn update_world_state(mut scripts: ResMut<Scripts>, mut ecs_world: ResMut<ECSWorld>) {
        let mut world_state = scripts.world_state.write();
        let mut script_world = &mut world_state.world;
        //script_world.clear();

        //for (entity, (game_entity, scriptable, transform)) in ecs_world
        //    .query_mut::<(&GameEntity, &ScriptableEntity, &Transform)>()
        //    .into_iter()
        //{
        //    script_world.spawn_at(
        //        entity,
        //        (game_entity.clone(), scriptable.clone(), transform.clone()),
        //    );
        //}
    }

    pub fn update_loaded_scripts(
        mut scripts: ResMut<Scripts>,
        ecs_world: ResMut<ECSWorld>,
        mut assets: ResMut<Assets>,
    ) {
        let scripts: &mut Scripts = &mut scripts;
        for script_path in scripts.to_load_scripts.drain() {
            let handle = assets.load_asset::<TextAsset>(script_path.clone());
            scripts.loading_scripts.insert(script_path, handle);
        }

        let mut finished_loading_paths = Vec::new();
        for (path, handle) in scripts.loading_scripts.iter() {
            match assets.get_asset_status(handle) {
                AssetStatus::Loaded => {
                    let text = assets.take_asset::<TextAsset>(handle).unwrap();
                    scripts.scripts.insert(path.clone(), text.contents);
                    finished_loading_paths.push(path.clone());
                }
                AssetStatus::NotFound => {
                    log::error!("Tried loading script {:?} but it doesn't exist.", path)
                }
                AssetStatus::Error(error) => {
                    log::error!("Error loading script {:?}, {}", path, error)
                }
                _ => {}
            }
        }
        for path in finished_loading_paths.drain(..) {
            scripts.loading_scripts.remove(&path);
        }
    }

    pub fn try_load_world_scripts(&mut self, ecs_world: &mut ECSWorld) {
        for (entity_id, scriptable) in ecs_world.query::<&ScriptableEntity>().into_iter() {
            for asset_path in &scriptable.scripts {
                if self.scripts.contains_key(asset_path)
                    || self.to_load_scripts.contains(asset_path)
                    || self.loading_scripts.contains_key(asset_path)
                {
                    continue;
                }
                self.to_load_scripts.insert(asset_path.clone());
            }
        }
    }

    pub fn load_script(&mut self, asset_path: AssetPath) {
        self.to_load_scripts.insert(asset_path);
    }

    /// Alias for `are_scripts_loaded`.
    pub fn can_start_game(&self, ecs_world: &mut ECSWorld) -> bool {
        return self.are_scripts_loaded(ecs_world);
    }

    pub fn are_scripts_loaded(&self, ecs_world: &mut ECSWorld) -> bool {
        let mut scripted_entities = ecs_world.query::<&ScriptableEntity>();
        for (entity, scriptable) in scripted_entities.into_iter() {
            for asset_path in &scriptable.scripts {
                if !self.scripts.contains_key(asset_path) {
                    return false;
                }
            }
        }

        return true;
    }
}
