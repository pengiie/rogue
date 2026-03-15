use nalgebra::Vector3;
use rogue_engine::{
    asset::asset::GameAssetPath,
    entity::{RenderableVoxelEntity, ecs_world::Entity},
    material::MaterialId,
    voxel::{
        attachment::Attachment,
        sft_compressed::VoxelModelSFTCompressed,
        voxel::{VoxelEditData, VoxelMaterialData, VoxelModelEdit, VoxelModelImpl},
    },
};

use crate::{
    session::EditorEvent,
    ui::{EditorCommand, EditorDialog, EditorUIContext, FilePickerType},
};

pub struct CreateVoxelModelDialogCreateInfo {
    pub target_entity: Option<Entity>,
}

const DIALOG_ID: &str = "create_voxel_model_dialog";

#[derive(Copy, Clone, Debug, PartialEq, Eq, strum_macros::VariantArray)]
enum CreateModelPreset {
    Empty,
    Solid,
}
#[derive(Clone)]
struct NewModelDialogState {
    side_length: u32,
    preset: CreateModelPreset,
    material: MaterialId,
}

fn create_voxel_model_dialog_show_fn(
    ui: &mut egui::Ui,
    ctx: &mut EditorUIContext,
    create_info: &CreateVoxelModelDialogCreateInfo,
) -> bool {
    ui.vertical(|ui| {
        let id = egui::Id::new(format!("new_voxel_model_dialog"));
        let mut state = ui.data_mut(|w| {
            w.get_temp_mut_or_insert_with(id, || {
                let default_material = ctx
                    .material_bank
                    .contains_material(&MaterialId::new(0, 0))
                    .then_some(MaterialId::new(0, 0))
                    .unwrap_or(MaterialId::null());
                NewModelDialogState {
                    side_length: 16,
                    preset: CreateModelPreset::Solid,
                    material: default_material,
                }
            })
            .clone()
        });

        ui.horizontal(|ui| {
            const MAX_DIMENSION: u32 = 4u32.pow(10);
            ui.label("Side Length:");
            ui.label("X");
            ui.add(egui::DragValue::new(&mut state.side_length).range(4..=MAX_DIMENSION));
        });

        ui.horizontal(|ui| {
            ui.label("Preset:");
            egui::ComboBox::from_id_salt("Create model preset")
                .selected_text(format!("{:?}", state.preset))
                .show_ui(ui, |ui| {
                    use strum::VariantArray as _;
                    for val in CreateModelPreset::VARIANTS {
                        ui.selectable_value(&mut state.preset, val.clone(), format!("{:?}", val));
                    }
                });
        });

        ui.add_enabled_ui(state.preset != CreateModelPreset::Empty, |ui| {
            ui.horizontal(|ui| {
                ui.label("Fill Material:");
                use crate::ui::material_picker::material_picker;
                material_picker(ui, ctx.material_bank, &mut state.material);
            });
        });

        if ui.button("Create").clicked() {
            let target_entity = create_info.target_entity.clone();
            ctx.commands.push(EditorCommand::FilePicker {
                picker_type: FilePickerType::CreateFile,
                callback: Box::new(move |ctx, file_path| {
                    let asset_path = GameAssetPath::from_relative_path(&file_path);

                    let mut model = VoxelModelSFTCompressed::new_empty(state.side_length);
                    model.initialize_attachment_buffers(&Attachment::BMAT);
                    match state.preset {
                        CreateModelPreset::Empty => {
                            // Do nothing, already empty.
                        }
                        CreateModelPreset::Solid => {
                            let edit = VoxelModelEdit {
                                min: Vector3::new(0, 0, 0),
                                max: Vector3::new(
                                    state.side_length,
                                    state.side_length,
                                    state.side_length,
                                ),
                                data: VoxelEditData::Fill {
                                    material: Some(VoxelMaterialData::Unbaked(state.material)),
                                },
                            };
                            model.set_voxel_range_impl(&edit);
                        }
                    }
                    let model_id = ctx
                        .voxel_registry
                        .register_voxel_model(model, Some(asset_path.clone()));

                    if let Some(selected_entity) = target_entity {
                        if let Ok(mut renderable) = ctx
                            .ecs_world
                            .get::<&mut RenderableVoxelEntity>(selected_entity)
                        {
                            renderable.set_model(Some(asset_path.clone()), model_id);
                        }
                    }
                    ctx.commands
                        .push(EditorCommand::CloseDialog(DIALOG_ID.to_owned()));
                    ctx.events.push(EditorEvent::SaveVoxelModel(asset_path));
                }),
                extensions: vec!["rvox".to_owned()],
                preset_file_path: None,
            });
        }

        ui.data_mut(|w| {
            w.insert_temp(id, state);
        });
    });

    false
}

pub fn create_voxel_model_dialog(create_info: CreateVoxelModelDialogCreateInfo) -> EditorCommand {
    EditorCommand::OpenDialog(EditorDialog {
        id: DIALOG_ID.to_owned(),
        title: "Create Voxel Model".to_owned(),
        show_fn: Box::new(move |ui, ctx| create_voxel_model_dialog_show_fn(ui, ctx, &create_info)),
    })
}
