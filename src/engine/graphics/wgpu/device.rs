use std::future::Future;

use log::{debug, info};

use crate::engine::{
    event::Events,
    graphics::backend::{GraphicsBackendDevice, GraphicsBackendEvent},
    window::window::{Window, WindowHandle},
};

pub struct WgpuDevice {
    ctx: Option<WgpuDeviceContext>,
    ctx_recv: std::sync::mpsc::Receiver<WgpuDeviceContext>,
}

struct WgpuDeviceContext {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    surface_resized: bool,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl WgpuDevice {
    fn new(window_handle: WindowHandle) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface_size = window_handle.inner_size();
        let surface = instance.create_surface(window_handle).unwrap();

        let (send, ctx_recv) = std::sync::mpsc::channel();

        let fut = async move {
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
                        memory_hints: wgpu::MemoryHints::default(),
                    },
                    None,
                )
                .await
                .expect("Couldn't get graphics device");
            device.on_uncaptured_error(Box::new(|err| log::error!("Wgpu Error: {:?}", err)));

            let limits = device.limits();
            debug!("Device limits are: {:?}", limits);

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
            info!("Surface format {:?}", surface_format);
            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface_format,
                width: surface_size.width,
                height: surface_size.height,
                present_mode: wgpu::PresentMode::AutoNoVsync,
                desired_maximum_frame_latency: 3,
                alpha_mode: surface_caps.alpha_modes[0],
                view_formats: vec![],
            };
            surface.configure(&device, &surface_config);

            WgpuDeviceContext {
                instance,
                surface,
                surface_resized: true,
                surface_config,
                device,
                queue,
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                wasm_bindgen_futures::spawn_local(fut);
            } else {
                pollster::block_on(fut);
            }
        }

        Self {
            ctx: None,
            ctx_recv,
        }
    }
}

impl GraphicsBackendDevice for WgpuDevice {
    fn pre_init_update(&mut self, events: &mut Events) {
        match self.ctx_recv.try_recv() {
            Ok(new_ctx) => {
                self.ctx.replace(new_ctx);
                events.push(GraphicsBackendEvent::Initialized);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                panic!("Failed to initialized wgpu backend, sender disconnected without sending context.");
            }
            _ => {}
        }
    }

    fn begin_frame(&mut self, events: &mut Events) {
        todo!()
    }

    fn create_frame_graph_executor(
        &mut self,
    ) -> Box<dyn crate::engine::graphics::backend::GraphicsBackendFrameGraphExecutor> {
        todo!()
    }

    fn register_compute_pipeline(
        &mut self,
        create_info: crate::engine::graphics::backend::ComputePipelineCreateInfo,
    ) -> crate::engine::graphics::backend::ResourceId<
        crate::engine::graphics::backend::ComputePipeline,
    > {
        todo!()
    }

    fn register_raster_pipeline(
        &mut self,
        create_info: crate::engine::graphics::backend::RasterPipelineCreateInfo,
    ) -> crate::engine::graphics::backend::ResourceId<
        crate::engine::graphics::backend::RasterPipeline,
    > {
        todo!()
    }

    fn create_image(
        &mut self,
        create_info: crate::engine::graphics::backend::ImageCreateInfo,
    ) -> crate::engine::graphics::backend::ResourceId<crate::engine::graphics::backend::Image> {
        todo!()
    }

    fn create_buffer(
        &mut self,
        create_info: crate::engine::graphics::backend::ImageCreateInfo,
    ) -> crate::engine::graphics::backend::ResourceId<crate::engine::graphics::backend::Image> {
        todo!()
    }

    fn end_frame(&mut self) {
        todo!()
    }

    fn update_pipelines(
        &mut self,
        shader_dictionary: &crate::engine::graphics::shader::ShaderCompiler,
    ) -> anyhow::Result<()> {
        todo!()
    }
}
