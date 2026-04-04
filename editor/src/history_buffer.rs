use std::collections::VecDeque;

use rogue_engine::{
    asset::repr::game_entity::{WorldGameComponentAsset, WorldGameEntityAsset},
    entity::ecs_world::{ECSWorld, Entity},
    input::{
        Input,
        keyboard::{Key, Modifier},
    },
    resource::{Res, ResMut},
};
use rogue_macros::Resource;

use crate::editing::voxel_editing::EditorVoxelEditing;

pub enum EditorHistoryAction {
    SpawnEntity {
        entity: Entity,
        post_spawn_asset: WorldGameEntityAsset,
    },
    ModifyComponent {
        entity: Entity,
        pre_modified_component: WorldGameComponentAsset,
        post_modified_component: WorldGameComponentAsset,
    },
}

impl EditorHistoryAction {
    pub fn undo(&self, ecs_world: &mut ECSWorld) {
        match self {
            EditorHistoryAction::SpawnEntity {
                entity,
                post_spawn_asset,
            } => {
                ecs_world.despawn(*entity, true);
            }
            EditorHistoryAction::ModifyComponent {
                entity,
                pre_modified_component,
                post_modified_component,
            } => {
                //ecs_world.insert_one_asset(entity, pre_modified_component);
            }
        }
    }
}

#[derive(Resource)]
pub struct EditorHistoryBuffer {
    buffer: VecDeque<EditorHistoryAction>,
}

impl EditorHistoryBuffer {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
        }
    }

    pub fn push_action(action: EditorHistoryAction) {}

    pub fn update_undo(
        history: ResMut<EditorHistoryBuffer>,
        input: Res<Input>,
        voxel_editing: Res<EditorVoxelEditing>,
    ) {
        if voxel_editing.enabled {
            return;
        }
        if !input.is_key_pressed_with_modifiers(Key::Z, &[Modifier::Control]) {
            return;
        }
    }
}
