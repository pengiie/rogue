use rogue_engine::{
    animation::{
        animation::{AnimationPropertyTypeInfo, AnimationTrackId},
        animation_bank::AnimationId,
    },
    asset::asset::GameAssetPath,
    entity::{GameEntity, ecs_world::Entity},
};

use crate::{
    session::EditorCommandEvent,
    ui::{EditorCommand, EditorDialog, EditorUIContext},
};

const DIALOG_ID: &str = "create_animation_track_dialog";

#[derive(Clone)]
struct CreateAnimationTrackDialogState {
    selected_track: Option<SelectedTrackInfo>,
}

#[derive(Clone)]
struct SelectedTrackInfo {
    track_id: AnimationTrackId,
    property_type_info: AnimationPropertyTypeInfo,
}

pub struct CreateAnimationTrackDialogCreateInfo {
    pub target_entity: Entity,
    pub target_animation: GameAssetPath,
}

pub fn create_animation_track_dialog(
    create_info: CreateAnimationTrackDialogCreateInfo,
) -> EditorCommand {
    EditorCommand::OpenDialog(EditorDialog {
        id: DIALOG_ID.to_owned(),
        title: "Add animation property".to_owned(),
        show_fn: Box::new(move |ui, ctx| {
            create_animation_track_dialog_show_fn(ui, ctx, &create_info)
        }),
    })
}

fn create_animation_track_dialog_show_fn(
    ui: &mut egui::Ui,
    ctx: &mut EditorUIContext,
    create_info: &CreateAnimationTrackDialogCreateInfo,
) -> bool {
    ui.vertical(|ui| {
        let id = egui::Id::new(format!("animation_property_dialog"));
        let mut state = ui.data_mut(|w| {
            w.get_temp_mut_or_insert_with(id, || CreateAnimationTrackDialogState {
                selected_track: None,
            })
            .clone()
        });

        ui.label("Select property to animate:");
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() * 0.75)
            .show(ui, |ui| {
                let mut entity_path = Vec::new();
                show_entity_properties(
                    ui,
                    ctx,
                    &mut state,
                    create_info.target_entity,
                    &mut entity_path,
                    &create_info.target_animation,
                );
            });

        ui.horizontal(|ui| {
            if ui
                .add_enabled(state.selected_track.is_some(), egui::Button::new("Select"))
                .clicked()
            {
                let SelectedTrackInfo {
                    track_id,
                    property_type_info,
                } = state.selected_track.clone().unwrap();
                ctx.commands
                    .push(EditorCommand::CloseDialog(DIALOG_ID.to_owned()));
                let Some(animation) = ctx
                    .animation_bank
                    .get_animation_mut(&create_info.target_animation)
                else {
                    // Animation must have been deleted while dialog was open.
                    return;
                };
                if animation.contains_track(&track_id) {
                    // Track got added somehow? idk just close.
                    return;
                }
                animation.create_track(track_id, property_type_info);
            }
            if let Some(SelectedTrackInfo {
                track_id,
                property_type_info,
            }) = &state.selected_track
            {
                ui.label(track_id.to_string());
            }
        });

        ui.data_mut(|w| {
            w.insert_temp(id, state);
        });
    });

    false
}

fn show_entity(
    ui: &mut egui::Ui,
    ctx: &mut EditorUIContext<'_>,
    state: &mut CreateAnimationTrackDialogState,
    entity: Entity,
    entity_path: &mut Vec<String>,
    target_animation: &GameAssetPath,
) {
    let game_entity = ctx.ecs_world.get::<&GameEntity>(entity).unwrap();
    ui.label(format!("{}", &game_entity.name));
    drop(game_entity);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        show_entity_properties(ui, ctx, state, entity, entity_path, target_animation);
    });
}

fn show_entity_properties(
    ui: &mut egui::Ui,
    ctx: &mut EditorUIContext<'_>,
    state: &mut CreateAnimationTrackDialogState,
    entity: Entity,
    entity_path: &mut Vec<String>,
    target_animation: &GameAssetPath,
) {
    ui.vertical(|ui| {
        let animatable_components = ctx.ecs_world.get_animatable_game_components(entity);
        let children = ctx.ecs_world.get_children(entity);

        for child in children {
            let entity_name = ctx
                .ecs_world
                .get::<&GameEntity>(child)
                .unwrap()
                .name
                .clone();
            entity_path.push(entity_name);
            show_entity(ui, ctx, state, child, entity_path, target_animation);
            entity_path.pop();
        }

        for (type_info, properties) in animatable_components {
            let game_component_type = ctx
                .ecs_world
                .game_components
                .get(&type_info.type_id)
                .unwrap();
            ui.label(format!("{}", game_component_type.component_name));
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    for property in properties {
                        let property_name = &property.property_name;
                        let track_id = AnimationTrackId {
                            entity_traversal: entity_path.clone(),
                            component_name: game_component_type.component_name.clone(),
                            component_property: property_name.clone(),
                        };
                        if ui
                            .selectable_label(
                                state
                                    .selected_track
                                    .as_ref()
                                    .map(|selected_track| &selected_track.track_id)
                                    == Some(&track_id),
                                property_name,
                            )
                            .clicked()
                        {
                            state.selected_track = Some(SelectedTrackInfo {
                                track_id,
                                property_type_info: property,
                            });
                        }
                    }
                });
            });
        }
    });
}
