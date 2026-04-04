use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use rogue_engine::{
    animation::{animation::Animation, animator::Animator},
    asset::asset::GameAssetPath,
};

use crate::{
    session::EditorCommandEvent,
    ui::{
        EditorCommand, EditorUIContext, FilePickerType,
        asset_properties_pane::AssetPropertiesPane,
        create_animation_track_dialog::{self, CreateAnimationTrackDialogCreateInfo},
        pane::EditorUIPane,
    },
};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default = "AnimationPane::new")]
pub struct AnimationPane {}

impl AnimationPane {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show_header(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Animation").size(20.0));

            let Some(selected_entity) = &ctx.session.selected_entity else {
                // Error is shown in pane show fn.
                return;
            };
            let Ok(animator) = ctx.ecs_world.get::<&mut Animator>(*selected_entity) else {
                // Error is shown in pane show fn.
                return;
            };

            let selected_animation = &mut ctx.animation_preview.selected_animation;
            ui.horizontal(|ui| {
                ui.label("Selected animation:");
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
    const TRACK_HEIGHT: f32 = 16.0;
    fn show_tracks(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        let Some(selected_entity) = ctx.session.selected_entity else {
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
            let mut should_update_duration = false;
            for (i, track) in selected_animation.tracks.iter_mut().enumerate() {
                for (j, channel) in track.channels.iter_mut().enumerate() {
                    let index = i + j;
                    let color = if index % 2 == 0 {
                        egui::Color32::from_hex(Self::TRACK_BG_FILL_1).unwrap()
                    } else {
                        egui::Color32::from_hex(Self::TRACK_BG_FILL_2).unwrap()
                    };
                    ui.style_mut().visuals.widgets.inactive.bg_fill = color;
                    ui.horizontal(|ui| {
                        ui.set_max_height(Self::TRACK_HEIGHT);
                        ui.label(format!(
                            "{}.{}",
                            track.track_id.to_string(),
                            channel.channel_type_info.channel_name
                        ));
                        // Record keyframe.
                        if ui.small_button("O").clicked() {
                            channel.record_keyframe(
                                ctx.ecs_world,
                                selected_entity,
                                ctx.animation_preview.playhead,
                            );
                            should_update_duration |= true;
                        }
                    });
                }
            }
            if should_update_duration {
                selected_animation.update_duration();
            }
        });
    }

    const TIME_AXIS_HEIGHT: f32 = 24.0;
    fn show_timeline(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        let Some(selected_entity) = &ctx.session.selected_entity else {
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
            const TIMELINE_ZOOM_SENSITIVITY: f32 = 0.001;
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
        const TICK_FONT_SIZE: f32 = 8.0;
        const TICK_HEIGHT: f32 = 4.0;
        const TICK_FONT_SPACING: f32 = 1.0;
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
            ctx.animation_preview.playhead =
                Duration::from_secs_f32(start_time + mouse_t * timeline_duration);
        }

        let timeline_rect = egui::Rect::from_min_size(
            time_axis_rect.min + egui::vec2(0.0, Self::TIME_AXIS_HEIGHT),
            egui::vec2(timeline_width, ui.available_height()),
        );

        // Draw keyframes.
        for (i, track) in selected_animation.tracks.iter().enumerate() {
            for (j, channel) in track.channels.iter().enumerate() {
                let y_index = i + j;
                let color = if y_index % 2 == 0 {
                    egui::Color32::from_hex(Self::TRACK_BG_FILL_1).unwrap()
                } else {
                    egui::Color32::from_hex(Self::TRACK_BG_FILL_2).unwrap()
                };
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(
                            timeline_rect.min.x,
                            timeline_rect.min.y + y_index as f32 * Self::TRACK_HEIGHT,
                        ),
                        egui::vec2(timeline_width, Self::TRACK_HEIGHT),
                    ),
                    0.0,
                    color,
                );
                for (ki, keyframe_time) in channel.times.iter().enumerate() {
                    let keyframe_time = keyframe_time.as_secs_f32();
                    let keyframe_x = ((keyframe_time - start_time) / timeline_duration)
                        * timeline_width
                        + timeline_rect.min.x;
                    let keyframe_y = timeline_rect.min.y + y_index as f32 * Self::TRACK_HEIGHT;
                    ui.painter().circle_filled(
                        egui::pos2(keyframe_x, keyframe_y + Self::TRACK_HEIGHT * 0.5),
                        4.0,
                        egui::Color32::from_hex("#AA1111").unwrap(),
                    );
                    let res = ui.interact(
                        egui::Rect::from_center_size(
                            egui::pos2(keyframe_x, keyframe_y + Self::TRACK_HEIGHT * 0.5),
                            egui::vec2(10.0, Self::TRACK_HEIGHT),
                        ),
                        ui.id().with(format!("keyframe_{}_{}_{}", i, j, ki)),
                        egui::Sense::click(),
                    );
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

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        Self::show_header(ui, ctx);

        let Some(selected_entity) = ctx.session.selected_entity else {
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

        ui.horizontal(|ui| {
            let mut track_y_offset = 0.0;
            ui.vertical(|ui| {
                Self::show_tracks(ui, ctx);
            });
            ui.vertical(|ui| {
                if ctx.animation_preview.selected_animation.is_some() {
                    ui.add_space(track_y_offset);
                    Self::show_timeline(ui, ctx);
                }
            });
        });
    }
}
