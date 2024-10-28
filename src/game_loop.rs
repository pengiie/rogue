use log::debug;

use crate::{
    app::App,
    engine::{
        asset::asset::Assets,
        event::Events,
        graphics::{pipeline_manager::RenderPipelineManager, renderer::Renderer},
        input::Input,
        physics::physics_world::PhysicsWorld,
        system::System,
        ui::UI,
        voxel::voxel_world::VoxelWorldGpu,
        window::time::Time,
    },
    game::{player::player::Player, world::game_world::GameWorld},
};

pub fn game_loop(app: &App) {
    // ------- FRAME SETUP ---------
    app.run_system(Time::update);

    // ------- ASSETS --------

    // Run any queued up asset tasks and update finished tasks.
    app.run_system(Assets::update_assets);

    // ------- GAME WORLD ---------
    if app
        .resource_bank()
        .get_resource_mut::<GameWorld>()
        .try_tick()
    {
        // TICK UPDATES
        app.run_system(GameWorld::load_test_models);
        app.run_system(GameWorld::update_test_models_position);
    }

    // ------- PHYSICS ---------

    // Update player logic.
    app.run_system(Player::update_player);

    app.run_system(PhysicsWorld::do_physics_update);

    // ------- UI ---------

    // Handle UI updates and Egui cpu-render.
    app.run_system(UI::update);
    app.run_system(UI::draw);

    // ------- GPU RENDERING ---------

    // Update render pipelines (loads shaders).
    app.run_system(RenderPipelineManager::update_pipelines);

    // Update voxel world owned gpu objects such as world data buffers.
    app.run_system(VoxelWorldGpu::update_gpu_objects);
    app.run_system(VoxelWorldGpu::write_render_data);

    // Update renderer owned gpu objects, aka all textures and bind groups based on any render
    // state changes.
    app.run_system(Renderer::update_gpu_objects);
    // Write renderer owned buffers and textures.
    //   - UI Textures
    app.run_system(Renderer::write_render_data);

    // Render the frame to the swapchain.
    app.run_system(Renderer::render);

    // ------- FRAME CLEANUP ---------

    // Discard any inputs and events cached for this frame.
    app.run_system(Input::clear_inputs);
    app.run_system(Events::clear_events);
}
