use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use rogue_macros::game_component;

use crate::{
    animation::{
        animation::Animation,
        animation_bank::{AnimationBank, AnimationId},
    },
    asset::asset::{AssetHandle, Assets, GameAssetPath},
    entity::ecs_world::ECSWorld,
    resource::{Res, ResMut},
    window::time::{Instant, Time},
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "Animator")]
#[serde(default)]
pub struct Animator {
    pub animations: HashSet<GameAssetPath>,
    #[serde(skip)]
    animation_handles: HashMap<GameAssetPath, AnimationId>,
    #[serde(skip)]
    pub playing_animations: HashMap<GameAssetPath, AnimatorPlayingAnimation>,
    #[serde(skip)]
    to_play_animations: HashMap<GameAssetPath, AnimatorPlayAnimationInfo>,
}

impl Animator {
    pub fn new() -> Self {
        Self {
            animations: HashSet::new(),
            animation_handles: HashMap::new(),
            playing_animations: HashMap::new(),
            to_play_animations: HashMap::new(),
        }
    }

    pub fn play_animation(
        &mut self,
        animation: &GameAssetPath,
        playback_info: AnimatorPlayAnimationInfo,
    ) {
        self.to_play_animations
            .insert(animation.clone(), playback_info);
    }

    pub fn update_animators_system(
        ecs_world: ResMut<ECSWorld>,
        time: Res<Time>,
        mut animation_bank: ResMut<AnimationBank>,
    ) {
        for (entity, animator) in ecs_world.query::<&mut Animator>().into_iter() {
            // Ensure animations are loaded before we play them.
            for animation_path in &animator.animations {
                if animation_bank
                    .get_animation_by_path(animation_path)
                    .is_none()
                {
                    animation_bank.request_animation(animation_path);
                }
            }

            let mut to_end_animations = Vec::new();
            for (animation_path, playing_animation) in &mut animator.playing_animations {
                let Some(animation) = animation_bank.get_animation_by_path(animation_path) else {
                    continue;
                };
                if !playing_animation.update_playback_playhead(animation, time.delta_time()) {
                    to_end_animations.push(animation_path.clone());
                    continue;
                }
                let t = playing_animation.animation_t();
                animation.apply_animation(&ecs_world, entity, t);
            }

            for animation_path in to_end_animations {
                animator.playing_animations.remove(&animation_path);
            }
        }
    }

    pub fn stop_all_animations(&mut self) {
        self.playing_animations.clear();
        self.to_play_animations.clear();
    }
}

impl Default for Animator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct AnimatorPlayAnimationInfo {
    pub repeat: bool,
    pub speed: f32,
}

#[derive(Clone)]
pub struct AnimatorPlayingAnimation {
    pub info: AnimatorPlayAnimationInfo,
    pub time_secs: f32,
    duration_secs: f32,
}

impl AnimatorPlayingAnimation {
    pub fn new(animation: &Animation, info: AnimatorPlayAnimationInfo) -> Self {
        Self {
            info,
            time_secs: 0.0,
            duration_secs: animation.duration.as_secs_f32(),
        }
    }

    pub fn animation_t(&self) -> f32 {
        return self.time_secs / self.duration_secs;
    }

    // Returns true if playback is over.
    pub fn update_playback_playhead(
        &mut self,
        animation: &Animation,
        delta_time: Duration,
    ) -> bool {
        self.time_secs += delta_time.as_secs_f32() * self.info.speed;
        if self.info.repeat {
            self.time_secs = self.time_secs % self.duration_secs;
            return false;
        }
        return self.time_secs > animation.duration.as_secs_f32();
    }
}
