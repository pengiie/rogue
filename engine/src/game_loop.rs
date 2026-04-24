use crate::animation::animation_bank::AnimationBank;
use crate::animation::animator::Animator;
use crate::app::{App, AppStage};
use crate::asset::asset::Assets;
use crate::audio::{Audio, AudioPlayer};
use crate::debug::debug_renderer::DebugRenderer;
use crate::entity::ecs_world::ECSWorld;
use crate::event::Events;
use crate::graphics::{device::DeviceResource, renderer::Renderer};
use crate::input::Input;
use crate::material::material_bank::MaterialBank;
use crate::material::material_gpu::MaterialBankGpu;
use crate::physics::physics_world::PhysicsWorld;
use crate::system::System;
use crate::voxel::baker_gpu::VoxelBakerGpu;
use crate::voxel::voxel_registry::VoxelModelRegistry;
use crate::voxel::voxel_registry_gpu::VoxelModelRegistryGpu;
use crate::window::time::Time;
use crate::world::sky::Sky;
use crate::world::terrain::region_map::RegionMap;
use crate::world::terrain::region_map_gpu::RegionMapGpu;
use crate::world::world_entities::WorldEntities;
use crate::world::world_entities_gpu::WorldEntitiesGpu;
use crate::world::world_streaming::WorldChunkStreamer;

pub fn game_loop(app: &App) {
    // This system is called in app.
    // DeviceResource::update

    // ------- FRAME SETUP ---------
    // Wait for any previous processing frames based off our timeline semaphores or capped
    // framerate.
    app.run_system(DeviceResource::begin_frame);
    // Update our time info such as delta time.
    app.run_system(Time::update);
    app.run_system(Input::collect_gamepad_events);

    // ------- ASSETS --------
    // Run any queued up asset tasks and update finished tasks.
    app.run_system(Assets::update);

    // -------- PHYSICS ----------
    // Do fixed-timestep physics updates for stability.
    let physics_updates = app
        .get_resource_mut::<PhysicsWorld>()
        .physics_update_count();
    for _ in 0..physics_updates {
        app.run_system(PhysicsWorld::start_time_step);

        // ------- APP-DEFINED FIXED-UPDATE SYSTEMS ------
        if let Some(systems) = app.systems(AppStage::FixedUpdate) {
            for system in systems {
                system.run(app.resource_bank());
            }
        }

        // Integrate velocities, mark collisions, and do collision resolution.
        app.run_system(PhysicsWorld::do_physics_update);
        app.run_system(PhysicsWorld::end_time_step);
    }
    app.run_system(PhysicsWorld::do_transform_interpolation);

    // ------- ANIMATION ---------
    app.run_system(Animator::update_animators_system);
    app.run_system(AnimationBank::update_loaded_animations);

    // ------- APP-DEFINED UPDATE SYSTEMS ------
    if let Some(systems) = app.systems(AppStage::Update) {
        for system in systems {
            system.run(app.resource_bank());
        }
    }

    // ------- AUDIO ---------
    app.run_system(AudioPlayer::on_update);
    app.run_system(Audio::on_update);

    // ------- ENTITIES ----------
    app.run_system(WorldEntities::load_entity_models);
    // Handle ECSWorld events.
    app.run_system(ECSWorld::handle_entity_commands);

    // ------- VOXEL REGISTRY -------
    // Load any entity renderables that have been requested to be loaded.
    app.run_system(VoxelModelRegistry::handle_model_load_events);
    app.run_system(VoxelModelRegistry::flush_out_events);

    // ------- SPATIAL WORLD -------

    // Updates the day/night cycle of the world.
    app.run_system(Sky::update_time);
    // Rendered terrain relative to player/camera anchor updating.
    app.run_system(WorldChunkStreamer::update);

    // Process any region map commands like clearing, saving, etc.
    app.run_system(RegionMap::update_process_commands);
    // Load any regions from disk into memory which have chunk data requested.
    app.run_system(RegionMap::update_region_loading);
    // Update from chunk commands and submits chunk events.
    app.run_system(RegionMap::update_chunks);
    // Update any terrain edits.
    app.run_system(RegionMap::update_region_edits);
    // Marks regions which should be written based off of region events.

    // ==============================================
    // ============== GPU RENDERING =================
    // ==============================================

    app.run_system(PhysicsWorld::render_debug_colliders);

    // Ensure we write this before the voxel world so materials can
    // be pointed to within the same frame.
    app.run_system(MaterialBank::update_material_loading);
    app.run_system(MaterialBank::update_events);
    app.run_system(MaterialBankGpu::write_render_data);

    // Requests the gpu voxel model representation for any used chunk models in the world.
    app.run_system(RegionMapGpu::update_gpu_chunk_models);
    app.run_system(WorldEntitiesGpu::write_render_data);

    // Allocates gpu voxel model data and invalidates any requested voxel model material data.
    app.run_system(VoxelModelRegistryGpu::write_render_data);

    // Write the gpu data used for terrain and entity rendering after gpu model ptrs are allocated.
    app.run_system(RegionMapGpu::write_render_data);

    // Write the debug renderer buffers.
    app.run_system(DebugRenderer::write_render_data);

    // Only continue with frame graph pass writing if we successfully acquired the swapchain since
    // some images may rely on swapchain info.
    app.run_system(Renderer::acquire_swapchain_image);
    if app.get_resource::<Renderer>().did_acquire_swapchain() {
        app.run_system(Renderer::begin_frame);

        if let Some(systems) = app.systems(AppStage::PreUniformsRenderWrite) {
            for system in systems {
                system.run(app.resource_bank());
            }
        }
        app.run_system(Renderer::write_frame_uniforms);

        app.run_system(VoxelBakerGpu::write_graph_passes);
        if let Some(systems) = app.systems(AppStage::RenderWrite) {
            for system in systems {
                system.run(app.resource_bank());
            }
        }

        app.run_system(Renderer::end_frame);
    }

    // ------- FRAME CLEANUP ---------

    // Discard any inputs and events cached for this frame.
    app.run_system(Input::clear_inputs);
    app.run_system(Events::frame_cleanup);
}
