use std::{
    borrow::BorrowMut,
    collections::HashMap,
    ffi::CString,
    num::NonZeroU32,
    sync::{
        atomic::{AtomicU32, AtomicU64},
        Arc,
    },
    u64,
};

use anyhow::Context;
use ash::vk::{QueueFlags, SemaphoreType};
use log::{debug, warn};
use nalgebra::Vector2;
use raw_window_handle::{HasDisplayHandle, HasRawWindowHandle, HasWindowHandle};

use crate::engine::{
    event::Events,
    graphics::{
        backend::{
            Buffer, ComputePipeline, ComputePipelineCreateInfo, GfxBufferCreateInfo, GfxFilterMode,
            GfxImageCreateInfo, GfxImageInfo, GfxImageType, GfxPresentMode, GfxSwapchainInfo,
            GraphicsBackendDevice, GraphicsBackendEvent, GraphicsBackendFrameGraphExecutor, Image,
            ImageFormat, Memory, RasterPipeline, RasterPipelineCreateInfo, ResourceId, Untyped,
        },
        gpu_allocator::{Allocation, AllocatorTree},
        shader::ShaderCompiler,
    },
    window::window::{Window, WindowHandle},
};

use super::{executor::VulkanFrameGraphExecutor, pipeline_manager::VulkanPipelineManager};

pub struct VulkanContextInner {
    entry: ash::Entry,
    instance: ash::Instance,
    debug_messenger: Option<ash::vk::DebugUtilsMessengerEXT>,
    surface: ash::vk::SurfaceKHR,
    physical_device: VulkanPhysicalDevice,
    device: ash::Device,
}

impl Drop for VulkanContextInner {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().unwrap();

            self.device.destroy_device(None);
        };

        let surface_loader = ash::khr::surface::Instance::new(&self.entry, &self.instance);
        unsafe { surface_loader.destroy_surface(self.surface, None) };

        if let Some(debug_messenger) = self.debug_messenger {
            let debug_utils_loader =
                ash::ext::debug_utils::Instance::new(&self.entry, &self.instance);
            unsafe {
                debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
            }
        }

        unsafe {
            self.instance.destroy_instance(None);
        };
    }
}

pub struct VulkanContext {
    inner: Arc<VulkanContextInner>,
    swapchain: parking_lot::RwLock<Arc<VulkanSwapchain>>,
    main_queue: ash::vk::Queue,
    main_queue_family_index: u32,

    // The number of frames the cpu and gpu can process before waiting.
    frames_in_flight: u32,

    /// The current frame the cpu is working on.
    current_cpu_frame: AtomicU64,
    /// The current frame the gpu is working on.
    gpu_timeline_semaphore: ash::vk::Semaphore,

    // Semaphore when the swapchain image is acquired.
    image_acquire_semaphores: Vec<ash::vk::Semaphore>,
    // Semaphore when the swapchain image is finished being rendered to.
    image_ready_semaphores: Vec<ash::vk::Semaphore>,

    // The current swapchain image index of the most recently acquired image.
    swapchain_image_index: AtomicU32,

    memory_allocator: parking_lot::RwLock<VulkanAllocator>,
    resource_manager: VulkanResourceManager,
}

impl VulkanContext {
    pub fn device(&self) -> ash::Device {
        self.inner.device.clone()
    }

    pub fn swapchain(&self) -> parking_lot::RwLockReadGuard<Arc<VulkanSwapchain>> {
        self.swapchain.read()
    }

    pub fn surface(&self) -> ash::vk::SurfaceKHR {
        self.inner.surface.clone()
    }

    pub fn frames_in_flight(&self) -> u32 {
        self.frames_in_flight
    }

    pub fn surface_loader(&self) -> ash::khr::surface::Instance {
        ash::khr::surface::Instance::new(&self.inner.entry, &self.inner.instance)
    }

    pub fn swapchain_loader(&self) -> ash::khr::swapchain::Device {
        ash::khr::swapchain::Device::new(&self.inner.instance, &self.inner.device)
    }

    pub fn debug_utils_loader(&self) -> ash::ext::debug_utils::Instance {
        ash::ext::debug_utils::Instance::new(&self.inner.entry, &self.inner.instance)
    }

    pub fn resource_manager(&self) -> &VulkanResourceManager {
        &self.resource_manager
    }

    pub fn curr_cpu_frame(&self) -> u64 {
        self.current_cpu_frame
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn curr_gpu_frame(&self) -> u64 {
        unsafe {
            self.device()
                .get_semaphore_counter_value(self.gpu_timeline_semaphore())
        }
        .expect("Failed to get gpu timeline semaphore.")
    }

    pub fn curr_cpu_frame_index(&self) -> u32 {
        (self.curr_cpu_frame() % self.frames_in_flight() as u64) as u32
    }

    pub fn curr_image_acquire_semaphore(&self) -> ash::vk::Semaphore {
        self.image_acquire_semaphores[self.curr_cpu_frame_index() as usize]
    }

    pub fn curr_image_ready_semaphore(&self) -> ash::vk::Semaphore {
        self.image_ready_semaphores[self.curr_cpu_frame_index() as usize]
    }

    pub fn gpu_timeline_semaphore(&self) -> ash::vk::Semaphore {
        self.gpu_timeline_semaphore
    }

    pub fn main_queue(&self) -> ash::vk::Queue {
        self.main_queue
    }

    pub fn curr_swapchain_image_index(&self) -> u32 {
        self.swapchain_image_index
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn create_image(
        &self,
        create_info: GfxImageCreateInfo,
    ) -> anyhow::Result<ResourceId<Image>> {
        let mut memory_allocator = self.memory_allocator.write();
        self.resource_manager
            .create_image(&mut memory_allocator, create_info)
    }

    pub fn create_buffer(
        &self,
        create_info: GfxBufferCreateInfo,
    ) -> anyhow::Result<ResourceId<Buffer>> {
        let mut memory_allocator = self.memory_allocator.write();
        self.resource_manager
            .create_buffer(&mut memory_allocator, create_info)
    }
}

pub type VulkanContextHandle = Arc<VulkanContext>;
pub struct VulkanDevice {
    context: VulkanContextHandle,
    pipeline_manager: VulkanPipelineManager,

    // Size is # of frames in flight (swapchain images).
    destroy_frame_queue: Vec<Vec<ResourceId<Untyped>>>,
}

pub struct VulkanSwapchain {
    ctx_ref: Arc<VulkanContextInner>,
    pub swapchain: ash::vk::SwapchainKHR,
    swapchain_images: Vec<ResourceId<Image>>,
}

pub struct VulkanPhysicalDevice {
    physical_device: ash::vk::PhysicalDevice,
    properties: ash::vk::PhysicalDeviceProperties,
    memory_properties: ash::vk::PhysicalDeviceMemoryProperties,
    queue_family_properties: Vec<ash::vk::QueueFamilyProperties>,
    features: ash::vk::PhysicalDeviceFeatures,
}

pub struct VulkanCreateInfo<'a> {
    pub window: &'a Window,
    pub swapchain_info: GfxSwapchainInfo,
    pub enable_debug: bool,
}

impl VulkanDevice {
    pub fn init(
        VulkanCreateInfo {
            window,
            swapchain_info,
            enable_debug,
        }: VulkanCreateInfo,
    ) -> anyhow::Result<Self> {
        let entry = unsafe { ash::Entry::load() }?;

        let instance = {
            let application_name = std::ffi::CString::new("Rogue").unwrap();
            let application_info = ash::vk::ApplicationInfo::default()
                .engine_name(&application_name)
                .application_name(&application_name)
                .api_version(ash::vk::API_VERSION_1_3);

            let enabled_layers_cstrs = Self::get_required_layer_names();
            let enabled_layers_ptrs = enabled_layers_cstrs
                .iter()
                .map(|cstr| cstr.as_ptr())
                .collect::<Vec<_>>();

            let enabled_extensions_cstrs: Vec<CString> = vec![];
            let mut enabled_extensions_ptrs = enabled_extensions_cstrs
                .iter()
                .map(|cstr| cstr.as_ptr())
                .collect::<Vec<_>>();
            enabled_extensions_ptrs.extend_from_slice(ash_window::enumerate_required_extensions(
                window.display_handle()?.as_raw(),
            )?);
            if enable_debug {
                enabled_extensions_ptrs.push(ash::ext::debug_utils::NAME.as_ptr());
            }

            let instance_create_info = ash::vk::InstanceCreateInfo::default()
                .application_info(&application_info)
                .enabled_layer_names(&enabled_layers_ptrs)
                .enabled_extension_names(&enabled_extensions_ptrs);

            unsafe { entry.create_instance(&instance_create_info, None) }?
        };

        // Setup debug_utils.
        let debug_messenger = if enable_debug {
            let messenger_create_info = ash::vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    ash::vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | ash::vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | ash::vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
                )
                .message_type(
                    ash::vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | ash::vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | ash::vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(Self::debug_utils_callback));

            let debug_utils_loader = ash::ext::debug_utils::Instance::new(&entry, &instance);
            Some(unsafe {
                debug_utils_loader.create_debug_utils_messenger(&messenger_create_info, None)
            }?)
        } else {
            None
        };

        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
        let surface = unsafe {
            ash_window::create_surface(
                &entry,
                &instance,
                window.display_handle()?.as_raw(),
                window.window_handle()?.as_raw(),
                None,
            )
        }?;

        let physical_device = {
            let mut devices = unsafe { instance.enumerate_physical_devices() }?
                .into_iter()
                .filter_map(|physical_device| {
                    let properties =
                        unsafe { instance.get_physical_device_properties(physical_device) };
                    let features =
                        unsafe { instance.get_physical_device_features(physical_device) };
                    let memory_properties =
                        unsafe { instance.get_physical_device_memory_properties(physical_device) };
                    let queue_family_properties = unsafe {
                        instance.get_physical_device_queue_family_properties(physical_device)
                    };

                    let mut has_surface_support = false;
                    for i in 0..queue_family_properties.len() {
                        if unsafe {
                            surface_loader
                                .get_physical_device_surface_support(
                                    physical_device,
                                    i as u32,
                                    surface,
                                )
                                .unwrap_or(false)
                        } {
                            has_surface_support = true;
                            break;
                        }
                    }
                    if !has_surface_support {
                        return None;
                    }

                    Some(VulkanPhysicalDevice {
                        physical_device,
                        properties,
                        memory_properties,
                        queue_family_properties,
                        features,
                    })
                })
                .collect::<Vec<_>>();
            let mut scored_devices = devices
                .iter()
                .enumerate()
                .map(|(i, physical_device)| {
                    let mut score = 0;

                    if physical_device.properties.device_type
                        == ash::vk::PhysicalDeviceType::DISCRETE_GPU
                    {
                        score += 10_000;
                    }

                    (i, score)
                })
                .collect::<Vec<_>>();
            scored_devices.sort_by(|(_, score_a), (_, score_b)| score_a.cmp(score_b));

            anyhow::ensure!(
                !devices.is_empty(),
                "No physical device exists, can't pick a physical device."
            );

            // Takes the highest scored device.
            let (_high_score, picked_device_index) = scored_devices.first().unwrap();
            devices.swap_remove(*picked_device_index)
        };

        let main_queue_family_index = physical_device
            .queue_family_properties
            .iter()
            .enumerate()
            .find_map(|(i, properties)| {
                if properties.queue_flags.contains(
                    ash::vk::QueueFlags::GRAPHICS
                        | ash::vk::QueueFlags::COMPUTE
                        | ash::vk::QueueFlags::TRANSFER,
                ) {
                    return Some(i as u32);
                }

                None
            })
            .context("Failed to find a suitable queue family")?;

        let device = {
            let enabled_extensions_ptrs = vec![
                ash::khr::swapchain::NAME.as_ptr(),
                ash::khr::synchronization2::NAME.as_ptr(),
            ];

            let mut feature_timeline_semaphore =
                ash::vk::PhysicalDeviceTimelineSemaphoreFeatures::default()
                    .timeline_semaphore(true);
            let mut feature_synchronization2 =
                ash::vk::PhysicalDeviceSynchronization2Features::default().synchronization2(true);
            feature_synchronization2.p_next =
                std::ptr::from_mut(&mut feature_timeline_semaphore) as *mut std::ffi::c_void;

            let mut features2 = ash::vk::PhysicalDeviceFeatures2::default()
                .push_next(&mut feature_synchronization2);
            unsafe {
                instance
                    .get_physical_device_features2(physical_device.physical_device, &mut features2)
            };

            let main_queue_priorities = [1.0];
            let queue_create_infos = [ash::vk::DeviceQueueCreateInfo::default()
                .queue_priorities(&main_queue_priorities)
                .queue_family_index(main_queue_family_index)];

            let device_create_info = ash::vk::DeviceCreateInfo::default()
                .push_next(&mut features2)
                .enabled_extension_names(&enabled_extensions_ptrs)
                .queue_create_infos(&queue_create_infos);

            unsafe {
                instance.create_device(physical_device.physical_device, &device_create_info, None)
            }?
        };

        let main_queue = unsafe { device.get_device_queue(main_queue_family_index, 0) };

        let (
            swapchain,
            swapchain_images,
            swapchain_format,
            swapchain_extent,
            swapchain_image_usage,
        ) = {
            let surface_capabilities = unsafe {
                surface_loader.get_physical_device_surface_capabilities(
                    physical_device.physical_device,
                    surface,
                )
            }?;
            let present_mode = Self::get_optimal_present_mode(
                &surface_loader,
                &surface,
                &physical_device,
                &swapchain_info.present_mode,
            )?;
            let swapchain_format =
                Self::get_optimal_swapchain_format(&surface_loader, &surface, &physical_device)?;

            let min_image_count = swapchain_info
                .triple_buffering
                .then_some(3)
                .unwrap_or(2)
                .clamp(
                    surface_capabilities.min_image_count,
                    surface_capabilities.max_image_count,
                );
            let swapchain_extent = ash::vk::Extent2D {
                width: window.width(),
                height: window.height(),
            };
            let swapchain_image_usage = ash::vk::ImageUsageFlags::TRANSFER_DST;

            let swapchain_create_info = ash::vk::SwapchainCreateInfoKHR::default()
                .surface(surface)
                .min_image_count(min_image_count)
                .image_format(swapchain_format.format)
                .image_color_space(swapchain_format.color_space)
                .image_extent(swapchain_extent)
                .image_array_layers(1)
                .image_sharing_mode(ash::vk::SharingMode::EXCLUSIVE)
                .image_usage(swapchain_image_usage)
                .composite_alpha(ash::vk::CompositeAlphaFlagsKHR::OPAQUE)
                .pre_transform(ash::vk::SurfaceTransformFlagsKHR::IDENTITY)
                .present_mode(present_mode);

            let swapchain_loader = ash::khr::swapchain::Device::new(&instance, &device);
            let swapchain =
                unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None) }?;
            let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain) }?;
            (
                swapchain,
                swapchain_images,
                swapchain_format,
                swapchain_extent,
                swapchain_image_usage,
            )
        };

        let timeline_semaphore = {
            let mut timeline_create_info = ash::vk::SemaphoreTypeCreateInfo::default()
                .semaphore_type(SemaphoreType::TIMELINE_KHR)
                .initial_value(0);
            let create_info =
                ash::vk::SemaphoreCreateInfo::default().push_next(&mut timeline_create_info);
            unsafe { device.create_semaphore(&create_info, None) }?
        };

        let frames_in_flight = 2;
        let image_acquire_semaphores = (0..frames_in_flight)
            .map(|_| {
                unsafe { device.create_semaphore(&ash::vk::SemaphoreCreateInfo::default(), None) }
                    .expect("Failed to create image acquire semaphore")
            })
            .collect::<Vec<_>>();
        let image_ready_semaphores = (0..frames_in_flight)
            .map(|_| {
                unsafe { device.create_semaphore(&ash::vk::SemaphoreCreateInfo::default(), None) }
                    .expect("Failed to create image ready semaphore")
            })
            .collect::<Vec<_>>();

        let context_inner = Arc::new(VulkanContextInner {
            entry,
            instance,
            debug_messenger,
            surface,
            physical_device,
            device,
        });
        let resource_manager = VulkanResourceManager::new(&context_inner);
        let swapchain_images = swapchain_images
            .into_iter()
            .map(|image| {
                resource_manager
                    .create_image_borrowed(VulkanBorrowedImageCreateInfo {
                        image,
                        usage: swapchain_image_usage,
                        info: VulkanImageInfo {
                            image_type: GfxImageType::D2,
                            format: swapchain_format.format,
                            extent: swapchain_extent,
                        },
                    })
                    .expect("Failed to create swapchain image")
            })
            .collect::<Vec<_>>();
        let swapchain = Arc::new(VulkanSwapchain {
            ctx_ref: context_inner.clone(),
            swapchain,
            swapchain_images,
        });

        let context = Arc::new(VulkanContext {
            inner: context_inner.clone(),
            swapchain: parking_lot::RwLock::new(swapchain),
            main_queue_family_index,
            main_queue,

            swapchain_image_index: AtomicU32::new(0),

            frames_in_flight,
            // One since `gpu_timeline_semaphore` starts signalling 0.
            current_cpu_frame: AtomicU64::new(1),
            gpu_timeline_semaphore: timeline_semaphore,

            image_acquire_semaphores,
            image_ready_semaphores,

            memory_allocator: parking_lot::RwLock::new(VulkanAllocator::new(&context_inner)),
            resource_manager,
        });

        let pipeline_manager = VulkanPipelineManager::new();
        Ok(VulkanDevice {
            context,

            pipeline_manager,

            destroy_frame_queue: (0..frames_in_flight).map(|_| Vec::new()).collect(),
        })
    }

    fn get_required_layer_names() -> Vec<CString> {
        vec![std::ffi::CString::new("VK_LAYER_KHRONOS_validation").unwrap()]
    }

    fn get_optimal_present_mode(
        surface_loader: &ash::khr::surface::Instance,
        surface: &ash::vk::SurfaceKHR,
        physical_device: &VulkanPhysicalDevice,
        requested_present_mode: &GfxPresentMode,
    ) -> anyhow::Result<ash::vk::PresentModeKHR> {
        let supported_modes = unsafe {
            surface_loader.get_physical_device_surface_present_modes(
                physical_device.physical_device,
                *surface,
            )
        }?;
        match requested_present_mode {
            GfxPresentMode::NoVsync => {
                if supported_modes.contains(&ash::vk::PresentModeKHR::IMMEDIATE) {
                    return Ok(ash::vk::PresentModeKHR::IMMEDIATE);
                }
            }
            GfxPresentMode::Vsync => {
                if supported_modes.contains(&ash::vk::PresentModeKHR::MAILBOX) {
                    return Ok(ash::vk::PresentModeKHR::MAILBOX);
                }
            }
        }

        // Only present mode to be guaranteed support.
        return Ok(ash::vk::PresentModeKHR::FIFO);
    }

    fn get_optimal_swapchain_format(
        surface_loader: &ash::khr::surface::Instance,
        surface: &ash::vk::SurfaceKHR,
        physical_device: &VulkanPhysicalDevice,
    ) -> anyhow::Result<ash::vk::SurfaceFormatKHR> {
        let available_formats = unsafe {
            surface_loader
                .get_physical_device_surface_formats(physical_device.physical_device, *surface)
        }?;

        available_formats
            .into_iter()
            .find(|format| {
                if format.color_space != ash::vk::ColorSpaceKHR::SRGB_NONLINEAR {
                    return false;
                }

                matches!(
                    format.format,
                    ash::vk::Format::R8G8B8A8_UNORM | ash::vk::Format::B8G8R8A8_UNORM
                )
            })
            .context("Couldn't find a supported backbuffer format.")
    }

    unsafe extern "system" fn debug_utils_callback(
        message_severity: ash::vk::DebugUtilsMessageSeverityFlagsEXT,
        message_type: ash::vk::DebugUtilsMessageTypeFlagsEXT,
        p_callback_data: *const ash::vk::DebugUtilsMessengerCallbackDataEXT,
        _p_user_data: *mut std::ffi::c_void,
    ) -> ash::vk::Bool32 {
        let message = std::ffi::CStr::from_ptr((*p_callback_data).p_message);
        let ty = format!("{:?}", message_type).to_lowercase();
        let message = format!("[{}] {:?}", ty, message);

        match message_severity {
            ash::vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => log::error!("{}", message),
            ash::vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => log::warn!("{}", message),
            _ => log::debug!("{}", message),
        }

        ash::vk::FALSE
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            self.device().device_wait_idle().unwrap();
            for semaphore in self
                .image_ready_semaphores
                .iter()
                .chain(&self.image_acquire_semaphores)
                .chain(std::slice::from_ref(&self.gpu_timeline_semaphore))
            {
                self.device().destroy_semaphore(*semaphore, None);
            }
        }
    }
}

impl Drop for VulkanSwapchain {
    fn drop(&mut self) {
        let swapchain_loader =
            ash::khr::swapchain::Device::new(&self.ctx_ref.instance, &self.ctx_ref.device);
        unsafe { swapchain_loader.destroy_swapchain(self.swapchain, None) };
    }
}

impl GraphicsBackendDevice for VulkanDevice {
    fn begin_frame(&mut self, events: &mut Events) {
        // Equals n - 1, where n is the current cpu frame.
        let prev_cpu_frame = self
            .context
            .current_cpu_frame
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Wait for gpu timeline semaphore for n - (f - 1), where f is the # of frames in flight.
        let fif_minus_one = self.context.frames_in_flight as u64 - 1;
        if prev_cpu_frame >= fif_minus_one {
            let wait_gpu_frame = prev_cpu_frame - fif_minus_one;
            let wait_info = ash::vk::SemaphoreWaitInfo::default()
                .semaphores(std::slice::from_ref(&self.context.gpu_timeline_semaphore))
                .values(std::slice::from_ref(&wait_gpu_frame));

            // TODO: Worry about timeout.
            unsafe { self.context.device().wait_semaphores(&wait_info, u64::MAX) };
        }
    }

    fn create_frame_graph_executor(&mut self) -> Box<dyn GraphicsBackendFrameGraphExecutor> {
        Box::new(VulkanFrameGraphExecutor::new(&self.context))
    }

    fn register_compute_pipeline(
        &mut self,
        create_info: ComputePipelineCreateInfo,
    ) -> ResourceId<ComputePipeline> {
        todo!()
    }

    fn register_raster_pipeline(
        &mut self,
        create_info: RasterPipelineCreateInfo,
    ) -> ResourceId<RasterPipeline> {
        todo!()
    }

    fn create_image(&mut self, create_info: GfxImageCreateInfo) -> ResourceId<Image> {
        self.context.create_image(create_info).unwrap()
    }

    fn get_image_info(&self, image: &ResourceId<Image>) -> GfxImageInfo {
        todo!("get image info")
    }

    fn create_buffer(&mut self, create_info: GfxBufferCreateInfo) -> ResourceId<Buffer> {
        self.context.create_buffer(create_info).unwrap()
    }

    fn write_buffer(&mut self, buffer: &ResourceId<Buffer>, offset: u64, bytes: &[u8]) {
        todo!()
    }

    fn end_frame(&mut self) {
        todo!()
    }

    fn update_pipelines(&mut self, shader_compiler: &ShaderCompiler) -> anyhow::Result<()> {
        self.update_pipelines(shader_compiler)
    }

    fn acquire_swapchain_image(&mut self) -> anyhow::Result<ResourceId<Image>> {
        let swapchain_loader = ash::khr::swapchain::Device::new(
            &self.context.inner.instance,
            &self.context.inner.device,
        );
        let (image_index, out_of_date) = unsafe {
            swapchain_loader.acquire_next_image(
                self.context.swapchain().swapchain,
                u64::MAX,
                self.context.curr_image_acquire_semaphore(),
                ash::vk::Fence::null(),
            )
        }?;
        self.context
            .swapchain_image_index
            .store(image_index, std::sync::atomic::Ordering::Relaxed);
        if out_of_date {
            warn!("Swapchain image is out of date");
        }

        let image_resource_id = *self
            .context
            .swapchain()
            .swapchain_images
            .get(image_index as usize)
            .unwrap();
        Ok(image_resource_id)
    }

    fn resize_swapchain(&mut self, new_size: winit::dpi::PhysicalSize<NonZeroU32>) {
        unsafe {
            self.context
                .device()
                .queue_wait_idle(self.context.main_queue)
        };
        todo!("resize swapchain")
    }

    fn pre_init_update(&mut self, events: &mut Events) {
        // We initialize synchronously.
        events.push(GraphicsBackendEvent::Initialized);
    }
}

struct VulkanAllocator {
    ctx: Arc<VulkanContextInner>,
    shared_memory: Vec<VulkanSharedMemory>,
    dedicated_memory: Vec<VulkanMemory>,
}

struct VulkanSharedMemory {
    memory: VulkanMemory,
    memory_property_flags: ash::vk::MemoryPropertyFlags,
    allocator: AllocatorTree,

    free_size_remaining: u64,
    active_allocations: u64,
}

#[derive(Clone)]
struct VulkanMemory {
    device_memory: ash::vk::DeviceMemory,
    /// Size of the entire `ash::vk::DeviceMemory` allocated.
    size: u64,
}

type VulkanMemoryIndex = u64;
struct VulkanMemoryAllocation {
    memory_index: VulkanMemoryIndex,
    // If this is `Some`, then this points to a shared allocation; else, this points to a dedicated
    // allocation.
    traversal: Option<Allocation>,
}

struct VulkanAllocationInfo {
    memory: VulkanMemory,
    offset: u64,
}

enum VulkanAllocationType {
    GpuLocal,
    GpuLocalDedicated,
    CpuLocal,
    CpuLocalDedicated,
}

impl VulkanAllocator {
    const SHARED_MEMORY_CHUNK_SIZE: u64 = 1 << 23; // 8.38 Mib

    fn new(ctx: &Arc<VulkanContextInner>) -> Self {
        Self {
            ctx: ctx.clone(),
            shared_memory: Vec::new(),
            dedicated_memory: Vec::new(),
        }
    }

    fn allocate_device_memory(
        &self,
        size: u64,
        memory_property_flags: ash::vk::MemoryPropertyFlags,
    ) -> anyhow::Result<(VulkanMemory, ash::vk::MemoryType)> {
        let (memory_type_index, memory_type) = self
            .ctx
            .physical_device
            .memory_properties
            .memory_types
            .iter()
            .enumerate()
            .find(|(i, memory_type)| memory_type.property_flags.intersects(memory_property_flags))
            .context("Failed to find a suitable memory type")?;

        let allocation_info = ash::vk::MemoryAllocateInfo::default()
            .allocation_size(size)
            .memory_type_index(memory_type_index as u32);
        let device_memory = unsafe { self.ctx.device.allocate_memory(&allocation_info, None) }?;

        Ok((
            VulkanMemory {
                device_memory,
                size,
            },
            *memory_type,
        ))
    }

    fn find_or_create_shared_memory(
        &mut self,
        allocation_size: u64,
        memory_property_flags: ash::vk::MemoryPropertyFlags,
    ) -> anyhow::Result<VulkanMemoryIndex> {
        // Check for available shared memory with the sample memory properties.
        for (memory_index, shared_memory) in self.shared_memory.iter().enumerate() {
            if shared_memory
                .memory_property_flags
                .intersects(memory_property_flags)
                && shared_memory.free_size_remaining >= allocation_size
            {
                return Ok(memory_index as VulkanMemoryIndex);
            }
        }

        let device_memory_size = Self::SHARED_MEMORY_CHUNK_SIZE;
        let (device_memory, memory_type) =
            self.allocate_device_memory(device_memory_size, memory_property_flags)?;
        self.shared_memory.push(VulkanSharedMemory {
            memory: device_memory,
            memory_property_flags: memory_type.property_flags,
            allocator: AllocatorTree::new_root(device_memory_size),
            free_size_remaining: device_memory_size,
            active_allocations: 0,
        });

        Ok(self.shared_memory.len() as VulkanMemoryIndex - 1)
    }

    // Since this is a power of 2 allocator, alignment happens automatically.
    fn allocate_memory(
        &mut self,
        size: u64,
        mut allocation_type: VulkanAllocationType,
    ) -> anyhow::Result<VulkanMemoryAllocation> {
        // If size is greater than a certain amount, force the allocation to be dedicated.
        if size > Self::SHARED_MEMORY_CHUNK_SIZE {
            match allocation_type {
                VulkanAllocationType::GpuLocal => {
                    allocation_type = VulkanAllocationType::GpuLocalDedicated
                }
                VulkanAllocationType::CpuLocal => {
                    allocation_type = VulkanAllocationType::CpuLocalDedicated
                }
                _ => {}
            }
        }

        let allocation = match allocation_type {
            // Shared
            VulkanAllocationType::GpuLocal | VulkanAllocationType::CpuLocal => {
                let memory_property_flags = match allocation_type {
                    VulkanAllocationType::GpuLocal => ash::vk::MemoryPropertyFlags::DEVICE_LOCAL,
                    VulkanAllocationType::CpuLocal => {
                        ash::vk::MemoryPropertyFlags::HOST_VISIBLE
                            | ash::vk::MemoryPropertyFlags::HOST_CACHED
                    }
                    _ => unreachable!(),
                };
                let shared_memory_index =
                    self.find_or_create_shared_memory(size, memory_property_flags)?;
                let shared_memory = self
                    .shared_memory
                    .get_mut(shared_memory_index as usize)
                    .unwrap();
                let shared_memory_traversal = shared_memory
                    .allocator
                    .allocate(size.next_power_of_two())
                    .unwrap();
                shared_memory.active_allocations += 1;
                shared_memory.free_size_remaining -= shared_memory_traversal.length_bytes();

                VulkanMemoryAllocation {
                    memory_index: shared_memory_index,
                    traversal: Some(shared_memory_traversal),
                }
            }
            // Dedicated
            VulkanAllocationType::GpuLocalDedicated | VulkanAllocationType::CpuLocalDedicated => {
                let dedicated_memory_index = self.dedicated_memory.len() as VulkanMemoryIndex;
                let memory_property_flags = match allocation_type {
                    VulkanAllocationType::GpuLocalDedicated => {
                        ash::vk::MemoryPropertyFlags::DEVICE_LOCAL
                    }
                    VulkanAllocationType::CpuLocalDedicated => {
                        ash::vk::MemoryPropertyFlags::HOST_VISIBLE
                            | ash::vk::MemoryPropertyFlags::HOST_CACHED
                    }
                    _ => unreachable!(),
                };
                let (device_memory, _) =
                    self.allocate_device_memory(size, memory_property_flags)?;
                self.dedicated_memory.push(device_memory);

                VulkanMemoryAllocation {
                    memory_index: dedicated_memory_index,
                    traversal: None,
                }
            }
        };

        Ok(allocation)
    }

    fn get_allocation_info(&self, allocation: &VulkanMemoryAllocation) -> VulkanAllocationInfo {
        match &allocation.traversal {
            Some(traversal) => {
                let shared_memory = self
                    .shared_memory
                    .get(allocation.memory_index as usize)
                    .expect("Tried to get allocation info but allocation was freed/invalid.");

                VulkanAllocationInfo {
                    memory: shared_memory.memory.clone(),
                    offset: traversal.start_index_stride_bytes(),
                }
            }
            None => {
                let memory = self
                    .dedicated_memory
                    .get(allocation.memory_index as usize)
                    .expect("Tried to get allocation info but allocation was freed/invalid.")
                    .clone();

                VulkanAllocationInfo { memory, offset: 0 }
            }
        }
    }
}

impl Drop for VulkanAllocator {
    fn drop(&mut self) {
        unsafe { self.ctx.device.device_wait_idle() };

        for memory in self
            .shared_memory
            .iter()
            .map(|shared_memory| &shared_memory.memory)
            .chain(self.dedicated_memory.iter())
        {
            unsafe { self.ctx.device.free_memory(memory.device_memory, None) };
        }
    }
}

pub struct VulkanResourceManager {
    ctx: Arc<VulkanContextInner>,

    current_resource_id: AtomicU32,

    // Just the swapchain images.
    borrowed_images: parking_lot::RwLock<HashMap<ResourceId<Image>, VulkanBorrowedImage>>,
    owned_images: parking_lot::RwLock<HashMap<ResourceId<Image>, VulkanImage>>,
    owned_buffers: parking_lot::RwLock<HashMap<ResourceId<Buffer>, VulkanBuffer>>,
}

struct VulkanBuffer {
    buffer: ash::vk::Buffer,
    allocation: VulkanMemoryAllocation,
}

// TODO: Track lifetime based on RWLockReadGuard
pub struct VulkanImageRef {
    pub image: ash::vk::Image,
    pub view: Option<ash::vk::ImageView>,
    pub info: VulkanImageInfo,
}

impl VulkanImageRef {
    pub fn full_subresource_range(&self) -> ash::vk::ImageSubresourceRange {
        ash::vk::ImageSubresourceRange::default()
            .base_array_layer(0)
            .layer_count(1)
            .base_mip_level(0)
            .level_count(1)
            .aspect_mask(self.info.image_type.into())
    }

    pub fn full_subresource_layer(&self) -> ash::vk::ImageSubresourceLayers {
        ash::vk::ImageSubresourceLayers::default()
            .base_array_layer(0)
            .layer_count(1)
            .mip_level(0)
            .aspect_mask(self.info.image_type.into())
    }

    pub fn full_offset_3d(&self) -> ash::vk::Offset3D {
        ash::vk::Offset3D {
            x: self.info.extent.width as i32,
            y: self.info.extent.height as i32,
            z: 1,
        }
    }
}

struct VulkanImage {
    image: ash::vk::Image,
    allocation: VulkanMemoryAllocation,
    view: Option<ash::vk::ImageView>,
    info: VulkanImageInfo,
}

struct VulkanBorrowedImage {
    image: ash::vk::Image,
    view: Option<ash::vk::ImageView>,
    info: VulkanImageInfo,
}

#[derive(Clone)]
struct VulkanImageInfo {
    pub image_type: GfxImageType,
    pub format: ash::vk::Format,
    pub extent: ash::vk::Extent2D,
}

impl From<GfxFilterMode> for ash::vk::Filter {
    fn from(value: GfxFilterMode) -> Self {
        match value {
            GfxFilterMode::Nearest => ash::vk::Filter::NEAREST,
            GfxFilterMode::Linear => ash::vk::Filter::LINEAR,
        }
    }
}

impl From<GfxImageType> for ash::vk::ImageAspectFlags {
    fn from(value: GfxImageType) -> Self {
        match value {
            GfxImageType::D2 | GfxImageType::Cube => ash::vk::ImageAspectFlags::COLOR,
            GfxImageType::DepthD2 => ash::vk::ImageAspectFlags::DEPTH,
        }
    }
}

impl From<GfxImageType> for ash::vk::ImageType {
    fn from(value: GfxImageType) -> Self {
        match value {
            GfxImageType::D2 | GfxImageType::DepthD2 | GfxImageType::Cube => {
                ash::vk::ImageType::TYPE_2D
            }
        }
    }
}

impl From<GfxImageType> for ash::vk::ImageViewType {
    fn from(value: GfxImageType) -> Self {
        match value {
            GfxImageType::D2 | GfxImageType::DepthD2 => ash::vk::ImageViewType::TYPE_2D,
            GfxImageType::Cube => ash::vk::ImageViewType::CUBE,
        }
    }
}

impl From<ImageFormat> for ash::vk::Format {
    fn from(value: ImageFormat) -> Self {
        match value {
            ImageFormat::Rgba8Unorm => ash::vk::Format::R8G8B8A8_UNORM,
            ImageFormat::D16Unorm => ash::vk::Format::D16_UNORM,
            ImageFormat::Rgba32Float => ash::vk::Format::R32G32B32A32_SFLOAT,
        }
    }
}

struct VulkanBorrowedImageCreateInfo {
    image: ash::vk::Image,
    usage: ash::vk::ImageUsageFlags,
    info: VulkanImageInfo,
}

impl VulkanResourceManager {
    fn new(ctx: &Arc<VulkanContextInner>) -> Self {
        Self {
            ctx: ctx.clone(),

            current_resource_id: AtomicU32::new(0),

            borrowed_images: parking_lot::RwLock::new(HashMap::new()),
            owned_images: parking_lot::RwLock::new(HashMap::new()),
            owned_buffers: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    fn next_resource_id<T: 'static>(&self) -> ResourceId<T> {
        let resource_id = ResourceId::new(
            self.current_resource_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        resource_id
    }

    fn create_image_borrowed(
        &self,
        image_info: VulkanBorrowedImageCreateInfo,
    ) -> anyhow::Result<ResourceId<Image>> {
        let resource_id = self.next_resource_id();
        let view = if image_info.usage.intersects(
            ash::vk::ImageUsageFlags::SAMPLED
                | ash::vk::ImageUsageFlags::COLOR_ATTACHMENT
                | ash::vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
        ) {
            Some(self.create_image_view(image_info.image, &image_info.info)?)
        } else {
            None
        };

        self.borrowed_images.write().insert(
            resource_id,
            VulkanBorrowedImage {
                image: image_info.image,
                view,
                info: image_info.info,
            },
        );

        Ok(resource_id)
    }

    fn create_buffer(
        &self,
        allocator: &mut VulkanAllocator,
        create_info: GfxBufferCreateInfo,
    ) -> anyhow::Result<ResourceId<Buffer>> {
        anyhow::ensure!(create_info.size > 0);
        let create_info = ash::vk::BufferCreateInfo::default()
            .size(create_info.size)
            .usage(
                ash::vk::BufferUsageFlags::STORAGE_BUFFER
                    | ash::vk::BufferUsageFlags::UNIFORM_BUFFER
                    | ash::vk::BufferUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(ash::vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { self.ctx.device.create_buffer(&create_info, None) }?;

        let allocation =
            allocator.allocate_memory(create_info.size, VulkanAllocationType::GpuLocal)?;
        let allocation_info = allocator.get_allocation_info(&allocation);
        unsafe {
            self.ctx.device.bind_buffer_memory(
                buffer,
                allocation_info.memory.device_memory,
                allocation_info.offset,
            )
        }?;

        let resource_id = self.next_resource_id();
        self.owned_buffers
            .write()
            .insert(resource_id, VulkanBuffer { buffer, allocation });

        Ok(resource_id)
    }

    /// Images have dedicated memory allocations by default.
    fn create_image(
        &self,
        allocator: &mut VulkanAllocator,
        create_info: GfxImageCreateInfo,
    ) -> anyhow::Result<ResourceId<Image>> {
        anyhow::ensure!(create_info.extent.x > 0 && create_info.extent.y > 0);
        let image_info = VulkanImageInfo {
            image_type: create_info.image_type,
            format: create_info.format.into(),
            extent: ash::vk::Extent2D {
                width: create_info.extent.x,
                height: create_info.extent.y,
            },
        };

        let type_specific_usages = match image_info.image_type {
            GfxImageType::DepthD2 => ash::vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            _ => ash::vk::ImageUsageFlags::COLOR_ATTACHMENT,
        };

        let create_info = ash::vk::ImageCreateInfo::default()
            .image_type(image_info.image_type.into())
            .format(image_info.format.into())
            .usage(
                ash::vk::ImageUsageFlags::STORAGE
                    | ash::vk::ImageUsageFlags::TRANSFER_SRC
                    | ash::vk::ImageUsageFlags::TRANSFER_DST
                    | type_specific_usages,
            )
            .extent(
                ash::vk::Extent3D::default()
                    .width(image_info.extent.width)
                    .height(image_info.extent.height)
                    .depth(1),
            )
            .samples(ash::vk::SampleCountFlags::TYPE_1)
            .array_layers(1)
            .mip_levels(1);
        let image = unsafe { self.ctx.device.create_image(&create_info, None) }?;

        let image_memory_requirements =
            unsafe { self.ctx.device.get_image_memory_requirements(image) };
        let allocation = allocator.allocate_memory(
            image_memory_requirements.size,
            VulkanAllocationType::GpuLocalDedicated,
        )?;
        let allocation_info = allocator.get_allocation_info(&allocation);
        unsafe {
            self.ctx.device.bind_image_memory(
                image,
                allocation_info.memory.device_memory,
                allocation_info.offset,
            )
        }?;

        let image_view = self.create_image_view(image, &image_info)?;

        let resource_id = self.next_resource_id();
        self.owned_images.write().insert(
            resource_id,
            VulkanImage {
                image,
                allocation,
                view: Some(image_view),
                info: image_info,
            },
        );

        Ok(resource_id)
    }

    fn create_image_view(
        &self,
        image: ash::vk::Image,
        image_info: &VulkanImageInfo,
    ) -> anyhow::Result<ash::vk::ImageView> {
        let create_info = ash::vk::ImageViewCreateInfo::default()
            .image(image)
            .format(image_info.format)
            .components(ash::vk::ComponentMapping::default())
            .view_type(image_info.image_type.into())
            .subresource_range(
                ash::vk::ImageSubresourceRange::default()
                    .aspect_mask(image_info.image_type.into())
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1),
            );
        let image_view = unsafe { self.ctx.device.create_image_view(&create_info, None) }?;
        Ok(image_view)
    }

    pub fn get_image<'a>(&'a self, image_id: ResourceId<Image>) -> VulkanImageRef {
        let borrowed_image_ref = self.borrowed_images.read();
        if borrowed_image_ref.contains_key(&image_id) {
            let img = borrowed_image_ref.get(&image_id).unwrap();

            return VulkanImageRef {
                image: img.image.clone(),
                view: img.view,
                info: img.info.clone(),
            };
        }
        drop(borrowed_image_ref);

        let owned_image_ref = self.owned_images.read();
        let Some(img) = owned_image_ref.get(&image_id) else {
            panic!(
                "Image id of {} doesn't exist in the resource manager.",
                image_id.id()
            );
        };
        let image_ref = VulkanImageRef {
            image: img.image.clone(),
            view: img.view,
            info: img.info.clone(),
        };
        drop(owned_image_ref);

        return image_ref;
    }
}

impl Drop for VulkanResourceManager {
    // Don't worry about freeing memory allocations since the `VulkanAllocator` will be dropped
    // as well, destroying any gpu memory allocation.
    fn drop(&mut self) {
        unsafe { self.ctx.device.device_wait_idle() };

        for (_, borrowed_image) in self.borrowed_images.write().iter() {
            if let Some(image_view) = borrowed_image.view {
                unsafe { self.ctx.device.destroy_image_view(image_view, None) };
            }
        }

        for (_, owned_images) in self.owned_images.write().iter() {
            if let Some(image_view) = owned_images.view {
                unsafe { self.ctx.device.destroy_image_view(image_view, None) };
            }

            unsafe { self.ctx.device.destroy_image(owned_images.image, None) };
        }
        for (_, owned_buffers) in self.owned_buffers.write().iter() {
            unsafe { self.ctx.device.destroy_buffer(owned_buffers.buffer, None) };
        }
    }
}
