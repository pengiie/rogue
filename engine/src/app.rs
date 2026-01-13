use std::{
    collections::HashMap,
    sync::mpsc::{channel, Receiver, Sender},
};

use winit::{
    application::ApplicationHandler, event::WindowEvent as WinitWindowEvent, event_loop::EventLoop,
};

use crate::graphics::{backend::GraphicsBackendEvent, camera::MainCamera, device::DeviceResource};
use crate::input::Input;
use crate::resource::{Res, ResMut, Resource, ResourceBank};
use crate::system::{System, SystemErased};
use crate::task::task_arbiter::TaskArbiter;
use crate::window::{time::Time, window::Window};
use crate::{
    asset::{
        asset::Assets,
        repr::{project::ProjectAsset, settings::UserSettingsAsset},
    },
    graphics::renderer::Renderer,
    material::material_gpu::MaterialBankGpu,
};
use crate::{audio::Audio, world::world_renderable::WorldRenderable};
use crate::{debug::DebugRenderer, entity::scripting::Scripts};
use crate::{
    event::{EventReader, Events},
    voxel::voxel_registry_gpu::VoxelModelRegistryGpu,
};
use crate::{game_loop, settings::Settings};

enum AppEvent {
    Init { device: DeviceResource },
}

pub struct AppCreateInfo {
    pub project: ProjectAsset,
    pub post_graphics_fn: Option<Box<dyn Fn(&mut ResourceBank)>>,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub enum AppStage {
    /// Runs during the physics update, where you should update velocities and apply forces
    /// which will be integrated the same physics update. You should use `Physics::curr_timestep`
    /// when calculting using delta time.
    FixedUpdate,

    /// Happens before any physics or rendering this frame. Where most logic should go.
    Update,

    /// Where you should write any render graph image or pass reference, as well as uniforms for
    /// shaders.
    RenderWrite,
}

pub struct App {
    event_loop: Option<EventLoop<()>>,

    initialized_window: bool,
    did_first_resize: bool,
    initialized_graphics: bool,
    graphics_event_reader: EventReader<GraphicsBackendEvent>,

    resource_bank: ResourceBank,
    systems: HashMap<AppStage, Vec<SystemErased>>,
    post_graphics_fn: Option<Box<dyn Fn(&mut ResourceBank)>>,

    event_sender: Sender<AppEvent>,
    event_receiver: Receiver<AppEvent>,
}

impl App {
    pub fn new(create_info: AppCreateInfo) -> Self {
        let event_loop = EventLoop::new().expect("Failed to create event loop");

        let (event_sender, event_receiver) = channel::<AppEvent>();

        let mut app = Self {
            event_loop: Some(event_loop),

            initialized_window: false,
            did_first_resize: false,
            initialized_graphics: false,
            graphics_event_reader: EventReader::new(),

            resource_bank: ResourceBank::new(),
            systems: HashMap::new(),
            post_graphics_fn: create_info.post_graphics_fn,

            event_sender,
            event_receiver,
        };

        app.insert_resource(TaskArbiter::new());
        app.insert_resource(Events::new());
        app.insert_resource(Scripts::new());
        app.insert_resource(Settings::from(&UserSettingsAsset::default()));
        app.insert_resource(Input::new());
        app.insert_resource(Time::new());
        app.insert_resource(Audio::new());
        app.insert_resource(MainCamera::new_empty());

        let project = create_info.project;
        app.insert_resource(project.ecs_world);
        app.insert_resource(project.voxel_registry);
        app.insert_resource(project.physics_world);
        app.insert_resource(project.material_bank);
        app.insert_resource(Assets::new(project.project_dir));

        app
    }

    /// Called after we get a valid DeviceResource graphics context, allowing us to setup resources that rely on that.
    fn init_post_graphics(&mut self) {
        let mut device_resource = self.get_resource_mut::<DeviceResource>();
        let renderer = Renderer::new(&mut device_resource);
        let voxel_registry_gpu = VoxelModelRegistryGpu::new(&mut device_resource);
        let world_renderable = WorldRenderable::new(&mut device_resource);
        drop(device_resource);
        self.insert_resource(renderer);
        self.insert_resource(MaterialBankGpu::new());

        self.insert_resource(DebugRenderer::new());
        self.insert_resource(world_renderable);
        self.insert_resource(voxel_registry_gpu);

        if let Some(init_fn) = &self.post_graphics_fn {
            (*init_fn)(&mut self.resource_bank);
        }
    }

    pub fn systems(&self, stage: AppStage) -> Option<&Vec<SystemErased>> {
        self.systems.get(&stage)
    }

    pub fn insert_system<Marker>(
        &mut self,
        stage: AppStage,
        system: impl System<Marker> + 'static,
    ) {
        self.systems
            .entry(stage)
            .or_insert_with(Vec::new)
            .push(SystemErased::new(system));
    }

    pub fn resource_bank(&self) -> &ResourceBank {
        &self.resource_bank
    }

    pub fn resource_bank_mut(&mut self) -> &mut ResourceBank {
        &mut self.resource_bank
    }

    pub fn get_resource<R: Resource>(&self) -> Res<R> {
        self.resource_bank.get_resource::<R>()
    }

    pub fn get_resource_mut<R: Resource>(&self) -> ResMut<R>
    where
        R: Resource,
    {
        self.resource_bank.get_resource_mut::<R>()
    }

    pub fn insert_resource<R: Resource>(&mut self, resource: R) {
        self.resource_bank.insert(resource);
    }

    pub fn run_with_window(mut self) {
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
        // If egui exists then input exists.
        //if self.resource_bank().has_resource::<Egui>() {
        //    let window = self.resource_bank().get_resource::<Window>();
        //    if !window.is_cursor_locked() {
        //        let egui_consumed = self
        //            .resource_bank()
        //            .get_resource_mut::<Egui>()
        //            .handle_window_event(&window, &event);
        //        if egui_consumed {
        //            return;
        //        }
        //    }
        //}
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

                if !self.initialized_graphics {
                    self.run_system(DeviceResource::pre_graphics_update);

                    let events = self.resource_bank.get_resource::<Events>();
                    for event in self.graphics_event_reader.read(&events) {
                        match event {
                            GraphicsBackendEvent::Initialized => {
                                self.initialized_graphics = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    drop(events);

                    // Graphics backend still isn't ready yet.
                    if !self.initialized_graphics {
                        return;
                    }

                    self.init_post_graphics();
                }

                game_loop::game_loop(self);

                self.resource_bank
                    .get_resource_mut::<Window>()
                    .finish_frame();
            }
            WinitWindowEvent::Resized(new_size) => {
                if !self.did_first_resize && new_size.width > 0 && new_size.height > 0 {
                    self.did_first_resize = true;

                    let mut gfx_device = DeviceResource::new();
                    gfx_device.init(
                        &self.resource_bank.get_resource::<Window>(),
                        &self.resource_bank.get_resource::<Settings>().graphics,
                    );
                    self.resource_bank.insert(gfx_device);
                }

                if self.initialized_graphics {
                    self.resource_bank()
                        .get_resource_mut::<DeviceResource>()
                        .resize_swapchain(new_size, false);

                    // TODO: Option to change between depending on window resize mode.
                    //self.resource_bank()
                    //    .get_resource_mut::<Settings>()
                    //    .graphics
                    //    .rt_size = Vector2::new(new_size.width, new_size.height);
                }
            }
            WinitWindowEvent::CloseRequested => {
                self.resource_bank()
                    .get_resource_mut::<Assets>()
                    .wait_until_all_saved();
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
        //if self.resource_bank().has_resource::<Input>() && self.resource_bank().has_resource::<UI>()
        //{
        //    let window = self.resource_bank().get_resource::<Window>();
        //    //let ui = self.resource_bank().get_resource::<UI>();
        //    //let editor = self.resource_bank().get_resource::<Editor>();
        //    let mouse_pos = input.mouse_position();
        //    let is_within_content = ui.content_padding.z <= mouse_pos.x
        //        && mouse_pos.x <= window.inner_size_vec2().x as f32 - ui.content_padding.w
        //        && ui.content_padding.x <= mouse_pos.y
        //        && mouse_pos.y <= window.inner_size_vec2().y as f32 - ui.content_padding.y;
        //    if !is_within_content {
        //        match &event {
        //            winit::event::DeviceEvent::Key(winit::event::RawKeyEvent {
        //                state: ElementState::Pressed,
        //                ..
        //            })
        //            | winit::event::DeviceEvent::MouseWheel { .. } => {
        //                return;
        //            }
        //            _ => {}
        //        }
        //    }

        //}
        let mut input = self.resource_bank().get_resource_mut::<Input>();
        input.handle_winit_device_event(device_id, event);
    }
}
