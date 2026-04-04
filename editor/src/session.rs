use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};

use nalgebra::Vector3;
use rogue_engine::{
    animation::{animation::Animation, animation_bank::AnimationBank},
    asset::asset::{Assets, GameAssetPath},
    common::geometry::ray::Ray,
    entity::ecs_world::{ECSWorld, Entity},
    event::{EventReader, Events},
    graphics::camera::{Camera, MainCamera},
    input::{Input, input_buffer::InputBuffer, mouse},
    material::MaterialBank,
    physics::{physics_world::PhysicsWorld, transform::Transform},
    resource::{Res, ResMut},
    voxel::{
        rvox_asset::RVOXAsset,
        voxel_registry::{self, VoxelModelId, VoxelModelRegistry},
    },
    window::{time::Time, window::Window},
    world::{
        region_map::{RegionMap, TerrainRaycastHit},
        world_entities::{WorldEntities, WorldEntityRaycastHit},
    },
};
use rogue_macros::Resource;
use winit::event::MouseButton;

use crate::{
    camera_controller::{EditorCameraController, EditorCameraControllerType},
    editor_project_settings::{EditorProjectSettings, EditorProjectSettingsData},
    editor_settings::{UserEditorSettingsAsset, UserEditorSettingsAssetProxy},
    game_session::EditorGameSession,
    gizmo::EditorGizmo,
    ui::EditorUI,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EditorCommandEvent {
    SaveEditorSettings,
    SaveProject,
    SaveVoxelModel(VoxelModelId),
    SaveAnimation(GameAssetPath),
}

pub enum EditorEvent {
    SelectedEntity(Option<Entity>),
}

#[derive(Resource)]
pub struct EditorSession {
    pub entity_raycast: Option<WorldEntityRaycastHit>,
    pub terrain_raycast: Option<TerrainRaycastHit>,
    pub editor_camera_ray: Ray,
    pub selected_entity: Option<Entity>,
    pub last_selected_entity: Option<Entity>,
    pub hovered_entity: Option<Entity>,

    pub editor_camera: Entity,
    pub editor_camera_focused: bool,
    editor_camera_controller: EditorCameraController,
    double_right_click_buffer: InputBuffer,

    pub render_colliders: bool,

    editor_event_reader: EventReader<EditorCommandEvent>,
}

impl EditorSession {
    pub fn new(
        ecs_world: &mut ECSWorld,
        main_camera: &mut MainCamera,
        project_settings: Option<&EditorProjectSettingsData>,
    ) -> Self {
        let editor_camera = Self::init_editor_camera(ecs_world);
        main_camera.set_camera(editor_camera, "editor_camera");

        Self {
            entity_raycast: None,
            terrain_raycast: None,
            editor_camera_ray: Ray::new(Vector3::zeros(), Vector3::zeros()),
            selected_entity: None,
            last_selected_entity: None,
            hovered_entity: None,

            render_colliders: false,

            editor_camera,
            editor_camera_focused: true,
            editor_camera_controller: project_settings.map_or_else(
                || EditorCameraController::new(),
                |settings| EditorCameraController::from_project_settings(settings),
            ),
            double_right_click_buffer: InputBuffer::new(2),
            editor_event_reader: EventReader::new(),
        }
    }

    pub fn is_editor_camera_focused(&self) -> bool {
        self.editor_camera_focused
    }

    pub fn editor_camera(&self) -> Entity {
        self.editor_camera
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
        main_camera: Res<MainCamera>,
    ) {
        session.editor_camera_focused = main_camera.camera() == Some(session.editor_camera());
        if !session.editor_camera_focused {
            return;
        }

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
            let entity_world_transform =
                ecs_world.get_world_transform(raycast.entity, &entity_transform);
            // On double right click, focus camera on raycast hit.
            session
                .editor_camera_controller
                .focus_on_position(entity_world_transform.position);
        }
    }

    pub fn editor_camera_controller(&self) -> &EditorCameraController {
        &self.editor_camera_controller
    }

    pub fn update_raycasts(
        mut session: ResMut<EditorSession>,
        ecs_world: Res<ECSWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        input: Res<Input>,
        editor_ui: Res<EditorUI>,
        window: Res<Window>,
        region_map: Res<RegionMap>,
    ) {
        // Update entity and terrain raycast.
        let Some((editor_camera_transform, editor_camera)) = ecs_world
            .query_one::<(&Transform, &Camera)>(session.editor_camera)
            .get()
        else {
            return;
        };

        let backbuffer_size = editor_ui.backbuffer_size(&window).cast::<f32>();
        let ray = match session.editor_camera_controller.controller_type {
            EditorCameraControllerType::PanOrbit => {
                let mouse_pos = input.mouse_position();
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
        session.terrain_raycast = region_map.raycast_terrain(&voxel_registry, &ray, 1000.0);
        session.editor_camera_ray = ray;
    }

    pub fn update_selected_entity(
        mut session: ResMut<EditorSession>,
        ecs_world: Res<ECSWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        input: Res<Input>,
        editor_ui: Res<EditorUI>,
        window: Res<Window>,
        gizmo: Res<EditorGizmo>,
        mut events: ResMut<Events>,
    ) {
        // Update selected entity.
        if input.is_mouse_button_pressed(mouse::Button::Left) && !gizmo.is_hovering() {
            if let Some(hit) = &session.entity_raycast {
                session.selected_entity = Some(hit.entity);
            } else {
                session.selected_entity = None;
            }
        }

        // Send out event if selected entity changed at any point.
        if session.selected_entity != session.last_selected_entity {
            session.last_selected_entity = session.selected_entity;
            events.push(EditorEvent::SelectedEntity(session.selected_entity));
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
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        material_bank: Res<MaterialBank>,
        main_camera: Res<MainCamera>,
        region_map: Res<RegionMap>,
        game_session: Res<EditorGameSession>,
        mut project_settings: ResMut<EditorProjectSettings>,
        mut animation_bank: ResMut<AnimationBank>,
    ) {
        let session = &mut *session;
        let mut unique_events = HashSet::new();
        for event in session.editor_event_reader.read(&events) {
            unique_events.insert(event);
        }

        for event in unique_events {
            match event {
                EditorCommandEvent::SaveEditorSettings => {
                    log::info!("Saving editor settings");
                    if let Some(project_dir) = assets.project_dir() {
                        project_settings.projects.insert(
                            project_dir.clone(),
                            EditorProjectSettingsData {
                                editor_camera_anchor: session
                                    .editor_camera_controller
                                    .rotation_anchor,
                                editor_camera_rotation: session.editor_camera_controller.euler,
                                editor_camera_distance: session.editor_camera_controller.distance,
                            },
                        );
                    }
                    let editor_settings = UserEditorSettingsAssetProxy {
                        last_project_dir: assets.project_dir(),
                        editor_ui: &editor_ui,
                        user_project_settings: &project_settings,
                    };
                    editor_settings.save_settings();
                }
                EditorCommandEvent::SaveProject => {
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
                EditorCommandEvent::SaveVoxelModel(voxel_model_id) => {
                    let Some(project_dir) = assets.project_dir() else {
                        return;
                    };
                    let asset = voxel_registry
                        .get_dyn_model(*voxel_model_id)
                        .create_rvox_asset();
                    let game_asset_path = voxel_registry.get_model_asset_path(*voxel_model_id).expect("Should not request to save voxel model if it doesn't have an associated asset path.");
                    voxel_registry.update_static_asset_model(&game_asset_path, *voxel_model_id);
                    let asset_path = game_asset_path.as_file_asset_path(&project_dir);
                    Assets::save_asset_sync::<RVOXAsset>(asset_path, asset);
                }
                EditorCommandEvent::SaveAnimation(animation_path) => {
                    let Some(project_dir) = assets.project_dir() else {
                        return;
                    };
                    if let Some(animation) = animation_bank.get_animation_by_path(animation_path) {
                        let asset_path = animation_path.as_file_asset_path(&project_dir);
                        Assets::save_asset_sync::<Animation>(asset_path, animation.clone());
                    }
                }
            }
        }
    }
}
