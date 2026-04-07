use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use rogue_engine::{
    animation::{
        animation::{AnimationRadians, AnimationTrackId},
        animation_bank::AnimationBank,
        animator::{Animator, AnimatorPlayingAnimation},
    },
    asset::asset::GameAssetPath,
    entity::{
        component::GameComponent,
        ecs_world::{ECSWorld, Entity},
    },
    event::{EventReader, Events},
    physics::transform::Transform,
    resource::{Res, ResMut, ResourceBank},
    window::time::Time,
};
use rogue_macros::Resource;

use crate::{
    editor_transform_euler::EditorTransformEuler,
    session::{EditorEvent, EditorSession},
};

#[derive(Resource)]
pub struct EditorAnimationPreviewer {
    pub enabled: bool,
    pub entity_target: Option<Entity>,
    pub selected_animation: Option<GameAssetPath>,
    pub playhead: Duration,
    pub last_playhead: Duration,
    pub is_playing: bool,
    pub editor_event_reader: EventReader<EditorEvent>,
    pub modified_channels: HashSet<(AnimationTrackId, String)>,
}

impl EditorAnimationPreviewer {
    pub fn new() -> Self {
        Self {
            enabled: false,
            entity_target: None,
            selected_animation: None,
            playhead: Duration::ZERO,
            last_playhead: Duration::ZERO,
            is_playing: false,
            editor_event_reader: EventReader::new(),
            modified_channels: HashSet::new(),
        }
    }

    pub fn mark_channel_modified(&mut self, track_id: AnimationTrackId, channel_name: String) {
        self.modified_channels.insert((track_id, channel_name));
    }

    pub fn update_animation_previewer(
        mut animation_previewer: ResMut<EditorAnimationPreviewer>,
        ecs_world: ResMut<ECSWorld>,
        editor_session: ResMut<EditorSession>,
        animation_bank: ResMut<AnimationBank>,
        time: Res<Time>,
        events: Res<Events>,
    ) {
        let animation_previewer = &mut *animation_previewer;
        for event in animation_previewer.editor_event_reader.read(&events) {
            match event {
                EditorEvent::SelectedEntity(Some(new_entity)) => {
                    if Some(new_entity) == animation_previewer.entity_target.as_ref() {
                        continue;
                    }
                    if !ecs_world.contains::<Animator>(*new_entity) {
                        continue;
                    }
                    animation_previewer.entity_target = Some(*new_entity);
                    animation_previewer.selected_animation = None;
                    animation_previewer.playhead = Duration::ZERO;
                    animation_previewer.is_playing = false;
                }
                _ => {}
            }
        }

        let Some(selected_entity) = animation_previewer.entity_target else {
            return;
        };
        let Some(selected_animation) = &animation_previewer.selected_animation else {
            return;
        };
        let Ok(mut animator) = ecs_world.get::<&mut Animator>(selected_entity) else {
            return;
        };

        let Some(selected_animation) = animation_bank.get_animation_by_path(selected_animation)
        else {
            return;
        };
        if selected_animation.duration.is_zero() {
            return;
        }

        if animation_previewer.is_playing {
            animation_previewer.modified_channels.clear();
            animation_previewer.playhead += time.delta_time();
            animation_previewer.playhead = Duration::from_secs_f32(
                animation_previewer.playhead.as_secs_f32()
                    % selected_animation.duration.as_secs_f32(),
            );
        }
        let playhead_changed = animation_previewer.playhead != animation_previewer.last_playhead;
        animation_previewer.last_playhead = animation_previewer.playhead;

        let animation_t = (animation_previewer.playhead.as_secs_f32()
            / selected_animation.duration.as_secs_f32())
        .clamp(0.0, 1.0);

        let should_animation_override = animation_previewer.is_playing || playhead_changed;
        if should_animation_override {
            selected_animation.apply_animation(&ecs_world, selected_entity, animation_t);

            // Update editor euler values.
            for track in &selected_animation.tracks {
                if !(track.track_id.component_name == <Transform as GameComponent>::NAME
                    && track.track_id.component_property == "rotation")
                {
                    continue;
                }
                let Some(track_entity) = track.track_id.get_entity(&ecs_world, selected_entity)
                else {
                    continue;
                };
                let Some((mut transform, mut editor_transform_euler)) = ecs_world
                    .query_one::<(&mut Transform, &mut EditorTransformEuler)>(track_entity)
                    .get()
                else {
                    continue;
                };
                for channel in &track.channels {
                    assert!(
                        channel.channel_type_info.type_info.type_id()
                            == std::any::TypeId::of::<AnimationRadians>()
                    );
                    let mut dst = AnimationRadians(0.0);
                    if let Some((start_index, end_index, t)) =
                        channel.find_interpolation_indices(Duration::from_secs_f32(
                            animation_t * selected_animation.duration.as_secs_f32(),
                        ))
                    {
                        let mut dst_ptr = &mut dst as *mut AnimationRadians as *mut u8;
                        let a_ptr = channel.values.get_unchecked(start_index).as_ptr() as *const u8;
                        let b_ptr = channel.values.get_unchecked(end_index).as_ptr() as *const u8;
                        // Safety: Each value is allocated with the same channel type info.
                        unsafe {
                            channel
                                .channel_type_info
                                .fn_caller
                                .update_erased(dst_ptr, a_ptr, b_ptr, t);
                        }
                    }
                    let new_rot = dst.0;
                    let mut new_euler = editor_transform_euler.euler();
                    // Update the euler angles for the editor transform per the entity interpolation.
                    match channel.channel_type_info.channel_name.as_str() {
                        "pitch" => {
                            new_euler.x = new_rot;
                        }
                        "yaw" => {
                            new_euler.y = new_rot;
                        }
                        "roll" => {
                            new_euler.z = new_rot;
                        }
                        _ => panic!("No other properties should exist for transform rotation"),
                    };
                    let new_quat = editor_transform_euler.set_euler(new_euler);
                    transform.rotation = new_quat;
                }
            }
        }
    }
}
