use std::{collections::HashSet, path::PathBuf, time::Duration};

use rogue_engine::{
    asset::asset::{Assets, GameAssetPath},
    entity::ecs_world::{ECSWorld, Entity},
    event::{EventReader, Events},
    graphics::camera::{Camera, MainCamera},
    input::{Input, input_buffer::InputBuffer, mouse},
    material::MaterialBank,
    physics::{physics_world::PhysicsWorld, transform::Transform},
    resource::{Res, ResMut},
    voxel::{rvox_asset::RVOXAsset, voxel_registry::VoxelModelRegistry},
    window::{time::Time, window::Window},
    world::{
        region_map::RegionMap,
        world_entities::{WorldEntities, WorldEntityRaycastHit},
    },
};
use rogue_macros::Resource;
use winit::event::MouseButton;

use crate::{
    camera_controller::{EditorCameraController, EditorCameraControllerType},
    editor_settings::{UserEditorSettingsAsset, UserEditorSettingsAssetProxy},
    game_session::EditorGameSession,
    ui::EditorUI,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EditorEvent {
    SaveEditorSettings,
    SaveProject,
    SaveVoxelModel(GameAssetPath),
}

#[derive(Resource)]
pub struct EditorSession {
    pub entity_raycast: Option<WorldEntityRaycastHit>,
    pub selected_entity: Option<Entity>,
    pub hovered_entity: Option<Entity>,

    editor_camera: Entity,
    editor_camera_controller: EditorCameraController,
    double_right_click_buffer: InputBuffer,

    editor_event_reader: EventReader<EditorEvent>,
}

impl EditorSession {
    pub fn new(ecs_world: &mut ECSWorld, main_camera: &mut MainCamera) -> Self {
        let editor_camera = Self::init_editor_camera(ecs_world);
        main_camera.set_camera(editor_camera, "editor_camera");

        Self {
            entity_raycast: None,
            selected_entity: None,
            hovered_entity: None,

            editor_camera,
            editor_camera_controller: EditorCameraController::new(),
            double_right_click_buffer: InputBuffer::new(2),
            editor_event_reader: EventReader::new(),
        }
    }

    pub fn init_editor_camera(ecs_world: &mut ECSWorld) -> Entity {
        ecs_world.spawn((Transform::new(), Camera::new(90.0f32.to_radians())))
    }

    pub fn update_editor_camera_controller(
        mut session: ResMut<EditorSession>,
        ecs_world: ResMut<ECSWorld>,
        input: Res<Input>,
        time: Res<Time>,
        mut window: ResMut<Window>,
    ) {
        let camera_transform = &mut ecs_world
            .get::<&mut Transform>(session.editor_camera)
            .unwrap();
        session
            .editor_camera_controller
            .update(camera_transform, &input, &time, &mut window);
        const RIGHT_CLICK_DOUBLE_CLICK_MS: u64 = 200;
        session
            .double_right_click_buffer
            .update(input.is_mouse_button_pressed(mouse::Button::Right));

        if session
            .double_right_click_buffer
            .did_double_input(Duration::from_millis(RIGHT_CLICK_DOUBLE_CLICK_MS))
            && let Some(raycast) = session.entity_raycast()
            && let Ok(entity_transform) = ecs_world.get::<&Transform>(raycast.entity)
        {
            // On double right click, focus camera on raycast hit.
            session
                .editor_camera_controller
                .focus_on_position(entity_transform.position);
        }
    }

    pub fn editor_camera_controller(&self) -> &EditorCameraController {
        &self.editor_camera_controller
    }

    pub fn update_selected_entity_and_raycast(
        mut session: ResMut<EditorSession>,
        ecs_world: Res<ECSWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        input: Res<Input>,
        editor_ui: Res<EditorUI>,
        window: Res<Window>,
    ) {
        // Update entity raycast.
        let Some((editor_camera_transform, editor_camera)) = ecs_world
            .query_one::<(&Transform, &Camera)>(session.editor_camera)
            .get()
        else {
            return;
        };

        let backbuffer_size = editor_ui.backbuffer_size(&window).cast::<f32>();
        let ray = match session.editor_camera_controller.controller_type {
            EditorCameraControllerType::PanOrbit => {
                let mouse_pos =
                    input.mouse_position() - editor_ui.backbuffer_offset().cast::<f32>();
                let uv = mouse_pos.component_div(&backbuffer_size);
                let aspect_ratio = backbuffer_size.x / backbuffer_size.y;
                editor_camera.create_ray(editor_camera_transform, uv, aspect_ratio)
            }
            EditorCameraControllerType::Fps => {
                return;
            }
        };

        session.entity_raycast =
            WorldEntities::raycast_voxel_entities(&ray, &ecs_world, &voxel_registry);

        // Update selected entity.
        if !input.is_mouse_button_pressed(mouse::Button::Left) {
            return;
        }
        if let Some(hit) = &session.entity_raycast {
            session.selected_entity = Some(hit.entity);
        } else {
            session.selected_entity = None;
        }
    }

    pub fn entity_raycast(&self) -> Option<&WorldEntityRaycastHit> {
        self.entity_raycast.as_ref()
    }

    pub fn update_editor_events(
        assets: Res<Assets>,
        editor_ui: Res<EditorUI>,
        events: Res<Events>,
        mut session: ResMut<EditorSession>,
        ecs_world: Res<ECSWorld>,
        physics_world: Res<PhysicsWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        material_bank: Res<MaterialBank>,
        main_camera: Res<MainCamera>,
        region_map: Res<RegionMap>,
        game_session: Res<EditorGameSession>,
    ) {
        let mut unique_events = HashSet::new();
        for event in session.editor_event_reader.read(&events) {
            unique_events.insert(event);
        }

        for event in unique_events {
            match event {
                EditorEvent::SaveEditorSettings => {
                    log::info!("Saving editor settings");
                    let editor_settings = UserEditorSettingsAssetProxy {
                        last_project_dir: assets.project_dir(),
                        editor_ui: &editor_ui,
                    };
                    editor_settings.save_settings();
                }
                EditorEvent::SaveProject => {
                    log::info!("Saving project");
                    assets.save_project(
                        rogue_engine::asset::repr::project::ProjectSerializeContext {
                            ecs_world: &ecs_world,
                            physics_world: &physics_world,
                            voxel_registry: &voxel_registry,
                            material_bank: &material_bank,
                            main_camera: &main_camera,
                            region_map: &region_map,
                            game_camera: game_session.game_camera.clone(),
                        },
                    );
                }
                EditorEvent::SaveVoxelModel(game_asset_path) => {
                    let Some(project_dir) = assets.project_dir() else {
                        return;
                    };
                    let Some(voxel_model_id) = voxel_registry.get_asset_model_id(&game_asset_path)
                    else {
                        log::error!("Tried to save voxel model that doesn't exist in registry!");
                        return;
                    };
                    let asset = voxel_registry
                        .get_dyn_model(voxel_model_id)
                        .create_rvox_asset();
                    let asset_path = game_asset_path.as_file_asset_path(&project_dir);
                    Assets::save_asset_sync::<RVOXAsset>(asset_path, asset);
                }
            }
        }
    }
}
