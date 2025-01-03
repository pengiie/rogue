use std::{
    future::Future,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use log::{debug, info};
use rogue_macros::Resource;
use wgpu::{
    Backends, DeviceDescriptor, Features, InstanceDescriptor, Limits, SurfaceConfiguration,
};

use crate::{
    engine::{
        event::Events,
        resource::ResMut,
        window::window::{Window, WindowHandle},
    },
    settings::GraphicsSettings,
};

use super::{
    backend::{GfxSwapchainInfo, GraphicsBackendDevice},
    vulkan::device::{VulkanCreateInfo, VulkanDevice},
};

pub type GfxDevice = Box<dyn GraphicsBackendDevice>;

#[derive(Resource)]
pub struct DeviceResource {
    backend_device: Option<GfxDevice>,
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
        if new_size.width > 0 && new_size.height > 0 {
            if let Some(device) = self.backend_device.as_mut() {
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

    pub fn begin_frame(mut device: ResMut<DeviceResource>, mut events: ResMut<Events>) {
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
