use std::{
    future::Future,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    rc::Rc,
    time::Duration,
};

use log::{debug, info};
use nalgebra::ComplexField;
use rogue_macros::Resource;
use wgpu::{
    Backends, DeviceDescriptor, Features, InstanceDescriptor, Limits, SurfaceConfiguration,
};

use crate::{
    engine::{
        event::Events,
        resource::{Res, ResMut},
        window::{
            time::Instant,
            window::{Window, WindowHandle},
        },
    },
    settings::{GraphicsSettings, Settings},
};

use super::{
    backend::{GfxSwapchainInfo, GraphicsBackendDevice},
    vulkan::device::{VulkanCreateInfo, VulkanDevice},
};

pub type GfxDevice = Box<dyn GraphicsBackendDevice>;

#[derive(Resource)]
pub struct DeviceResource {
    backend_device: Option<GfxDevice>,
    last_frame_time: Option<Instant>,
}

impl Deref for DeviceResource {
    type Target = Box<dyn GraphicsBackendDevice>;

    fn deref(&self) -> &Self::Target {
        self.backend_device.as_ref().unwrap()
    }
}

impl DerefMut for DeviceResource {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.backend_device.as_mut().unwrap()
    }
}

impl DeviceResource {
    pub fn new() -> Self {
        Self {
            last_frame_time: None,
            backend_device: None,
        }
    }

    pub fn init(&mut self, window: &Window, settings: &GraphicsSettings) {
        let device = if cfg!(target_arch = "wasm32") {
            unimplemented!("Wasm target not supported yet (if ever).");
        } else {
            VulkanDevice::init(VulkanCreateInfo {
                window,
                swapchain_info: GfxSwapchainInfo {
                    present_mode: settings.present_mode,
                    triple_buffering: settings.triple_buffering,
                },
                enable_debug: true,
            })
            .expect("Failed to create Vulkan device.")
        };
        self.backend_device = Some(Box::new(device));
    }

    pub fn resize_swapchain(&mut self, new_size: winit::dpi::PhysicalSize<u32>, skip_frame: bool) {
        if let Some(device) = self.backend_device.as_mut() {
            let old_size = device.swapchain_size();
            if new_size.width > 0
                && new_size.height > 0
                && (old_size.x != new_size.width || old_size.y != new_size.height)
            {
                debug!(
                    "Resizing swapchain to {}x{}",
                    new_size.width, new_size.height
                );
                device.resize_swapchain(
                    winit::dpi::PhysicalSize {
                        width: NonZeroU32::new(new_size.width).unwrap(),
                        height: NonZeroU32::new(new_size.height).unwrap(),
                    },
                    skip_frame,
                );
            }
        }
    }

    // Systems
    pub fn pre_init_update(mut device: ResMut<DeviceResource>, mut events: ResMut<Events>) {
        if let Some(device) = device.backend_device.as_mut() {
            device.pre_init_update(&mut events);
        }
    }

    pub fn begin_frame(
        mut device: ResMut<DeviceResource>,
        mut events: ResMut<Events>,
        settings: Res<Settings>,
    ) {
        // Cap framerate.
        if let Some(last_frame_time) = device.last_frame_time {
            let elapsed_time_us = last_frame_time.elapsed().as_micros();
            let minimum_wait_time_us =
                ((1.0 / settings.frame_rate_cap as f64) * 1_000_000.0).floor() as u128;
            if elapsed_time_us < minimum_wait_time_us {
                std::thread::sleep(Duration::from_micros(
                    (minimum_wait_time_us - elapsed_time_us) as u64,
                ));
            }
        }
        device.last_frame_time = Some(Instant::now());

        device
            .backend_device
            .as_mut()
            .unwrap()
            .begin_frame(&mut events);
    }

    pub fn end_frame(mut device: ResMut<DeviceResource>) {
        device.backend_device.as_mut().unwrap().end_frame();
    }
}
