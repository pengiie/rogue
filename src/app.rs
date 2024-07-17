use std::sync::mpsc::{channel, Receiver, Sender};

use log::info;
use winit::{event::WindowEvent as WinitWindowEvent, event_loop::EventLoop};

use crate::{
    engine::{
        ecs::ecs_world::ECSWorld,
        graphics::{device::DeviceResource, renderer::Renderer},
        input::Input,
        resource::ResourceBank,
        system::System,
        window::window::Window,
    },
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
                winit::platform::web::{ActiveEventLoopExtWebSys, EventLoopExtWebSys},
                event_loop.spawn_app(self);
            } else {
                event_loop.run_app(&mut self);
            }
        }
    }

    // Initialized after the window resize event, aka. after the window has been created and we
    // know it's actual size, the graphics context has yet to be created here.
    fn init_pre_graphics(&mut self) {
        let settings = Settings::default();
        let ecs = ECSWorld::new();
        let input = Input::new();

        let rb = self.resource_bank_mut();
        rb.insert(settings);
        rb.insert(ecs);
        rb.insert(input);
    }

    fn init_post_graphics(&mut self) {
        let renderer = Renderer::new(&self.resource_bank().get_resource::<DeviceResource>());

        let rb = self.resource_bank_mut();
        rb.insert(renderer);
    }
}

impl winit::application::ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !self.initialized_window {
            self.initialized_window = true;

            let window = Window::new(event_loop);
            self.resource_bank_mut().insert(window);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
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
            }
            WinitWindowEvent::Resized(new_size) => {
                if !self.did_first_resize {
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
                }
            }
            WinitWindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => {}
        }
    }
}
