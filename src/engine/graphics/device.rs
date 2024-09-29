use std::{
    future::Future,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use log::debug;
use rogue_macros::Resource;
use wgpu::{
    Backends, DeviceDescriptor, Features, InstanceDescriptor, Limits, SurfaceConfiguration,
};

use crate::engine::window::window::{Window, WindowHandle};

#[derive(Resource)]
pub struct DeviceResource {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    surface_resized: bool,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Deref for DeviceResource {
    type Target = wgpu::Device;

    fn deref(&self) -> &Self::Target {
        &self.device
    }
}

impl DerefMut for DeviceResource {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.device
    }
}

impl DeviceResource {
    pub fn init(window: WindowHandle) -> impl Future<Output = Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..Default::default()
        });

        let surface_size = window.inner_size();
        let surface = instance.create_surface(window).unwrap();

        debug!(
            "Surface width: {:?}, height: {:?}",
            surface_size.width, surface_size.height
        );
        async move {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .expect("Couldn't find adapter to initialize graphics.");

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("rogue_device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits {
                            max_buffer_size: 1 << 30,
                            max_storage_buffer_binding_size: 1 << 30,
                            ..Default::default()
                        },
                    },
                    None,
                )
                .await
                .expect("Couldn't get graphics device");

            let surface_caps = surface.get_capabilities(&adapter);
            let surface_format = *surface_caps
                .formats
                .iter()
                .find(|format| {
                    matches!(
                        format,
                        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
                    )
                })
                .expect("Couldn't find compatible surface format");
            println!("Surface format {:?}", surface_format);
            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface_format,
                width: surface_size.width,
                height: surface_size.height,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 3,
                alpha_mode: surface_caps.alpha_modes[0],
                view_formats: vec![],
            };
            surface.configure(&device, &surface_config);

            Self {
                instance,
                surface,
                surface_resized: true,
                surface_config,
                device,
                queue,
            }
        }
    }

    pub fn finish_frame(&mut self) {
        self.surface_resized = false;
    }

    pub fn did_surface_resize(&self) -> bool {
        self.surface_resized
    }

    pub fn resize_surface(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
            self.surface_resized = true;
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn device_mut(&mut self) -> &mut wgpu::Device {
        &mut self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn surface(&self) -> &wgpu::Surface {
        &self.surface
    }

    pub fn surface_mut(&mut self) -> &mut wgpu::Surface<'static> {
        &mut self.surface
    }

    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    pub fn surface_config(&self) -> &wgpu::SurfaceConfiguration {
        &self.surface_config
    }
}
