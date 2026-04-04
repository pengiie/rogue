use std::time::Duration;

use rogue_engine::{
    animation::{
        animation_bank::AnimationBank,
        animator::{Animator, AnimatorPlayingAnimation},
    },
    asset::asset::GameAssetPath,
    entity::ecs_world::ECSWorld,
    resource::{Res, ResMut, ResourceBank},
    window::time::Time,
};
use rogue_macros::Resource;

use crate::session::EditorSession;

#[derive(Resource)]
pub struct EditorAnimationPreviewer {
    pub enabled: bool,
    pub selected_animation: Option<GameAssetPath>,
    pub playhead: Duration,
    pub is_playing: bool,
}

impl EditorAnimationPreviewer {
    pub fn new() -> Self {
        Self {
            enabled: false,
            selected_animation: None,
            playhead: Duration::ZERO,
            is_playing: false,
        }
    }
    pub fn update_animation_previewer(
        mut animation_previewer: ResMut<EditorAnimationPreviewer>,
        ecs_world: ResMut<ECSWorld>,
        editor_session: ResMut<EditorSession>,
        animation_bank: ResMut<AnimationBank>,
        time: Res<Time>,
    ) {
        let Some(selected_entity) = editor_session.selected_entity else {
            return;
        };
        let Some(selected_animation) = &animation_previewer.selected_animation else {
            return;
        };
        let Ok(mut animator) = ecs_world.get::<&mut Animator>(selected_entity) else {
            return;
        };

        if animation_previewer.is_playing {
            let selected_animation = animation_bank
                .get_animation_by_path(selected_animation)
                .expect("Should exist if playing and it is selected.");

            animation_previewer.playhead += time.delta_time();
            animation_previewer.playhead = Duration::from_secs_f32(
                animation_previewer.playhead.as_secs_f32()
                    % selected_animation.duration.as_secs_f32(),
            );
            let animation_t = (animation_previewer.playhead.as_secs_f32()
                / selected_animation.duration.as_secs_f32());
            assert!(animation_t >= 0.0 && animation_t <= 1.0);
            selected_animation.apply_animation(&ecs_world, selected_entity, animation_t);
        }
    }
}
