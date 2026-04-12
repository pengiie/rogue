use crate::EditorTransformEuler;
use rogue_engine::physics::transform::Transform;
use rogue_engine::{animation::animation::AnimationRadians, entity::component::GameComponent};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use rogue_engine::{
    animation::{
        animation::{Animation, AnimationTrackId},
        animator::Animator,
    },
    asset::asset::GameAssetPath,
    entity::{
        GameEntity,
        ecs_world::{ECSWorld, Entity},
    },
};

use crate::{
    animation_preview::EditorAnimationPreviewer,
    session::EditorCommandEvent,
    ui::{
        EditorCommand, EditorUIContext, FilePickerType,
        asset_properties_pane::AssetPropertiesPane,
        create_animation_track_dialog::{self, CreateAnimationTrackDialogCreateInfo},
        pane::EditorUIPane,
    },
};

#[derive(PartialEq, Eq, Hash, Clone)]
struct AnimationKeyframeId {
    track_id: AnimationTrackId,
    channel_name: String,
    // Use time instead of an index since the index isn't stable.
    keyframe_time: Duration,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default = "AnimationPane::new")]
pub struct AnimationPane {
    #[serde(skip)]
    selected_keyframes: HashSet<AnimationKeyframeId>,
    last_animation: Option<GameAssetPath>,
}

struct RenderedTrack {
    entity_traversal: Vec<String>,
    component_name: Option<String>,
    property_name: Option<String>,
    channel_name: Option<String>,
}

impl AnimationPane {
    pub fn new() -> Self {
        Self {
            selected_keyframes: HashSet::new(),
            last_animation: None,
        }
    }

    pub fn show_header(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Animation").size(20.0));

            let Some(selected_entity) = &ctx.animation_preview.entity_target else {
                // Error is shown in pane show fn.
                return;
            };
            let Ok(animator) = ctx.ecs_world.get::<&mut Animator>(*selected_entity) else {
                // Error is shown in pane show fn.
                return;
            };

            ui.horizontal(|ui| {
                let entity_name = &ctx
                    .ecs_world
                    .get::<&GameEntity>(*selected_entity)
                    .unwrap()
                    .name;
                ui.label(entity_name);
            });
            ui.separator();

            let selected_animation = &mut ctx.animation_preview.selected_animation;
            ui.horizontal(|ui| {
                ui.label("Animation:");
                let animation_name = selected_animation
                    .as_ref()
                    .map(|animation| animation.as_relative_path_str())
                    .unwrap_or_else(|| "None".to_owned());
                ui.menu_button(animation_name, |ui| {
                    if ui.button("Create new").clicked() {
                        let selected_entity = *selected_entity;
                        ctx.commands.push(EditorCommand::FilePicker {
                            picker_type: FilePickerType::CreateFile,
                            callback: Box::new(move |ctx, asset_path| {
                                let asset_path = GameAssetPath::from_relative_path(&asset_path);
                                let Ok(mut animator) =
                                    ctx.ecs_world.get::<&mut Animator>(selected_entity)
                                else {
                                    return;
                                };
                                animator.animations.insert(asset_path.clone());
                                ctx.animation_bank
                                    .insert_animation(asset_path.clone(), Animation::new());
                                ctx.animation_preview.selected_animation = Some(asset_path);
                            }),
                            extensions: vec!["ranim".to_owned()],
                            preset_file_path: None,
                        });
                        ui.close_menu();
                    }

                    if ui
                        .add_enabled(selected_animation.is_some(), egui::Button::new("Save"))
                        .clicked()
                    {
                        let selected_entity = *selected_entity;
                        ctx.events.push(EditorCommandEvent::SaveAnimation(
                            selected_animation.clone().unwrap(),
                        ));
                        ui.close_menu();
                    }

                    for animation in &animator.animations {
                        if ui.button(animation.as_relative_path_str()).clicked() {
                            *selected_animation = Some(animation.clone());
                            ui.close_menu();
                        }
                    }
                })
            });

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !ctx.animation_preview.is_playing,
                        egui::Button::new("\u{25B6}"),
                    )
                    .clicked()
                {
                    ctx.animation_preview.is_playing = true;
                }
                if ui
                    .add_enabled(
                        ctx.animation_preview.is_playing,
                        egui::Button::new("\u{25A0}"),
                    )
                    .clicked()
                {
                    ctx.animation_preview.is_playing = false;
                }
            });
        });
    }

    const TRACK_BG_FILL_1: &str = "#222222";
    const TRACK_BG_FILL_2: &str = "#111111";
    const TRACK_HEIGHT: f32 = 20.0;
    fn show_tracks(
        ui: &mut egui::Ui,
        ctx: &mut EditorUIContext<'_>,
        rendered_tracks: &mut Vec<RenderedTrack>,
    ) {
        let Some(selected_entity) = ctx.animation_preview.entity_target else {
            return;
        };
        let Some(selected_animation) =
            ctx.animation_preview
                .selected_animation
                .as_ref()
                .and_then(|animation_path| {
                    ctx.animation_bank.get_animation_by_path_mut(animation_path)
                })
        else {
            return;
        };
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Tracks");
                if ui.button("Add").clicked() {
                    ctx.commands.push(
                        create_animation_track_dialog::create_animation_track_dialog(
                            CreateAnimationTrackDialogCreateInfo {
                                target_entity: selected_entity,
                                target_animation: ctx
                                    .animation_preview
                                    .selected_animation
                                    .clone()
                                    .unwrap(),
                            },
                        ),
                    );
                }
            });
            let mut create_keyframes = Vec::new();
            Self::show_track_entity_props(
                ui,
                ctx.ecs_world,
                ctx.animation_preview,
                selected_animation,
                selected_entity,
                &mut Vec::new(),
                None,
                None,
                0,
                &mut create_keyframes,
                rendered_tracks,
            );
            for (track_id, channel_names) in &create_keyframes {
                let track = selected_animation
                    .get_track_mut(&track_id)
                    .expect("Track should exist when creating keyframe.");
                let channels = track.channels.iter_mut().filter(|channel| {
                    channel_names.contains(&channel.channel_type_info.channel_name)
                });
                for channel in channels {
                    channel.remove_nearby_keyframes(
                        ctx.animation_preview.playhead,
                        Duration::from_secs_f32(Self::KEYFRAME_SNAP_THRESHOLD_SECS),
                    );

                    let new_keyframe_time = ctx.animation_preview.playhead;
                    let track_entity = track_id
                        .get_entity(ctx.ecs_world, selected_entity)
                        .expect("Track entity should exist when creating keyframe.");
                    if track_id.component_name == <Transform as GameComponent>::NAME
                        && &track_id.component_property == "rotation"
                        && let Some((mut transform, mut editor_transform_euler)) = ctx
                            .ecs_world
                            .query_one::<(&mut Transform, &mut EditorTransformEuler)>(track_entity)
                            .get()
                    {
                        let euler_angle = match channel.channel_type_info.channel_name.as_str() {
                            "pitch" => editor_transform_euler.euler().x,
                            "yaw" => editor_transform_euler.euler().y,
                            "roll" => editor_transform_euler.euler().z,
                            _ => panic!("No other properties should exist for transform rotation"),
                        };
                        channel.record_keyframe_euler(
                            new_keyframe_time,
                            AnimationRadians(euler_angle),
                        );
                    } else {
                        channel.record_keyframe(ctx.ecs_world, selected_entity, new_keyframe_time);
                    }
                }
            }
            if !create_keyframes.is_empty() {
                selected_animation.update_duration();
            }
        });
    }

    fn show_track_entity_props(
        ui: &mut egui::Ui,
        ecs_world: &ECSWorld,
        animation_preview: &mut EditorAnimationPreviewer,
        animation: &Animation,
        base_entity: Entity,
        entity_traversal: &mut Vec<String>,
        component_name: Option<String>,
        property_name: Option<String>,
        indentation: usize,
        create_keyframes: &mut Vec<(AnimationTrackId, /*channels*/ Vec<String>)>,
        rendered_tracks: &mut Vec<RenderedTrack>,
    ) {
        const INDENT_PER_LEVEL: f32 = 4.0;
        let indent_space = indentation as f32 * INDENT_PER_LEVEL;
        if let Some(property_name) = &property_name {
            let track_id = AnimationTrackId {
                entity_traversal: entity_traversal.clone(),
                component_name: component_name.clone().unwrap(),
                component_property: property_name.clone(),
            };
            let track = animation.get_track(&track_id).unwrap();

            ui.horizontal(|ui| {
                rendered_tracks.push(RenderedTrack {
                    entity_traversal: entity_traversal.clone(),
                    component_name: component_name.clone(),
                    property_name: Some(property_name.clone()),
                    channel_name: None,
                });
                ui.add_space(indent_space);
                ui.set_max_height(Self::TRACK_HEIGHT);
                ui.label(property_name);
                if ui.small_button("O").clicked() {
                    let channel_names = track
                        .channels
                        .iter()
                        .map(|channel| channel.channel_type_info.channel_name.clone())
                        .collect();
                    create_keyframes.push((track_id.clone(), channel_names));
                }
            });

            ui.vertical(|ui| {
                for channel in &track.channels {
                    let channel_name = &channel.channel_type_info.channel_name;
                    ui.horizontal(|ui| {
                        rendered_tracks.push(RenderedTrack {
                            entity_traversal: entity_traversal.clone(),
                            component_name: component_name.clone(),
                            property_name: Some(property_name.clone()),
                            channel_name: Some(channel_name.clone()),
                        });
                        ui.add_space(indent_space + INDENT_PER_LEVEL);
                        ui.set_max_height(Self::TRACK_HEIGHT);
                        ui.label(channel_name);

                        // Special case for transform rotations since want to track the euler angle
                        // state for proper angles.
                        let Some(track_entity) = track_id.get_entity(ecs_world, base_entity) else {
                            ui.label("(NOT FOUND)");
                            return;
                        };
                        if track_id.component_name == <Transform as GameComponent>::NAME
                            && &track_id.component_property == "rotation"
                            && let Some((mut transform, mut editor_transform_euler)) = ecs_world
                                .query_one::<(&mut Transform, &mut EditorTransformEuler)>(
                                    track_entity,
                                )
                                .get()
                        {
                            let curr_euler = editor_transform_euler.euler();
                            let mut prev_rot_degrees = match channel_name.as_str() {
                                "pitch" => curr_euler.x,
                                "yaw" => curr_euler.y,
                                "roll" => curr_euler.z,
                                _ => panic!(
                                    "No other properties should exist for transform rotation"
                                ),
                            }
                            .to_degrees();
                            let mut new_rot_degrees = prev_rot_degrees;
                            let res =
                                ui.add(egui::DragValue::new(&mut new_rot_degrees).suffix("°"));
                            if res.changed() {
                                let new_rot = new_rot_degrees.to_radians();
                                let mut new_euler = editor_transform_euler.euler();
                                match channel_name.as_str() {
                                    "pitch" => {
                                        new_euler.x = new_rot;
                                    }
                                    "yaw" => {
                                        new_euler.y = new_rot;
                                    }
                                    "roll" => {
                                        new_euler.z = new_rot;
                                    }
                                    _ => panic!(
                                        "No other properties should exist for transform rotation"
                                    ),
                                }
                                let new_quat = editor_transform_euler.set_euler(new_euler);
                                transform.rotation = new_quat;
                            }
                        } else {
                            channel.show_ui(ui, base_entity, ecs_world);
                        }

                        if ui.small_button("O").clicked() {
                            create_keyframes.push((track_id.clone(), vec![channel_name.clone()]));
                        }
                    });
                }
            });
            return;
        }

        ui.vertical(|ui| {
            ui.style_mut().spacing.item_spacing.y = 0.0;
            let mut visited_track_entities = HashSet::new();
            for track in &animation.tracks {
                if !track.track_id.matches_prefix(
                    entity_traversal,
                    component_name.as_ref(),
                    property_name.as_ref(),
                ) {
                    continue;
                }

                if let Some(component_name) = &component_name {
                    assert!(&track.track_id.component_name == component_name);
                    assert!(&track.track_id.entity_traversal == entity_traversal);
                    // Don't indent since property name isn't rendered yet.
                    Self::show_track_entity_props(
                        ui,
                        ecs_world,
                        animation_preview,
                        animation,
                        base_entity,
                        entity_traversal,
                        Some(component_name.clone()),
                        Some(track.track_id.component_property.clone()),
                        indentation,
                        create_keyframes,
                        rendered_tracks,
                    );
                } else if track.track_id.entity_traversal.len() == entity_traversal.len() {
                    ui.horizontal(|ui| {
                        rendered_tracks.push(RenderedTrack {
                            entity_traversal: entity_traversal.clone(),
                            component_name: Some(track.track_id.component_name.clone()),
                            property_name: None,
                            channel_name: None,
                        });
                        ui.add_space(indent_space);
                        ui.set_max_height(Self::TRACK_HEIGHT);
                        ui.label(&track.track_id.component_name);
                    });
                    Self::show_track_entity_props(
                        ui,
                        ecs_world,
                        animation_preview,
                        animation,
                        base_entity,
                        entity_traversal,
                        Some(track.track_id.component_name.clone()),
                        None,
                        indentation + 1,
                        create_keyframes,
                        rendered_tracks,
                    );
                } else {
                    let next_entity_name = &track.track_id.entity_traversal[entity_traversal.len()];
                    if visited_track_entities.contains(next_entity_name) {
                        continue;
                    }
                    visited_track_entities.insert(next_entity_name.clone());
                    ui.horizontal(|ui| {
                        rendered_tracks.push(RenderedTrack {
                            entity_traversal: entity_traversal.clone(),
                            component_name: None,
                            property_name: None,
                            channel_name: None,
                        });
                        ui.add_space(indent_space);
                        ui.set_max_height(Self::TRACK_HEIGHT);
                        ui.label(next_entity_name);
                    });
                    entity_traversal.push(next_entity_name.clone());
                    Self::show_track_entity_props(
                        ui,
                        ecs_world,
                        animation_preview,
                        animation,
                        base_entity,
                        entity_traversal,
                        None,
                        None,
                        indentation + 1,
                        create_keyframes,
                        rendered_tracks,
                    );
                    entity_traversal.pop();
                }
            }
        });
    }

    const TIME_AXIS_HEIGHT: f32 = 24.0;
    const KEYFRAME_SNAP_THRESHOLD_SECS: f32 = 0.05;
    fn show_timeline(
        ui: &mut egui::Ui,
        ctx: &mut EditorUIContext<'_>,
        rendered_tracks: &Vec<RenderedTrack>,
    ) {
        let Some(selected_entity) = &ctx.animation_preview.entity_target else {
            return;
        };
        let Some(selected_animation) = ctx
            .animation_preview
            .selected_animation
            .as_ref()
            .and_then(|animation_path| ctx.animation_bank.get_animation_by_path(animation_path))
        else {
            return;
        };

        let timeline_width = ui.available_width();
        let timeline_height = ui.available_height();

        let start_time = 0.0;
        let timeline_duration_id = ui.id().with("timeline_duration");
        let mut timeline_duration = ui.data_mut(|w| {
            *w.get_temp_mut_or_insert_with(timeline_duration_id, || {
                if selected_animation.duration.as_secs_f32() > 0.0 {
                    selected_animation.duration.as_secs_f32()
                } else {
                    5.0
                }
            })
        });
        let scroll_rect =
            egui::Rect::from_min_size(ui.cursor().min, egui::vec2(timeline_width, timeline_height));
        let scroll_id = ui.id().with("timeline_scroll");
        let scroll_response = ui.interact(scroll_rect, scroll_id, egui::Sense::hover());
        if scroll_response.hovered() {
            //log::info!("Scroll delta: {:?}", ui.input(|r| r.smooth_scroll_delta));
            //log::info!(
            //    "Modifiers: ctrl={}, shift={}, alt={}",
            //    ui.input(|r| r.modifiers.ctrl),
            //    ui.input(|r| r.modifiers.shift),
            //    ui.input(|r| r.modifiers.alt)
            //);
            const TIMELINE_ZOOM_SENSITIVITY: f32 = 0.1;
            let scroll_delta = ui.input(|r| {
                if r.modifiers.ctrl {
                    r.smooth_scroll_delta.y
                } else {
                    0.0
                }
            });
            if scroll_delta != 0.0 {
                timeline_duration *= 1.0 + scroll_delta * TIMELINE_ZOOM_SENSITIVITY;
                timeline_duration = timeline_duration.clamp(0.5, 50.0);
                ui.data_mut(|w| {
                    w.insert_temp(timeline_duration_id, timeline_duration);
                });
            }
        }
        let end_time = start_time + timeline_duration;

        // Draw time axis.
        let time_axis_rect = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(timeline_width, Self::TIME_AXIS_HEIGHT),
        );
        ui.painter().rect_filled(
            time_axis_rect,
            2.0,
            egui::Color32::from_hex("#232323").unwrap(),
        );
        const INCREMENTS: usize = 10;
        const TICK_FONT_SIZE: f32 = 10.0;
        const TICK_HEIGHT: f32 = 4.0;
        const TICK_FONT_SPACING: f32 = 1.0;
        let ticks_times = (0..=INCREMENTS)
            .map(|i| start_time + (i as f32 / INCREMENTS as f32) * timeline_duration);
        for i_u in 0..=INCREMENTS {
            let i = i_u as f32;
            let mut x = (i / INCREMENTS as f32) * timeline_width + time_axis_rect.min.x;
            ui.painter().line_segment(
                [
                    egui::pos2(x, time_axis_rect.max.y - TICK_HEIGHT),
                    egui::pos2(x, time_axis_rect.max.y),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_hex("#3A3A3A").unwrap()),
            );
            let tick_time = start_time + (i / INCREMENTS as f32) * timeline_duration;
            let text_alignment = if i_u == 0 {
                egui::Align2::LEFT_BOTTOM
            } else if i_u == INCREMENTS {
                egui::Align2::RIGHT_BOTTOM
            } else {
                egui::Align2::CENTER_BOTTOM
            };

            let mut tick_text = format!("{:.2}", tick_time)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned();
            if tick_time != 0.0 {
                tick_text = tick_text.trim_start_matches('0').to_owned();
            }
            ui.painter().text(
                egui::pos2(x, time_axis_rect.max.y - TICK_HEIGHT - TICK_FONT_SPACING),
                text_alignment,
                tick_text,
                egui::FontId::monospace(TICK_FONT_SIZE),
                egui::Color32::WHITE,
            );
        }

        // Interact with time axis.
        let time_axis_interact_rect = time_axis_rect;
        let time_axis_res = ui.interact(
            time_axis_interact_rect,
            ui.id().with("time_axis"),
            egui::Sense::click_and_drag(),
        );
        if time_axis_res.is_pointer_button_down_on() {
            let mouse_pos = time_axis_res.interact_pointer_pos().unwrap();
            let mouse_axis_x = mouse_pos.x - time_axis_rect.min.x;
            let mouse_t = (mouse_axis_x / timeline_width).clamp(0.0, 1.0);
            let mut mouse_time = start_time + mouse_t * timeline_duration;
            let original_mouse_time = mouse_time;
            let closest_tick_time = ticks_times
                .min_by_key(|tick_time| ((*tick_time - mouse_time).abs() * 1000.0) as u32);
            const TICK_SNAP_THRESHOLD: f32 = 0.02;
            let closest_keyframe_time = selected_animation
                .tracks
                .iter()
                .flat_map(|track| track.channels.iter())
                .flat_map(|channel| channel.times.iter())
                .min_by_key(|keyframe_time| {
                    ((keyframe_time.as_secs_f32() - mouse_time) * 1000.0) as u32
                });
            if let Some(closest_tick_time) = closest_tick_time {
                if (closest_tick_time - mouse_time).abs() < TICK_SNAP_THRESHOLD {
                    mouse_time = closest_tick_time;
                }
            }
            if let Some(closest_keyframe_time) = closest_keyframe_time {
                if (closest_keyframe_time.as_secs_f32() - mouse_time).abs()
                    < Self::KEYFRAME_SNAP_THRESHOLD_SECS
                {
                    mouse_time = closest_keyframe_time.as_secs_f32();
                }
            }
            ctx.animation_preview.playhead = Duration::from_secs_f32(mouse_time);
        }

        let timeline_rect = egui::Rect::from_min_size(
            time_axis_rect.min + egui::vec2(0.0, Self::TIME_AXIS_HEIGHT),
            egui::vec2(timeline_width, ui.available_height()),
        );

        // Draw keyframes.
        for (
            i,
            RenderedTrack {
                entity_traversal,
                component_name,
                property_name,
                channel_name,
            },
        ) in rendered_tracks.iter().enumerate()
        {
            let color = if i % 2 == 0 {
                egui::Color32::from_hex(Self::TRACK_BG_FILL_1).unwrap()
            } else {
                egui::Color32::from_hex(Self::TRACK_BG_FILL_2).unwrap()
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(
                        timeline_rect.min.x,
                        timeline_rect.min.y + i as f32 * Self::TRACK_HEIGHT,
                    ),
                    egui::vec2(timeline_width, Self::TRACK_HEIGHT),
                ),
                0.0,
                color,
            );

            let matching_tracks = selected_animation.tracks.iter().filter(|track| {
                track.track_id.matches_prefix(
                    entity_traversal,
                    component_name.as_ref(),
                    property_name.as_ref(),
                )
            });
            for track in matching_tracks {
                let matching_channels = track.channels.iter().filter(|channel| {
                    channel_name
                        .as_ref()
                        .map(|cn| &channel.channel_type_info.channel_name == cn)
                        .unwrap_or(true)
                });
                for channel in matching_channels {
                    for keyframe_time in &channel.times {
                        let keyframe_time = keyframe_time.as_secs_f32();
                        let keyframe_t = (keyframe_time - start_time) / timeline_duration;
                        if keyframe_t < 0.0 || keyframe_t > 1.0 {
                            continue;
                        }
                        let keyframe_x = keyframe_t * timeline_width + timeline_rect.min.x;
                        let keyframe_y = timeline_rect.min.y + i as f32 * Self::TRACK_HEIGHT;
                        ui.painter().circle_filled(
                            egui::pos2(keyframe_x, keyframe_y + Self::TRACK_HEIGHT * 0.5),
                            4.0,
                            egui::Color32::from_hex("#AA1111").unwrap(),
                        );
                    }
                }
            }
        }

        // Gray out area after animation duration.
        let animation_end_x = ((selected_animation.duration.as_secs_f32() - start_time)
            / timeline_duration)
            .clamp(0.0, 1.0)
            * timeline_width
            + time_axis_rect.min.x;
        if animation_end_x < timeline_rect.max.x && selected_animation.tracks.len() > 0 {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(animation_end_x, timeline_rect.min.y),
                    timeline_rect.max,
                ),
                0.0,
                egui::Color32::from_hex("#000000")
                    .unwrap()
                    .linear_multiply(0.5),
            );
        }

        // Draw playhead.
        let playhead_position = ctx
            .animation_preview
            .playhead
            .as_secs_f32()
            .clamp(start_time, end_time);
        let playhead_x = ((playhead_position - start_time) / timeline_duration) * timeline_width
            + time_axis_rect.min.x;
        let playhead_render_rect = egui::Rect::from_min_size(
            egui::pos2(playhead_x, time_axis_rect.min.y),
            egui::vec2(2.0, timeline_height),
        );
        ui.painter()
            .rect_filled(playhead_render_rect, 0.0, egui::Color32::WHITE);
    }
}

impl EditorUIPane for AnimationPane {
    const ID: &'static str = "animation";
    const NAME: &'static str = "Animation";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut crate::ui::EditorUIContext<'_>) {
        Self::show_header(ui, ctx);

        let Some(selected_entity) = ctx.animation_preview.entity_target else {
            ui.label("No entity selected");
            return;
        };
        if ctx.ecs_world.get::<&mut Animator>(selected_entity).is_err() {
            ui.label("Selected entity doesn't have an Animator component.");
            return;
        };
        if ctx
            .animation_preview
            .selected_animation
            .as_ref()
            .and_then(|animation_path| ctx.animation_bank.get_animation_by_path(animation_path))
            .is_none()
        {
            // Selected animation is shown as None in header.
            return;
        };

        // Clear last animation ui state.
        if &ctx.animation_preview.selected_animation != &self.last_animation {
            self.selected_keyframes.clear();
            self.last_animation = ctx.animation_preview.selected_animation.clone();
        }

        ui.horizontal(|ui| {
            let mut track_y_offset = 0.0;
            let mut rendered_tracks = Vec::new();

            let ideal_tracks_width_id = ui.id().with("ideal_track_width");
            let ideal_tracks_width =
                ui.data_mut(|w| *w.get_temp_mut_or_insert_with(ideal_tracks_width_id, || 0.0f32));
            let start_width = ui.available_width();
            ui.vertical(|ui| {
                if ideal_tracks_width > 0.0 {
                    ui.set_width(ideal_tracks_width);
                }
                Self::show_tracks(ui, ctx, &mut rendered_tracks);
            });
            let tracks_width = start_width - ui.available_width();
            ui.data_mut(|w| {
                let last_ideal_tracks_width = w.get_temp::<f32>(ideal_tracks_width_id).unwrap();
                if tracks_width - 25.0 > last_ideal_tracks_width {
                    w.insert_temp(ideal_tracks_width_id, tracks_width);
                }
            });

            ui.vertical(|ui| {
                if ctx.animation_preview.selected_animation.is_some() {
                    ui.add_space(track_y_offset);
                    Self::show_timeline(ui, ctx, &rendered_tracks);
                }
            });
        });
    }
}
