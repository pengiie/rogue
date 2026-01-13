use crate::asset::asset::Assets;
use crate::debug::DebugRenderer;
use crate::entity::{ecs_world::ECSWorld, scripting::Scripts};
use crate::event::Events;
use crate::graphics::{device::DeviceResource, renderer::Renderer};
use crate::input::Input;
use crate::material::{material_gpu::MaterialBankGpu, MaterialBank};
use crate::physics::physics_world::PhysicsWorld;
use crate::system::System;
use crate::voxel::voxel_registry::VoxelModelRegistry;
use crate::voxel::voxel_registry_gpu::VoxelModelRegistryGpu;
use crate::window::time::{Instant, Time};
use crate::world::world_renderable::WorldRenderable;
use crate::{
    app::{App, AppStage},
    game::game_loop,
};

pub fn game_loop(app: &App) {
    // This system is called in app.
    // DeviceResource::update

    // ------- FRAME SETUP ---------
    app.run_system(DeviceResource::begin_frame);
    app.run_system(Time::update);
    app.run_system(Input::collect_gamepad_events);

    let cpu_time = Instant::now();

    // TODO: Figure out why we do this here on renderer rewrite cause i forgot
    // ------- RENDERER SETUP
    app.run_system(Renderer::begin_frame);

    // ------ SESSION ----------

    // Since we are about to run scripts, ensure state is updated.
    app.run_system(Scripts::update_world_state);

    // Starts and stops the game, running Scripts::on_setup.
    // Manages project asset loading.
    //app.run_system(EditorSession::update);

    // ------- ASSETS --------

    // Load any requested scripts
    app.run_system(Scripts::update_loaded_scripts);

    // Run any queued up asset tasks and update finished tasks.
    app.run_system(Assets::update);

    // ------- UI ---------

    // Handle UI updates and Egui cpu-render.
    //app.run_system(UI::update);
    //app.run_system(UI::draw);

    // ------- EDITOR INPUT---------

    //app.run_system(Editor::update_toggle);
    //if app.get_resource::<Editor>().is_active {
    //    app.run_system(Editor::update_editor_actions);
    //    app.run_system(Editor::update_editor_zoom);
    //    let curr_editor_view = app.get_resource::<Editor>().curr_editor_view;
    //    match curr_editor_view {
    //        EditorView::PanOrbit => {
    //            app.run_system(Editor::update_editor_pan_orbit);
    //        }
    //        EditorView::Fps => {
    //            app.run_system(Editor::update_editor_fps);
    //        }
    //    }

    //    // Override editor camera position with any animations.
    //    app.run_system(Editor::update_camera_animations);
    //}

    // ----- GAME SCRIPTS ------
    // let session_state = app.get_resource::<EditorSession>().session_state;
    // if session_state == SessionState::Game {
    //     game_loop::on_game_update(app);
    // }
    if let Some(systems) = app.systems(AppStage::Update) {
        for system in systems {
            system.run(app.resource_bank());
        }
    }

    // -------- PHYSICS ----------
    // Only do dynamics if we are in game running.
    //app.get_resource_mut::<PhysicsWorld>().do_dynamics = session_state == SessionState::Game;
    let physics_updates = app
        .get_resource_mut::<PhysicsWorld>()
        .physics_update_count();
    for _ in 0..physics_updates {
        app.run_system(PhysicsWorld::start_time_step);
        if let Some(systems) = app.systems(AppStage::FixedUpdate) {
            for system in systems {
                system.run(app.resource_bank());
            }
        }
        //app.run_system(Scripts::run_on_physics_update);
        //app.run_system(Scripts::update_script_events);
        app.run_system(PhysicsWorld::do_physics_update);
        app.run_system(PhysicsWorld::end_time_step);
    }

    // Handle ECSWorld events.
    app.run_system(ECSWorld::handle_despawn_events);

    // ------- VOXEL REGISTRY -------
    // Load any entity renderables that need to be loaded.
    app.run_system(VoxelModelRegistry::handle_model_load_events);

    // ------- SPATIAL WORLD -------

    // Handle region and chunk loading updates.
    //app.run_system(World::update);

    // Rendered terrain relative to player/camera anchor updating.
    app.run_system(WorldRenderable::update);

    // ------- GPU RENDERING ---------

    app.run_system(PhysicsWorld::render_debug_colliders);

    // Ensure we write this before the voxel world so materials can
    // be pointed to within the same frame.
    app.run_system(MaterialBank::update_events);
    app.run_system(MaterialBankGpu::write_render_data);

    //app.run_system(VoxelWorldGpu::update_gpu_objects);
    //app.run_system(VoxelWorldGpu::write_render_data);

    // Only continue with frame graph pass writing if we successfully acquired the swapchain since
    // some images may rely on swapchain info.
    app.run_system(Renderer::acquire_swapchain_image);
    if app.get_resource::<Renderer>().did_acquire_swapchain() {
        if let Some(systems) = app.systems(AppStage::RenderWrite) {
            for system in systems {
                system.run(app.resource_bank());
            }
        }

        app.run_system(DebugRenderer::write_debug_shapes_pass);
        app.run_system(WorldRenderable::write_render_data);
        app.run_system(VoxelModelRegistryGpu::write_render_data);

        app.run_system(Renderer::write_frame_uniforms);
        app.run_system(Renderer::finish_frame);
    }

    // ------- FRAME CLEANUP ---------

    // Discard any inputs and events cached for this frame.
    app.run_system(Input::clear_inputs);
    app.run_system(Events::frame_cleanup);
    //app.run_system(VoxelWorld::clear_state);
    app.run_system(Events::frame_cleanup);

    //debug!(
    //    "CPU Frame took {}ms",
    //    cpu_time.elapsed().as_micros() as f32 / 1000.0
    //)
}
