// Ran before we have anything inserted.
// pub fn init_pre_graphics(app: &mut App) {
//     //let asset_path = AssetPath::new_user_dir(consts::io::GAME_USER_SETTINGS_FILE);
//     //let settings = Settings::from(&match Assets::load_asset_sync::<UserSettingsAsset>(
//     //    asset_path.clone(),
//     //) {
//     //    Ok(settings) => {
//     //        log::info!("Using existing user settings.");
//     //        settings
//     //    }
//     //    Err(_) => {
//     //        log::info!(
//     //            "Existing user settings not found, creating new user settings {:?}.",
//     //            asset_path
//     //        );
//     //        UserSettingsAsset::default()
//     //    }
//     //});
//
//     init_editor_project(app);
// }
//
// /// Initializes the ECSWorld, EditorSession, PhysicsWorld, and Editor.
// pub fn init_editor_project(app: &mut App) {
//     let mut editor_settings = Assets::load_asset_sync::<EditorUserSettingsAsset>(
//         AssetPath::new_user_dir(consts::io::EDITOR_USER_SETTINGS_FILE),
//     )
//     .unwrap_or(EditorUserSettingsAsset {
//         last_project_dir: None,
//     });
//
//     // Ensure last project still exists.
//     if let Some(last_project_dir) = editor_settings.last_project_dir.as_ref() {
//         if std::fs::read_dir(last_project_dir).is_err() {
//             editor_settings.last_project_dir = None;
//         }
//     }
//
//     let project = editor_settings
//         .last_project_dir
//         .as_ref()
//         .map(|last_project_dir| {
//             ProjectAsset::from_existing_raw(last_project_dir)
//                 .map_err(|err| {
//                     log::error!(
//                         "Error when trying to deserialize last project. Error: {:?}",
//                         err
//                     );
//                     err
//                 })
//                 .ok()
//         })
//         .unwrap_or(None);
//
//     if project.is_none() {
//         editor_settings.last_project_dir = None;
//     }
//
//     let project = project.unwrap_or_else(|| ProjectAsset::new_empty());
//
//     // Initialize project assets.
//     let mut events = app.get_resource_mut::<Events>();
//     for (entity, renderable) in project
//         .ecs_world
//         .query::<&RenderableVoxelEntity>()
//         .into_iter()
//     {
//         events.push(voxel_events::EventVoxelRenderableEntityLoad {
//             entity,
//             reload: false,
//         });
//     }
//     drop(events);
//
//     app.insert_resource(project.ecs_world);
//     app.insert_resource(project.physics_world);
//     app.insert_resource(project.material_bank);
//     app.insert_resource(Editor::new(project.editor_settings));
//     app.insert_resource(EditorSession::new(editor_settings, project.settings));
// }
//
// /// The graphics `DeviceResource` has been inserted before this.
// pub fn init_post_graphics(app: &mut App) {
//     engine::ui::initialize_debug_ui_resource(app);
//
//     engine::graphics::initialize_graphics_resources(app);
//     app.insert_resource(MaterialBankGpu::new());
//
//     app.insert_resource(World::new());
//     engine::voxel::initialize_voxel_world_resources(app);
// }
