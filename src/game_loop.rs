use crate::{
    app::App,
    engine::{graphics::renderer::Renderer, input::Input, system::System},
};

pub fn game_loop(app: &App) {
    // Render the frame to the swapchain.
    run_system(app, Renderer::write_render_data);
    run_system(app, Renderer::render);

    // Discard any inputs cached for this frame.
    run_system(app, Input::clear_inputs);
}

fn run_system<Marker>(app: &App, mut system: impl System<Marker>) {
    system.run(app.resource_bank());
}
