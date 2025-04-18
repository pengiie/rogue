use log::debug;

use crate::{
    app::App,
    engine::{
        asset::{self, asset::Assets},
        event::Events,
        graphics::{device::DeviceResource, pass::ui::UIPass, renderer::Renderer},
        input::Input,
        physics::physics_world::PhysicsWorld,
        system::System,
        ui::UI,
        voxel::{
            cursor::VoxelCursor,
            voxel_world::{VoxelWorld, VoxelWorldGpu},
        },
        window::time::{Instant, Time},
        world::game_world::GameWorld,
    },
    game::entity::player::Player,
};

pub fn game_loop(app: &App) {
    // This system is called in app.
    // DeviceResource::update

    // ------- FRAME SETUP ---------
    app.run_system(DeviceResource::begin_frame);
    app.run_system(Time::update);

    let cpu_time = Instant::now();

    // ------- RENDERER SETUP
    app.run_system(Renderer::begin_frame);

    // ------- ASSETS --------

    // Run any queued up asset tasks and update finished tasks.
    app.run_system(Assets::update_assets);

    // ------- GAME WORLD ---------
    app.run_system(GameWorld::update_io);
    if app
        .resource_bank()
        .get_resource_mut::<GameWorld>()
        .try_tick()
    {
        // TICK UPDATES
    }

    // ------- PHYSICS ---------

    // Update player logic.
    app.run_system(Player::update);

    app.run_system(PhysicsWorld::do_physics_update);

    // ------- VOXEL WORLD -------
    app.run_system(VoxelCursor::update_post_physics);
    app.run_system(VoxelWorld::update_post_physics);

    // ------- UI ---------

    // Handle UI updates and Egui cpu-render.
    app.run_system(UI::update);
    app.run_system(UI::draw);

    // ------- GPU RENDERING ---------

    app.run_system(VoxelWorldGpu::update_gpu_objects);
    app.run_system(VoxelWorldGpu::write_render_data);

    app.run_system(Renderer::write_common_render_data);
    app.run_system(UIPass::write_debug_ui_render_data);

    // Only continue with frame graph pass writing if we successfully acquired the swapchain since
    // some images may rely on swapchain info.
    app.run_system(Renderer::acquire_swapchain_image);
    if app.get_resource::<Renderer>().did_acquire_swapchain() {
        app.run_system(UIPass::write_ui_pass);
        app.run_system(Renderer::finish_frame);
    }

    // ------- FRAME CLEANUP ---------

    // Discard any inputs and events cached for this frame.
    app.run_system(Input::clear_inputs);
    app.run_system(Events::clear_events);
    app.run_system(VoxelWorld::clear_state);

    //debug!(
    //    "CPU Frame took {}ms",
    //    cpu_time.elapsed().as_micros() as f32 / 1000.0
    //)
}
