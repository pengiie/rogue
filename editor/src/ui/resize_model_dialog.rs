use nalgebra::Vector3;
use rogue_engine::{
    common::{
        color::Color,
        morton::{next_power_of_4, prev_power_of_4},
    },
    entity::{RenderableVoxelEntity, ecs_world::Entity},
    physics::transform::Transform,
    voxel::{
        sft_compressed::VoxelModelSFTCompressed,
        voxel_registry::{VoxelModelEvent, VoxelModelId},
    },
};

use crate::ui::{EditorCommand, EditorDialog, EditorUIContext};

const DIALOG_ID: &str = "resize_voxel_model_dialog";

pub struct ResizeVoxelModelDialogCreateInfo {
    pub target_model: VoxelModelId,
    pub associated_entity: Entity,
}

#[derive(Clone)]
struct ResizeVoxelModelDialogState {
    original_side_length: Vector3<u32>,
    side_length: Vector3<u32>,
}

pub fn resize_voxel_model_dialog_cmd(
    create_info: ResizeVoxelModelDialogCreateInfo,
) -> EditorCommand {
    EditorCommand::OpenDialog(EditorDialog {
        id: DIALOG_ID.to_owned(),
        title: "Resize Voxel Model".to_owned(),
        show_fn: Box::new(move |ui, ctx| resize_voxel_model_dialog_show_fn(ui, ctx, &create_info)),
    })
}

fn resize_voxel_model_dialog_show_fn(
    ui: &mut egui::Ui,
    ctx: &mut EditorUIContext,
    create_info: &ResizeVoxelModelDialogCreateInfo,
) -> bool {
    let id = egui::Id::new(format!("resize_voxel_model_dialog"));
    let mut state = ui.data_mut(|w| {
        w.get_temp_mut_or_insert_with(id, || {
            let side_length = ctx
                .voxel_registry
                .get_dyn_model(create_info.target_model)
                .length();
            ResizeVoxelModelDialogState {
                side_length,
                original_side_length: side_length,
            }
        })
        .clone()
    });

    // Render a bounding box around the associated entities model.
    if let Some((transform, renderable)) = ctx
        .ecs_world
        .query_one::<(&Transform, &RenderableVoxelEntity)>(create_info.associated_entity)
        .get()
    {
        let world_transform = ctx
            .ecs_world
            .get_world_transform(create_info.associated_entity, transform);
        let model_side_length = ctx
            .voxel_registry
            .get_dyn_model(create_info.target_model)
            .length();
        const OBB_THICKNESS: f32 = 0.025;
        let new_obb = world_transform.as_voxel_model_obb(state.side_length);
        ctx.debug_renderer
            .draw_obb(&new_obb, OBB_THICKNESS, Color::new_srgb_hex("#FF0000"));
    };

    let model_type = ctx
        .voxel_registry
        .get_voxel_model_type_id(create_info.target_model);
    if model_type == std::any::TypeId::of::<VoxelModelSFTCompressed>() {
        let mut side_length = state.side_length.x;
        let powers_of_4 = [
            // 4
            1 << 2,
            // 16
            1 << 4,
            // 64
            1 << 6,
            // 256
            1 << 8,
            // 1024
            1 << 10,
            // 4096
            1 << 12,
        ];
        egui::ComboBox::from_label("Side Length")
            .selected_text(side_length.to_string())
            .show_ui(ui, |ui| {
                for &power in &powers_of_4 {
                    ui.selectable_value(&mut side_length, power, power.to_string());
                }
            });
        state.side_length = Vector3::new(side_length, side_length, side_length);
    } else {
        ui.label("Don't know how to resize this voxel model type.");
    }

    let mut close_requested = false;
    ui.horizontal(|ui| {
        let changed_size = state.side_length != state.original_side_length;
        if ui
            .add_enabled(changed_size, egui::Button::new("Apply"))
            .clicked()
        {
            ctx.voxel_registry
                .get_dyn_model_mut(create_info.target_model)
                .resize_model(state.side_length);
            ctx.events
                .push(VoxelModelEvent::UpdatedModel(create_info.target_model));
            close_requested = true;
        }

        if ui.button("Close").clicked() {
            close_requested = true;
        }
    });

    // Preserve state.
    ui.data_mut(|w| {
        w.insert_temp(id, state);
    });
    return close_requested;
}
