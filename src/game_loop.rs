use crate::{
    app::App,
    engine::{
        graphics::renderer::Renderer, input::Input, system::System, ui::UI, window::time::Time,
    },
    game::player::player::Player,
};

pub fn game_loop(app: &App) {
    app.run_system(Time::update);

    // Update physics logic.
    app.run_system(Player::update_player);

    // Handle UI.
    app.run_system(UI::update);
    app.run_system(UI::draw);

    // Render the frame to the swapchain.
    app.run_system(Renderer::write_render_data);
    app.run_system(Renderer::render);

    // Discard any inputs cached for this frame.
    app.run_system(Input::clear_inputs);
}
