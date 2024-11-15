use std::sync::mpsc::{channel, Receiver, Sender};

use log::{debug, info};
use raw_window_handle::HasWindowHandle;
use winit::{
    application::ApplicationHandler, event::WindowEvent as WinitWindowEvent, event_loop::EventLoop,
};

use crate::{
    engine::{
        self,
        asset::asset::Assets,
        ecs::ecs_world::ECSWorld,
        event::Events,
        graphics::{
            device::DeviceResource, pipeline_manager::RenderPipelineManager, renderer::Renderer,
        },
        input::Input,
        physics::physics_world::PhysicsWorld,
        resource::ResourceBank,
        system::System,
        ui::{gui::Egui, state::UIState},
        voxel::voxel_world::{VoxelWorld, VoxelWorldGpu},
        window::{time::Time, window::Window},
    },
    game::{player::player::Player, world::game_world::GameWorld},
    game_loop,
    settings::Settings,
};

enum AppEvent {
    Init { device: DeviceResource },
}

pub struct App {
    event_loop: Option<EventLoop<()>>,

    initialized_window: bool,
    did_first_resize: bool,
    initialized_graphics: bool,
    resource_bank: ResourceBank,

    event_sender: Sender<AppEvent>,
    event_receiver: Receiver<AppEvent>,
}

impl App {
    pub fn new() -> Self {
        let event_loop = EventLoop::new().expect("Failed to create event loop");

        let (event_sender, event_receiver) = channel::<AppEvent>();

        Self {
            event_loop: Some(event_loop),

            initialized_window: false,
            did_first_resize: false,
            initialized_graphics: false,
            resource_bank: ResourceBank::new(),

            event_sender,
            event_receiver,
        }
    }

    pub fn resource_bank(&self) -> &ResourceBank {
        &self.resource_bank
    }

    pub fn resource_bank_mut(&mut self) -> &mut ResourceBank {
        &mut self.resource_bank
    }

    pub fn run(mut self) {
        let event_loop = self.event_loop.take().unwrap();
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                use winit::platform::web::{ActiveEventLoopExtWebSys, EventLoopExtWebSys};

                event_loop.spawn_app(self);
            } else {
                let _ = event_loop.run_app(&mut self);
            }
        }
    }

    pub fn run_system<Marker>(&self, mut system: impl System<Marker>) {
        system.run(self.resource_bank());
    }

    // Initialized after the window resize event, aka. after the window has been created and we
    // know it's actual size, the graphics context has yet to be created here.
    fn init_pre_graphics(&mut self) {
        let events = Events::new();
        let settings = Settings::default();
        let ecs = ECSWorld::new();
        let input = Input::new();
        let time = Time::new();
        let assets = Assets::new();
        let physics = PhysicsWorld::new();

        let rb = self.resource_bank_mut();
        rb.insert(events);
        rb.insert(settings);
        rb.insert(ecs);
        rb.insert(input);
        rb.insert(time);
        rb.insert(assets);
        rb.insert(physics);
    }

    fn init_post_graphics(&mut self) {
        let egui = Egui::new(&self.resource_bank.get_resource::<Window>());
        let ui_state = UIState::default();

        let rb = self.resource_bank_mut();
        rb.insert(ui_state);
        rb.insert(egui);

        engine::graphics::initialize_graphics_resources(rb);
        engine::voxel::initialize_voxel_world_resources(rb);
        // Game Stuff

        let game_world = GameWorld::new();
        rb.insert(game_world);

        self.run_system(Player::spawn_player);
    }
}

impl winit::application::ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !self.initialized_window {
            self.initialized_window = true;

            let window = Window::new(event_loop);
            let window_id = window.handle().id();
            let event = winit::event::WindowEvent::Resized(window.handle().inner_size());
            self.resource_bank_mut().insert(window);
            self.window_event(event_loop, window_id, event);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if self.resource_bank().has_resource::<Egui>() {
            let window = self.resource_bank().get_resource::<Window>();
            if self
                .resource_bank()
                .get_resource_mut::<Egui>()
                .handle_window_event(&window, &event)
            {
                return;
            }
        }
        if self.resource_bank().has_resource::<Input>() {
            self.resource_bank()
                .get_resource_mut::<Input>()
                .handle_winit_window_event(event.clone());
        }
        match event {
            WinitWindowEvent::RedrawRequested => {
                // We need to always request a redraw even with `ControlFlow::Poll` event though I
                // thought Poll always does this anyways.
                self.resource_bank
                    .get_resource::<Window>()
                    .handle()
                    .request_redraw();

                if let Ok(app_event) = self.event_receiver.try_recv() {
                    match app_event {
                        AppEvent::Init { device } => {
                            self.initialized_graphics = true;

                            self.resource_bank_mut().insert(device);
                            self.init_post_graphics();
                        }
                    }
                }

                if !self.initialized_graphics {
                    return;
                }

                game_loop::game_loop(self);

                self.resource_bank
                    .get_resource_mut::<Window>()
                    .finish_frame();
                self.resource_bank
                    .get_resource_mut::<DeviceResource>()
                    .finish_frame();
            }
            WinitWindowEvent::Resized(new_size) => {
                debug!("Window resized {:?}", new_size);
                if !self.did_first_resize && new_size.width > 0 && new_size.height > 0 {
                    self.did_first_resize = true;

                    self.init_pre_graphics();

                    let sender = self.event_sender.clone();
                    let window = self
                        .resource_bank()
                        .get_resource::<Window>()
                        .handle()
                        .clone();

                    let gfx_fut = async move {
                        let device = DeviceResource::init(window).await;
                        let _ = sender.send(AppEvent::Init { device });
                    };

                    cfg_if::cfg_if! {
                        if #[cfg(target_arch = "wasm32")] {
                            wasm_bindgen_futures::spawn_local(gfx_fut);
                        } else {
                            pollster::block_on(gfx_fut);
                        }
                    }
                }

                if self.initialized_graphics {
                    self.resource_bank()
                        .get_resource_mut::<DeviceResource>()
                        .resize_surface(new_size);

                    debug!("Resized window to {}x{}", new_size.width, new_size.height);
                    // TODO: Option to change between depending on window resize mode.
                    self.resource_bank()
                        .get_resource_mut::<Settings>()
                        .graphics
                        .render_size = (new_size.width, new_size.height);
                }
            }
            WinitWindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if self.resource_bank().has_resource::<Input>() {
            self.resource_bank()
                .get_resource_mut::<Input>()
                .handle_winit_device_event(device_id, event);
        }
    }
}
