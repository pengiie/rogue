use core::panic;
use std::{
    any::Any,
    borrow::BorrowMut,
    collections::{HashMap, HashSet},
    ffi::CString,
    num::NonZeroU32,
    sync::{
        atomic::{AtomicU32, AtomicU64},
        Arc,
    },
    u32, u64,
};

use anyhow::{anyhow, Context};
use ash::{
    khr::swapchain,
    vk::{self, QueueFlags, SemaphoreType},
};
use log::{debug, warn};
use nalgebra::{Vector2, Vector3};
use parking_lot::lock_api::RwLock;
use raw_window_handle::{HasDisplayHandle, HasRawWindowHandle, HasWindowHandle};

use crate::{
    common::freelist::{FreeList, FreeListHandle},
    engine::{
        event::Events,
        graphics::{
            backend::{
                BindGroup, Binding, Buffer, ComputePipeline, GfxAddressMode, GfxBlendFactor,
                GfxBlendOp, GfxBufferCreateInfo, GfxBufferInfo, GfxComputePipelineCreateInfo,
                GfxComputePipelineInfo, GfxCullMode, GfxFilterMode, GfxFrontFace,
                GfxImageCreateInfo, GfxImageFormat, GfxImageInfo, GfxImageType, GfxImageWrite,
                GfxLoadOp, GfxPresentMode, GfxRasterPipelineBlendStateAttachmentInfo,
                GfxRasterPipelineBlendStateCreateInfo, GfxRasterPipelineCreateInfo,
                GfxSamplerCreateInfo, GfxSwapchainInfo, GfxVertexAttribute,
                GfxVertexAttributeFormat, GfxVertexFormat, GraphicsBackendDevice,
                GraphicsBackendEvent, GraphicsBackendFrameGraphExecutor, Image, Memory,
                RasterPipeline, ResourceId, Sampler, ShaderSetData, ShaderWriter, UniformSetData,
                Untyped,
            },
            gpu_allocator::{Allocation, AllocatorTree},
            shader::{
                Shader, ShaderBindingType, ShaderCompilationOptions, ShaderCompilationTarget,
                ShaderCompiler, ShaderSetBinding, ShaderStage,
            },
        },
        window::{
            time::Instant,
            window::{Window, WindowHandle},
        },
    },
};

use super::{executor::VulkanFrameGraphExecutor, recorder::VulkanRecorder};

pub const VK_STAGING_BUFFER_MIN_SIZE: u64 = 1 << 23; // 8 MiB

pub struct VulkanContextInner {
    entry: ash::Entry,
    instance: ash::Instance,
    debug_messenger: Option<ash::vk::DebugUtilsMessengerEXT>,
    surface: ash::vk::SurfaceKHR,
    physical_device: VulkanPhysicalDevice,
    device: ash::Device,

    main_queue: ash::vk::Queue,
    main_queue_family_index: u32,
    transfer_queue: Option<ash::vk::Queue>,
    transfer_queue_family_index: Option<u32>,

    // The number of frames the cpu and gpu can process before waiting.
    frames_in_flight: u32,

    /// The current frame the cpu is working on.
    current_cpu_frame: AtomicU64,
    /// The current frame the gpu is working on.
    gpu_timeline_semaphore: ash::vk::Semaphore,
}

impl VulkanContextInner {
    pub fn curr_cpu_frame(&self) -> u64 {
        self.current_cpu_frame
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn curr_cpu_frame_index(&self) -> u32 {
        (self.curr_cpu_frame() % self.frames_in_flight as u64) as u32
    }

    pub fn curr_gpu_frame(&self) -> u64 {
        unsafe {
            self.device
                .get_semaphore_counter_value(self.gpu_timeline_semaphore)
        }
        .expect("Failed to get gpu timeline semaphore.")
    }
}

impl Drop for VulkanContextInner {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().unwrap();
            println!("Dropping the inner device context");

            self.device
                .destroy_semaphore(self.gpu_timeline_semaphore, None);
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

// Order of fields is important due to struct drop order.
pub struct VulkanContext {
    memory_allocator: parking_lot::RwLock<VulkanAllocator>,
    resource_manager: VulkanResourceManager,

    swapchain: parking_lot::RwLock<Arc<VulkanSwapchain>>,
    inner: Arc<VulkanContextInner>,

    // Semaphore when the swapchain image is acquired.
    image_acquire_semaphores: Vec<ash::vk::Semaphore>,
    // Semaphore when the swapchain image is finished being rendered to.
    image_ready_semaphores: Vec<ash::vk::Semaphore>,

    // The current swapchain image index of the most recently acquired image.
    swapchain_image_index: AtomicU32,
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
        self.inner.frames_in_flight
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
        self.inner.curr_cpu_frame()
    }

    pub fn curr_gpu_frame(&self) -> u64 {
        self.inner.curr_gpu_frame()
    }

    pub fn curr_cpu_frame_index(&self) -> u32 {
        self.inner.curr_cpu_frame_index()
    }

    pub fn curr_image_acquire_semaphore(&self) -> ash::vk::Semaphore {
        self.image_acquire_semaphores[self.curr_cpu_frame_index() as usize]
    }

    pub fn curr_image_ready_semaphore(&self) -> ash::vk::Semaphore {
        self.image_ready_semaphores[self.curr_cpu_frame_index() as usize]
    }

    pub fn gpu_timeline_semaphore(&self) -> ash::vk::Semaphore {
        self.inner.gpu_timeline_semaphore
    }

    pub fn main_queue(&self) -> ash::vk::Queue {
        self.inner.main_queue
    }

    pub fn transfer_queue(&self) -> Option<ash::vk::Queue> {
        self.inner.transfer_queue
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

    pub fn get_image_info(&self, image: &ResourceId<Image>) -> GfxImageInfo {
        let image = self.resource_manager.get_image(*image);
        let info = &image.info;

        GfxImageInfo {
            resolution: Vector3::new(info.extent.width, info.extent.height, 1),
            format: info.format.into(),
        }
    }

    pub fn get_image(&self, image: &ResourceId<Image>) -> VulkanImageRef {
        self.resource_manager.get_image(*image)
    }

    pub fn create_frame_event(&self) -> ash::vk::Event {
        self.resource_manager.get_frame_vk_event()
    }

    pub fn create_buffer(
        &self,
        create_info: GfxBufferCreateInfo,
    ) -> anyhow::Result<ResourceId<Buffer>> {
        let mut memory_allocator = self.memory_allocator.write();
        self.resource_manager.create_buffer(
            &mut memory_allocator,
            create_info,
            VulkanAllocationType::GpuLocal,
            false,
        )
    }

    pub fn get_buffer(&self, buffer: ResourceId<Buffer>) -> VulkanBuffer {
        self.resource_manager.get_buffer_info(&buffer)
    }

    pub fn get_pipeline_layout(&self, pipeline: ResourceId<Untyped>) -> VulkanPipelineLayout {
        self.resource_manager.get_pipeline_layout(pipeline)
    }

    pub fn record_buffer_writes(&self, recorder: &mut VulkanRecorder) {
        self.resource_manager.record_buffer_writes(recorder);
    }

    /// Guarantees that the buffer write will be available when recording the current cpu/gpu frame.
    pub fn write_buffer(
        &self,
        buffer: &ResourceId<Buffer>,
        offset: u64,
        write_len: u64,
    ) -> *mut u8 {
        let mut memory_allocator = self.memory_allocator.write();
        self.resource_manager
            .write_buffer(&mut memory_allocator, buffer, offset, write_len)
    }

    /// Guarantees that the image write will be available when recording the current cpu/gpu frame.
    pub fn write_image(&self, info: GfxImageWrite) {
        let mut memory_allocator = self.memory_allocator.write();
        self.resource_manager
            .write_image(&mut memory_allocator, info)
    }

    pub fn create_compute_pipeline(
        &self,
        create_info: GfxComputePipelineCreateInfo,
    ) -> anyhow::Result<ResourceId<ComputePipeline>> {
        self.resource_manager.create_compute_pipeline(create_info)
    }

    pub fn destroy_compute_pipeline(&self, id: ResourceId<ComputePipeline>) {
        warn!("TODO: Work on destroying pipelines.");
    }

    pub fn create_raster_pipeline(
        &self,
        create_info: GfxRasterPipelineCreateInfo,
    ) -> anyhow::Result<ResourceId<RasterPipeline>> {
        self.resource_manager.create_raster_pipeline(create_info)
    }

    pub fn get_compute_pipeline(
        &self,
        compute_pipeline: ResourceId<ComputePipeline>,
    ) -> VulkanComputePipeline {
        self.resource_manager.get_compute_pipeline(compute_pipeline)
    }

    pub fn get_raster_pipeline(
        &self,
        raster_pipeline: ResourceId<RasterPipeline>,
    ) -> VulkanRasterPipeline {
        self.resource_manager.get_raster_pipeline(raster_pipeline)
    }

    pub fn create_sampler(&self, create_info: GfxSamplerCreateInfo) -> ResourceId<Sampler> {
        self.resource_manager.create_sampler(create_info)
    }

    /// Writes the descriptor sets.
    pub fn write_shader_sets(&self, binding_data: HashMap<&ShaderSetBinding, ShaderSetData>) {
        let mut memory_allocator = self.memory_allocator.write();
        self.resource_manager
            .write_shader_sets(&mut memory_allocator, binding_data);
    }

    pub fn bind_uniforms(
        &self,
        pipeline: ResourceId<Untyped>,
        binding_data: HashMap<u32, ShaderSetData>,
    ) -> VulkanUniformBinding {
        let mut memory_allocator = self.memory_allocator.write();
        self.resource_manager
            .bind_uniforms(&mut memory_allocator, pipeline, binding_data)
    }
}

pub struct VulkanUniformBinding {
    pub pipeline_layout: ash::vk::PipelineLayout,
    pub first_set: u32,
    pub descriptor_sets: Vec<ash::vk::DescriptorSet>,
    pub expected_image_layouts:
        HashMap<ResourceId<Image>, (ash::vk::ImageLayout, ash::vk::AccessFlags)>,
}

pub type VulkanContextHandle = Arc<VulkanContext>;
pub struct VulkanDevice {
    context: VulkanContextHandle,

    // Size is # of frames in flight (swapchain images).
    destroy_frame_queue: Vec<Vec<ResourceId<Untyped>>>,
    skipped_gpu_frames: HashSet<u64>,
}

pub struct VulkanSwapchain {
    ctx_ref: Arc<VulkanContextInner>,
    pub swapchain: ash::vk::SwapchainKHR,
    pub create_info: ash::vk::SwapchainCreateInfoKHR<'static>,
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

            let available_layers = unsafe {
                entry
                    .enumerate_instance_layer_properties()
                    .expect("Failed to get vk layers")
            };
            let available_layer_names = available_layers
                .iter()
                .map(|name| name.layer_name_as_c_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            for layer in available_layers {
                debug!(
                    "Avilable vulkan layer {:?}",
                    layer.layer_name_as_c_str().unwrap()
                );
            }

            let enabled_layers = Self::get_required_layer_names()
                .into_iter()
                .filter(|str| available_layer_names.contains(&str))
                .collect::<Vec<_>>();
            debug!("enabled layers {:?}", &enabled_layers);
            let enabled_layers_ptrs = enabled_layers
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
        let transfer_queue_family_index = physical_device
            .queue_family_properties
            .iter()
            .enumerate()
            .find_map(|(i, properties)| {
                if i as u32 != main_queue_family_index
                    && properties
                        .queue_flags
                        .contains(ash::vk::QueueFlags::TRANSFER)
                {
                    return Some(i as u32);
                }

                None
            });

        let device = {
            let enabled_extensions_ptrs = vec![
                ash::khr::swapchain::NAME.as_ptr(),
                ash::khr::dynamic_rendering_local_read::NAME.as_ptr(),
            ];

            let mut feature_dynamic_rendering_local_read =
                ash::vk::PhysicalDeviceDynamicRenderingLocalReadFeaturesKHR::default()
                    .dynamic_rendering_local_read(true);
            let mut feature_dynamic_rendering =
                ash::vk::PhysicalDeviceDynamicRenderingFeatures::default().dynamic_rendering(true);
            feature_dynamic_rendering.p_next =
                std::ptr::from_mut(&mut feature_dynamic_rendering_local_read)
                    as *mut std::ffi::c_void;
            let mut feature_timeline_semaphore =
                ash::vk::PhysicalDeviceTimelineSemaphoreFeatures::default()
                    .timeline_semaphore(true);
            feature_timeline_semaphore.p_next =
                std::ptr::from_mut(&mut feature_dynamic_rendering) as *mut std::ffi::c_void;
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

            let queue_priorities = [1.0];
            let mut queue_create_infos = vec![ash::vk::DeviceQueueCreateInfo::default()
                .queue_priorities(&queue_priorities)
                .queue_family_index(main_queue_family_index)];
            if let Some(transfer_queue_family_index) = transfer_queue_family_index {
                queue_create_infos.push(
                    ash::vk::DeviceQueueCreateInfo::default()
                        .queue_priorities(&queue_priorities)
                        .queue_family_index(transfer_queue_family_index),
                );
            }

            let device_create_info = ash::vk::DeviceCreateInfo::default()
                .push_next(&mut features2)
                .enabled_extension_names(&enabled_extensions_ptrs)
                .queue_create_infos(&queue_create_infos);

            unsafe {
                instance.create_device(physical_device.physical_device, &device_create_info, None)
            }?
        };

        let main_queue = unsafe { device.get_device_queue(main_queue_family_index, 0) };
        let transfer_queue =
            transfer_queue_family_index.map(|transfer_queue_family_index| unsafe {
                device.get_device_queue(transfer_queue_family_index, 0)
            });

        let (
            swapchain,
            swapchain_create_info,
            swapchain_images,
            swapchain_format,
            swapchain_extent,
            swapchain_image_usage,
        ) = {
            let mut surface_capabilities = unsafe {
                surface_loader.get_physical_device_surface_capabilities(
                    physical_device.physical_device,
                    surface,
                )
            }?;
            debug!("Surface capabilities: {:?}", surface_capabilities);
            let present_mode = Self::get_optimal_present_mode(
                &surface_loader,
                &surface,
                &physical_device,
                &swapchain_info.present_mode,
            )?;
            let swapchain_format =
                Self::get_optimal_swapchain_format(&surface_loader, &surface, &physical_device)?;

            // A max image count of 0 means there is no limit, so choose some arbitrary upper limit.
            if surface_capabilities.max_image_count == 0 {
                surface_capabilities.max_image_count = surface_capabilities.min_image_count + 1;
            }

            let min_image_count = swapchain_info
                .triple_buffering
                .then_some(3)
                .unwrap_or(2)
                .clamp(
                    surface_capabilities.min_image_count,
                    surface_capabilities.max_image_count,
                );
            let swapchain_extent = ash::vk::Extent2D {
                width: window.width().clamp(
                    surface_capabilities.min_image_extent.width,
                    surface_capabilities.max_image_extent.width,
                ),
                height: window.height().clamp(
                    surface_capabilities.min_image_extent.height,
                    surface_capabilities.max_image_extent.height,
                ),
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
                swapchain_create_info,
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

            main_queue_family_index,
            main_queue,
            transfer_queue_family_index,
            transfer_queue,

            frames_in_flight,
            // One since `gpu_timeline_semaphore` starts signalling 0.
            current_cpu_frame: AtomicU64::new(1),
            gpu_timeline_semaphore: timeline_semaphore,
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
            create_info: swapchain_create_info,
            swapchain,
            swapchain_images,
        });

        let context = Arc::new(VulkanContext {
            inner: context_inner.clone(),
            swapchain: parking_lot::RwLock::new(swapchain),

            swapchain_image_index: AtomicU32::new(0),

            image_acquire_semaphores,
            image_ready_semaphores,

            memory_allocator: parking_lot::RwLock::new(VulkanAllocator::new(&context_inner)),
            resource_manager,
        });

        Ok(VulkanDevice {
            context,

            destroy_frame_queue: (0..frames_in_flight).map(|_| Vec::new()).collect(),
            skipped_gpu_frames: HashSet::new(),
        })
    }

    fn get_required_layer_names() -> Vec<CString> {
        vec![
            #[cfg(debug_assertions)]
            std::ffi::CString::new("VK_LAYER_KHRONOS_validation").unwrap(),
            //std::ffi::CString::new("VK_LAYER_LUNARG_api_dump").unwrap(),
        ]
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
        let message = std::ffi::CStr::from_ptr((*p_callback_data).p_message)
            .to_str()
            .unwrap();
        if message.contains("VVL-DEBUG-PRINTF") {
            let first_bar = message.find("|").unwrap();
            let message = &message[(first_bar + 1)..];
            let second_bar = message.find("|").unwrap();
            log::debug!("Shader printf: {}", message[(second_bar + 1)..].trim());
            return ash::vk::FALSE;
        }

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
            println!("Dropping vulkan context");

            for semaphore in self
                .image_ready_semaphores
                .iter()
                .chain(&self.image_acquire_semaphores)
            {
                self.device().destroy_semaphore(*semaphore, None);
            }
        }
    }
}

impl Drop for VulkanSwapchain {
    fn drop(&mut self) {
        unsafe { self.ctx_ref.device.device_wait_idle() };
        println!("Dropping the swapchain with handle {:?}", self.swapchain);

        let swapchain_loader =
            ash::khr::swapchain::Device::new(&self.ctx_ref.instance, &self.ctx_ref.device);
        unsafe { swapchain_loader.destroy_swapchain(self.swapchain, None) };
    }
}

impl GraphicsBackendDevice for VulkanDevice {
    fn pre_init_update(&mut self, events: &mut Events) {
        // We initialize synchronously.
        events.push(GraphicsBackendEvent::Initialized);
    }

    fn begin_frame(&mut self, events: &mut Events) {
        // Equals n - 1, where n is the current cpu frame.
        let prev_cpu_frame = self
            .context
            .inner
            .current_cpu_frame
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Wait for gpu timeline semaphore for n - (f - 1), where f is the # of frames in flight.
        let fif_minus_one = self.context.inner.frames_in_flight as u64 - 1;
        if prev_cpu_frame >= fif_minus_one {
            let wait_gpu_frame = prev_cpu_frame - fif_minus_one;

            // Don't wait on the gpu frame if it was skipped due to a bad swapchain image.
            if !self.skipped_gpu_frames.remove(&wait_gpu_frame) {
                let wait_info = ash::vk::SemaphoreWaitInfo::default()
                    .semaphores(std::slice::from_ref(
                        &self.context.inner.gpu_timeline_semaphore,
                    ))
                    .values(std::slice::from_ref(&wait_gpu_frame));

                // TODO: Worry about timeout.
                unsafe { self.context.device().wait_semaphores(&wait_info, u64::MAX) };
            } else {
                let signal_semaphore_info = ash::vk::SemaphoreSignalInfo::default()
                    .semaphore(self.context.inner.gpu_timeline_semaphore)
                    .value(wait_gpu_frame);
                unsafe {
                    self.context
                        .device()
                        .signal_semaphore(&signal_semaphore_info)
                };
            }
        }

        // Free previously used events, cache slots, and descriptor owned uniform buffers..
        self.context.resource_manager.retire_resources();
    }

    fn swapchain_size(&self) -> Vector2<u32> {
        let extent = self.context.swapchain.read().create_info.image_extent;
        Vector2::new(extent.width, extent.height)
    }

    fn create_frame_graph_executor(&mut self) -> Box<dyn GraphicsBackendFrameGraphExecutor> {
        Box::new(VulkanFrameGraphExecutor::new(&self.context))
    }

    fn register_compute_pipeline(
        &mut self,
        create_info: GfxComputePipelineCreateInfo,
    ) -> ResourceId<ComputePipeline> {
        todo!()
    }

    fn register_raster_pipeline(
        &mut self,
        create_info: GfxRasterPipelineCreateInfo,
    ) -> ResourceId<RasterPipeline> {
        todo!()
    }

    fn create_image(&mut self, create_info: GfxImageCreateInfo) -> ResourceId<Image> {
        self.context.create_image(create_info).unwrap()
    }

    fn get_image_info(&self, image: &ResourceId<Image>) -> GfxImageInfo {
        self.context.get_image_info(image)
    }

    fn write_image(&mut self, info: GfxImageWrite) {
        self.context.write_image(info)
    }

    fn create_buffer(&mut self, create_info: GfxBufferCreateInfo) -> ResourceId<Buffer> {
        self.context.create_buffer(create_info).unwrap()
    }

    fn write_buffer(
        &mut self,
        buffer: &ResourceId<Buffer>,
        offset: u64,
        write_len: u64,
    ) -> &mut [u8] {
        let ptr = self.context.write_buffer(buffer, offset, write_len);
        unsafe { std::slice::from_raw_parts_mut(ptr, write_len as usize) }
    }

    fn create_sampler(&mut self, create_info: GfxSamplerCreateInfo) -> ResourceId<Sampler> {
        self.context.create_sampler(create_info)
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
        let acquire_timer = Instant::now();
        let (image_index, out_of_date) = match unsafe {
            swapchain_loader.acquire_next_image(
                self.context.swapchain().swapchain,
                20 * 1_000_000_000, // 20 second timeout
                self.context.curr_image_acquire_semaphore(),
                ash::vk::Fence::null(),
            )
        } {
            Ok((image_index, out_of_date)) => (image_index, out_of_date),
            Err(err) => anyhow::bail!("Got error {}", err),
        };
        if out_of_date {
            debug!("Swapchain is out of date.");
        }
        // debug!(
        //     "Took {}ms to acquire vk swapchain image.",
        //     acquire_timer.elapsed().as_micros() as f32 / 1000.0
        // );
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

    fn resize_swapchain(
        &mut self,
        new_size: winit::dpi::PhysicalSize<NonZeroU32>,
        skip_frame: bool,
    ) {
        let mut swapchain = self.context.swapchain.write();

        let surface_loader = self.context.surface_loader();
        let surface_capabilities = unsafe {
            surface_loader.get_physical_device_surface_capabilities(
                self.context.inner.physical_device.physical_device,
                self.context.surface(),
            )
        }
        .expect("Failed to get vulkan surface capabilities.");

        let new_extent = ash::vk::Extent2D::default()
            .width(Into::<u32>::into(new_size.width).clamp(
                surface_capabilities.min_image_extent.width,
                surface_capabilities.max_image_extent.width,
            ))
            .height(Into::<u32>::into(new_size.height).clamp(
                surface_capabilities.min_image_extent.height,
                surface_capabilities.max_image_extent.height,
            ));

        let new_swapchain_create_info = swapchain
            .create_info
            .image_extent(new_extent)
            .old_swapchain(swapchain.swapchain);

        let swapchain_loader = self.context.swapchain_loader();
        let new_swapchain =
            unsafe { swapchain_loader.create_swapchain(&new_swapchain_create_info, None) }
                .expect("Failed to recreate swapchain");
        debug!("Created new swapchain with handle {:?}", new_swapchain);

        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(new_swapchain) }
            .expect("Failed to get swapchain images");
        let swapchain_images = swapchain_images
            .into_iter()
            .map(|image| {
                self.context
                    .resource_manager
                    .create_image_borrowed(VulkanBorrowedImageCreateInfo {
                        image,
                        usage: new_swapchain_create_info.image_usage,
                        info: VulkanImageInfo {
                            image_type: GfxImageType::D2,
                            format: new_swapchain_create_info.image_format,
                            extent: new_extent,
                        },
                    })
                    .expect("Failed to create swapchain image")
            })
            .collect::<Vec<_>>();

        let mut new_swapchain = Arc::new(VulkanSwapchain {
            ctx_ref: self.context.inner.clone(),
            swapchain: new_swapchain,
            create_info: new_swapchain_create_info,
            swapchain_images,
        });

        std::mem::swap(&mut new_swapchain, &mut swapchain);
        for image_id in &new_swapchain.swapchain_images {
            self.context
                .resource_manager
                .destroy_image_borrowed(image_id);
        }
        drop(new_swapchain);

        if skip_frame {
            self.skipped_gpu_frames
                .insert(self.context.curr_cpu_frame());
        }
    }

    fn get_buffer_info(&self, buffer: &ResourceId<Buffer>) -> GfxBufferInfo {
        let buf = self.context.get_buffer(*buffer);
        GfxBufferInfo { size: buf.size }
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
    mapped_ptr: Option<*mut u8>,
    device_memory: ash::vk::DeviceMemory,
    /// Size of the entire `ash::vk::DeviceMemory` allocated.
    size: u64,
}

type VulkanMemoryIndex = u64;

#[derive(Clone)]
struct VulkanMemoryAllocation {
    memory_index: VulkanMemoryIndex,
    // If this is `Some`, then this points to a shared allocation; else, this points to a dedicated
    // allocation.
    traversal: Option<Allocation>,
}

struct VulkanAllocationInfo {
    /// Mapped pointer relative to the start of this allocation and is only valid for the
    /// allocation size.
    mapped_ptr: Option<*mut u8>,
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
    const SHARED_MEMORY_CHUNK_SIZE: u64 = 1 << 25; // 32 Mib

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
            .find(|(i, memory_type)| memory_type.property_flags.contains(memory_property_flags))
            .context("Failed to find a suitable memory type")?;

        let allocation_info = ash::vk::MemoryAllocateInfo::default()
            .allocation_size(size)
            .memory_type_index(memory_type_index as u32);
        let device_memory = unsafe { self.ctx.device.allocate_memory(&allocation_info, None) }?;
        debug!(
                    "Allocated {} bytes of with memory type index {} with properties {:?} for memory properties {:?}",
                    size, memory_type_index as u32, memory_type.property_flags, memory_property_flags
                );

        Ok((
            VulkanMemory {
                device_memory,
                size,
                mapped_ptr: None,
            },
            *memory_type,
        ))
    }

    fn find_or_create_shared_memory(
        &mut self,
        allocation_size: u64,
        allocation_alignment: u64,
        memory_property_flags: ash::vk::MemoryPropertyFlags,
    ) -> anyhow::Result<VulkanMemoryIndex> {
        // Check for available shared memory with the sample memory properties.
        for (memory_index, shared_memory) in self.shared_memory.iter().enumerate() {
            if shared_memory
                .memory_property_flags
                .contains(memory_property_flags)
                && shared_memory.free_size_remaining >= allocation_size
            {
                debug!(
                    "Using shared memory index {} with {}/{} bytes remaining for memory properties {:?}",
                    memory_index, shared_memory.free_size_remaining, shared_memory.memory.size, memory_property_flags
                );
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
        alignment: u32,
        mut allocation_type: VulkanAllocationType,
        mapped: bool,
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
                            | ash::vk::MemoryPropertyFlags::HOST_COHERENT
                    }
                    _ => unreachable!(),
                };
                let shared_memory_index = self.find_or_create_shared_memory(
                    size,
                    alignment as u64,
                    memory_property_flags,
                )?;
                let shared_memory = self
                    .shared_memory
                    .get_mut(shared_memory_index as usize)
                    .unwrap();
                let shared_memory_traversal = shared_memory
                    .allocator
                    .allocate(size.next_power_of_two(), alignment)
                    .unwrap();
                shared_memory.active_allocations += 1;
                shared_memory.free_size_remaining -= shared_memory_traversal.length_bytes();

                if mapped && shared_memory.memory.mapped_ptr.is_none() {
                    shared_memory.memory.mapped_ptr = Some(unsafe {
                        self.ctx
                            .device
                            .map_memory(
                                shared_memory.memory.device_memory,
                                0,
                                shared_memory.memory.size,
                                ash::vk::MemoryMapFlags::empty(),
                            )
                            .expect("Failed to map shared gpu memory")
                            as *mut u8
                    });
                }

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
                            | ash::vk::MemoryPropertyFlags::HOST_COHERENT
                    }
                    _ => unreachable!(),
                };
                let (mut device_memory, _) =
                    self.allocate_device_memory(size, memory_property_flags)?;
                log::debug!(
                    "Allocation dedicated memory of {} bytes",
                    device_memory.size
                );

                if mapped && device_memory.mapped_ptr.is_none() {
                    device_memory.mapped_ptr = Some(unsafe {
                        self.ctx
                            .device
                            .map_memory(
                                device_memory.device_memory,
                                0,
                                device_memory.size,
                                ash::vk::MemoryMapFlags::empty(),
                            )
                            .expect("Failed to map dedicated gpu memory")
                            as *mut u8
                    });
                }

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
                    mapped_ptr: shared_memory.memory.mapped_ptr.map(|ptr| unsafe {
                        ptr.byte_add(traversal.start_index_stride_bytes() as usize)
                    }),
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

                VulkanAllocationInfo {
                    mapped_ptr: memory.mapped_ptr,
                    memory,
                    offset: 0,
                }
            }
        }
    }
}

impl Drop for VulkanAllocator {
    fn drop(&mut self) {
        unsafe { self.ctx.device.device_wait_idle() };
        println!("Dropping vulkan allocator");

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

// TODO: Use freelist allocators instead. Especially important for descriptor set layouts since the
// vec will only grow at the moment.
pub struct VulkanResourceManager {
    ctx: Arc<VulkanContextInner>,

    current_resource_id: AtomicU32,

    // Just the swapchain images.
    borrowed_images: parking_lot::RwLock<HashMap<ResourceId<Image>, VulkanBorrowedImage>>,
    owned_images: parking_lot::RwLock<HashMap<ResourceId<Image>, VulkanImage>>,
    owned_buffers: parking_lot::RwLock<HashMap<ResourceId<Buffer>, VulkanBuffer>>,

    samplers: parking_lot::RwLock<HashMap<ResourceId<Sampler>, ash::vk::Sampler>>,

    pipeline_layouts: parking_lot::RwLock<Vec<VulkanPipelineLayout>>,
    shader_pipeline_layout_map:
        parking_lot::RwLock<HashMap<Vec<ShaderSetBinding>, /*pipeline_layout_index=*/ u64>>,

    compute_pipelines:
        parking_lot::RwLock<HashMap<ResourceId<ComputePipeline>, VulkanComputePipeline>>,
    raster_pipelines:
        parking_lot::RwLock<HashMap<ResourceId<RasterPipeline>, VulkanRasterPipeline>>,

    descriptor_pool: ash::vk::DescriptorPool,
    descriptor_set_groups: parking_lot::RwLock<HashMap<ShaderSetBinding, VulkanDescriptorSetGroup>>,
    descriptor_sets: parking_lot::RwLock<FreeList<VulkanDescriptorSet>>,
    cache_slots: parking_lot::RwLock<
        HashMap<u32, (VulkanShaderSetData, FreeListHandle<VulkanDescriptorSet>)>,
    >,

    // Events are only valid when using them, for one gpu frame. After that gpu frame the event is
    // available to use again.
    event_pools: Vec<parking_lot::RwLock<VulkanEventPool>>,

    staging_buffers: parking_lot::RwLock<FreeList<VulkanStagingBuffer>>,
    /// Staging buffers that have ownership transferred to the gpu.
    in_use_staging_buffers: parking_lot::RwLock<HashSet<FreeListHandle<VulkanStagingBuffer>>>,
    /// Staging buffers associated with the frame index they are used on the gpu, so when we get
    /// the timeline semaphore for that frame index, we can free time.
    staging_buffer_gpu_timeline: Vec<parking_lot::RwLock<Vec<FreeListHandle<VulkanStagingBuffer>>>>,
    copy_tasks: parking_lot::RwLock<
        HashMap<FreeListHandle<VulkanStagingBuffer>, Vec<VulkanStagingCopyTask>>,
    >,
}

struct VulkanStagingBuffer {
    buffer: ResourceId<Buffer>,
    /// The mapped coherent pointer to the start of the gpu buffer.
    mapped_pointer: *mut u8,
    /// The current index into the staging buffer that we can start writing.
    curr_write_pointer: u64,
    size: u64,
}

struct VulkanEventPool {
    free_events: Vec<u32>,
    event_pool: Vec<ash::vk::Event>,
}

// The lower the score indicates more usage, this score is used as a type of garbage collector for
// our cache when we see we no longer need a descriptor set of a certain type.
struct VulkanUsageScore(u32);

impl VulkanUsageScore {
    const FRAME_INCREMENT: u32 = 100;
    // If a descriptor set hasn't been used in 600 frames, its unlikely that we will need it again
    // soon so we can safely delete it.
    const DELETION_THRESHOLD: u32 = Self::FRAME_INCREMENT * 600;

    fn zero() -> Self {
        Self(0)
    }

    fn set_used(&mut self) {
        self.0 = 0;
    }

    fn increment_unused(&mut self) {
        self.0 += Self::FRAME_INCREMENT;
    }

    fn should_delete(&self) -> bool {
        self.0 >= Self::DELETION_THRESHOLD
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct VulkanShaderSetData {
    /// Set bindings ordered by backend binding index, not including the global uniform buffer binding.
    bindings: Vec<(/*binding_index=*/ u32, Binding)>,
}

struct VulkanDescriptorSetGroup {
    pub layout: ash::vk::DescriptorSetLayout,
    pub pipeline_ref_count: u32,
    // TODO: Support creating a new descriptor set on uniform update within the same frame with the
    // same bindings.
    pub binding_set_maps: Vec<
        HashMap<
            VulkanShaderSetData,
            (
                FreeListHandle<VulkanDescriptorSet>,
                /*uniform_buffer=*/ Option<ResourceId<Buffer>>,
            ),
        >,
    >,
}

impl VulkanDescriptorSetGroup {
    pub fn new(layout: ash::vk::DescriptorSetLayout, frames_in_flight: u32) -> Self {
        Self {
            layout,
            pipeline_ref_count: 0,
            binding_set_maps: (0..frames_in_flight).map(|_| HashMap::new()).collect(),
        }
    }
}

struct VulkanDescriptorSet {
    descriptor_set: ash::vk::DescriptorSet,
    usage_score: VulkanUsageScore,
}

#[derive(Clone)]
pub struct VulkanPipelineLayout {
    pub layout: ash::vk::PipelineLayout,
    pub shader_bindings: Vec<ShaderSetBinding>,
    // TODO: ref count so we can auto destruct non needed pipeline layouts.
    // pub ref_count: u32
}

#[derive(Clone)]
pub struct VulkanComputePipeline {
    pub pipeline_layout: u64,
    pub pipeline: ash::vk::Pipeline,
    pub workgroup_size: Vector3<u32>,
}

#[derive(Clone)]
pub struct VulkanRasterPipeline {
    pub pipeline_layout: u64,
    pub pipeline: ash::vk::Pipeline,
}

#[derive(Clone)]
pub struct VulkanBuffer {
    pub buffer: ash::vk::Buffer,
    pub size: u64,
    pub allocation: VulkanMemoryAllocation,
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

    pub fn resolution_xy(&self) -> Vector2<u32> {
        Vector2::new(self.info.extent.width, self.info.extent.height)
    }
}

impl From<GfxSamplerCreateInfo> for ash::vk::SamplerCreateInfo<'_> {
    fn from(value: GfxSamplerCreateInfo) -> Self {
        ash::vk::SamplerCreateInfo::default()
            .min_filter(value.min_filter.into())
            .mag_filter(value.mag_filter.into())
            .mipmap_mode(value.mipmap_filter.into())
            .address_mode_u(value.address_mode.into())
            .address_mode_v(value.address_mode.into())
            .address_mode_w(value.address_mode.into())
            .min_lod(1.0)
            .max_lod(1.0)
            .anisotropy_enable(false)
    }
}

impl From<GfxFilterMode> for ash::vk::Filter {
    fn from(value: GfxFilterMode) -> Self {
        match value {
            GfxFilterMode::Nearest => ash::vk::Filter::NEAREST,
            GfxFilterMode::Linear => ash::vk::Filter::LINEAR,
        }
    }
}

impl From<GfxFilterMode> for ash::vk::SamplerMipmapMode {
    fn from(value: GfxFilterMode) -> Self {
        match value {
            GfxFilterMode::Nearest => ash::vk::SamplerMipmapMode::NEAREST,
            GfxFilterMode::Linear => ash::vk::SamplerMipmapMode::LINEAR,
        }
    }
}

impl From<GfxAddressMode> for ash::vk::SamplerAddressMode {
    fn from(value: GfxAddressMode) -> Self {
        match value {
            GfxAddressMode::ClampToEdge => ash::vk::SamplerAddressMode::CLAMP_TO_EDGE,
            GfxAddressMode::Repeat => ash::vk::SamplerAddressMode::REPEAT,
            GfxAddressMode::MirroredRepeat => ash::vk::SamplerAddressMode::MIRRORED_REPEAT,
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
pub struct VulkanImageInfo {
    pub image_type: GfxImageType,
    pub format: ash::vk::Format,
    pub extent: ash::vk::Extent2D,
}

impl VulkanImageInfo {
    pub fn pixel_byte_size(&self) -> usize {
        match self.format {
            ash::vk::Format::R8G8B8A8_UINT
            | ash::vk::Format::R8G8B8A8_SINT
            | ash::vk::Format::R8G8B8A8_SRGB
            | ash::vk::Format::R8G8B8A8_UNORM => 4,
            _ => todo!("Support image format"),
        }
    }
}

impl From<GfxCullMode> for ash::vk::CullModeFlags {
    fn from(value: GfxCullMode) -> Self {
        match value {
            GfxCullMode::None => ash::vk::CullModeFlags::NONE,
            GfxCullMode::Front => ash::vk::CullModeFlags::FRONT,
            GfxCullMode::Back => ash::vk::CullModeFlags::BACK,
        }
    }
}

impl From<GfxFrontFace> for ash::vk::FrontFace {
    fn from(value: GfxFrontFace) -> Self {
        match value {
            GfxFrontFace::Clockwise => ash::vk::FrontFace::CLOCKWISE,
            GfxFrontFace::CounterClockwise => ash::vk::FrontFace::COUNTER_CLOCKWISE,
        }
    }
}

impl From<&GfxRasterPipelineBlendStateAttachmentInfo>
    for ash::vk::PipelineColorBlendAttachmentState
{
    fn from(value: &GfxRasterPipelineBlendStateAttachmentInfo) -> Self {
        ash::vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .color_write_mask(ash::vk::ColorComponentFlags::RGBA)
            .src_color_blend_factor(value.src_color_blend_factor.into())
            .dst_color_blend_factor(value.dst_color_blend_factor.into())
            .color_blend_op(value.color_blend_op.into())
            .src_alpha_blend_factor(value.src_alpha_blend_factor.into())
            .dst_alpha_blend_factor(value.dst_alpha_blend_factor.into())
            .alpha_blend_op(value.alpha_blend_op.into())
    }
}

impl From<GfxBlendFactor> for ash::vk::BlendFactor {
    fn from(value: GfxBlendFactor) -> Self {
        match value {
            GfxBlendFactor::One => ash::vk::BlendFactor::ONE,
            GfxBlendFactor::OneMinusSrcAlpha => ash::vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            GfxBlendFactor::SrcColor => ash::vk::BlendFactor::SRC_COLOR,
            GfxBlendFactor::DstColor => ash::vk::BlendFactor::DST_COLOR,
            GfxBlendFactor::SrcAlpha => ash::vk::BlendFactor::DST_ALPHA,
            GfxBlendFactor::DstAlpha => ash::vk::BlendFactor::DST_ALPHA,
            GfxBlendFactor::Zero => ash::vk::BlendFactor::ZERO,
        }
    }
}

impl From<GfxBlendOp> for ash::vk::BlendOp {
    fn from(value: GfxBlendOp) -> Self {
        match value {
            GfxBlendOp::Add => ash::vk::BlendOp::ADD,
            GfxBlendOp::Subtract => ash::vk::BlendOp::SUBTRACT,
        }
    }
}

impl From<GfxVertexAttributeFormat> for ash::vk::Format {
    fn from(value: GfxVertexAttributeFormat) -> Self {
        match value {
            GfxVertexAttributeFormat::Float2 => ash::vk::Format::R32G32_SFLOAT,
            GfxVertexAttributeFormat::Float3 => ash::vk::Format::R32G32B32_SFLOAT,
            GfxVertexAttributeFormat::Uint => ash::vk::Format::R32_UINT,
        }
    }
}

impl From<GfxLoadOp> for ash::vk::AttachmentLoadOp {
    fn from(value: GfxLoadOp) -> Self {
        match value {
            GfxLoadOp::Clear => ash::vk::AttachmentLoadOp::CLEAR,
            GfxLoadOp::Load => ash::vk::AttachmentLoadOp::LOAD,
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

impl From<GfxImageFormat> for ash::vk::Format {
    fn from(value: GfxImageFormat) -> Self {
        match value {
            GfxImageFormat::R16Float => ash::vk::Format::R16_SFLOAT,
            GfxImageFormat::Rgba8Unorm => ash::vk::Format::R8G8B8A8_UNORM,
            GfxImageFormat::Rgba8Srgb => ash::vk::Format::R8G8B8A8_SRGB,
            GfxImageFormat::D16Unorm => ash::vk::Format::D16_UNORM,
            GfxImageFormat::D24UnormS8Uint => ash::vk::Format::D24_UNORM_S8_UINT,
            GfxImageFormat::D32Float => ash::vk::Format::D32_SFLOAT,
            GfxImageFormat::Rgba32Float => ash::vk::Format::R32G32B32A32_SFLOAT,
        }
    }
}

impl From<ash::vk::Format> for GfxImageFormat {
    fn from(value: ash::vk::Format) -> Self {
        match value {
            ash::vk::Format::R16_SFLOAT => GfxImageFormat::R16Float,
            ash::vk::Format::R8G8B8A8_UNORM => GfxImageFormat::Rgba8Unorm,
            ash::vk::Format::D16_UNORM => GfxImageFormat::D16Unorm,
            ash::vk::Format::D24_UNORM_S8_UINT => GfxImageFormat::D24UnormS8Uint,
            ash::vk::Format::D32_SFLOAT => GfxImageFormat::D32Float,
            ash::vk::Format::R32G32B32A32_SFLOAT => GfxImageFormat::Rgba32Float,
            // TODO: Decide if we want to allow this or instead return an Option.
            ash::vk::Format::B8G8R8A8_UNORM => GfxImageFormat::Rgba8Unorm,
            format => todo!(
                "Support vulkan image format conversion for format {:?}.",
                format
            ),
        }
    }
}

struct VulkanBorrowedImageCreateInfo {
    image: ash::vk::Image,
    usage: ash::vk::ImageUsageFlags,
    info: VulkanImageInfo,
}

enum VulkanStagingCopyTask {
    Buffer {
        dst_buffer: ResourceId<Buffer>,
        src_offset: u64,
        dst_offset: u64,
        copy_size: u64,
    },
    Image {
        dst_image: ResourceId<Image>,
        src_offset: u64,
        image_offset: ash::vk::Offset3D,
        image_extent: ash::vk::Extent3D,
    },
}

enum VulkanSetWriteIndex {
    ImageInfo(usize),
    BufferInfo(usize),
}

impl VulkanResourceManager {
    fn new(ctx: &Arc<VulkanContextInner>) -> Self {
        let descriptor_pool = {
            const DESC_COUNT: u32 = 10000;
            let pool_sizes = [
                ash::vk::DescriptorPoolSize::default()
                    .ty(ash::vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(DESC_COUNT),
                ash::vk::DescriptorPoolSize::default()
                    .ty(ash::vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(DESC_COUNT),
                ash::vk::DescriptorPoolSize::default()
                    .ty(ash::vk::DescriptorType::SAMPLER)
                    .descriptor_count(DESC_COUNT),
                ash::vk::DescriptorPoolSize::default()
                    .ty(ash::vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(DESC_COUNT),
                ash::vk::DescriptorPoolSize::default()
                    .ty(ash::vk::DescriptorType::STORAGE_IMAGE)
                    .descriptor_count(DESC_COUNT),
            ];

            let create_info = ash::vk::DescriptorPoolCreateInfo::default()
                .max_sets(DESC_COUNT)
                .flags(ash::vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                .pool_sizes(&pool_sizes);
            unsafe {
                ctx.device
                    .create_descriptor_pool(&create_info, None)
                    .expect("Failed to create descriptor pool")
            }
        };

        Self {
            ctx: ctx.clone(),

            current_resource_id: AtomicU32::new(0),

            borrowed_images: parking_lot::RwLock::new(HashMap::new()),
            owned_images: parking_lot::RwLock::new(HashMap::new()),
            owned_buffers: parking_lot::RwLock::new(HashMap::new()),
            samplers: parking_lot::RwLock::new(HashMap::new()),

            pipeline_layouts: parking_lot::RwLock::new(Vec::new()),
            shader_pipeline_layout_map: parking_lot::RwLock::new(HashMap::new()),
            compute_pipelines: parking_lot::RwLock::new(HashMap::new()),
            raster_pipelines: parking_lot::RwLock::new(HashMap::new()),

            descriptor_pool,
            descriptor_set_groups: parking_lot::RwLock::new(HashMap::new()),
            descriptor_sets: parking_lot::RwLock::new(FreeList::new()),
            cache_slots: parking_lot::RwLock::new(HashMap::new()),

            event_pools: (0..ctx.frames_in_flight)
                .map(|_| {
                    parking_lot::RwLock::new(VulkanEventPool {
                        free_events: Vec::new(),
                        event_pool: Vec::new(),
                    })
                })
                .collect(),

            staging_buffers: parking_lot::RwLock::new(FreeList::new()),
            in_use_staging_buffers: parking_lot::RwLock::new(HashSet::new()),
            staging_buffer_gpu_timeline: (0..ctx.frames_in_flight)
                .map(|_| parking_lot::RwLock::new(Vec::new()))
                .collect(),

            copy_tasks: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    fn next_resource_id<T: 'static>(&self) -> ResourceId<T> {
        let resource_id = ResourceId::new(
            self.current_resource_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        resource_id
    }

    fn retire_resources(&self) {
        let curr_gpu_frame = self.ctx.curr_gpu_frame();
        let curr_cpu_frame = self.ctx.curr_cpu_frame();
        // This is called after we wait for our gpu timeline semaphore n - 2 so
        // we know this is our minimum.
        let minimum_gpu_frame = (curr_cpu_frame.saturating_sub(self.ctx.frames_in_flight as u64));
        for i in minimum_gpu_frame..curr_cpu_frame {
            if curr_gpu_frame < i {
                continue;
            }
            let frame_index = i % self.ctx.frames_in_flight as u64;

            let mut event_pool = self.event_pools[frame_index as usize].write();
            for i in 0..event_pool.event_pool.len() {
                event_pool.free_events.push(i as u32);
            }

            // Free up staging buffers from the finished gpu frame.
            let mut to_free_staging_buffer_handles =
                self.staging_buffer_gpu_timeline[frame_index as usize].write();
            let mut staging_buffers = self.staging_buffers.write();
            let mut in_use_staging_buffers = self.in_use_staging_buffers.write();
            for staging_buffer_handle in to_free_staging_buffer_handles.drain(..) {
                in_use_staging_buffers.remove(&staging_buffer_handle);
                let staging_buffer = staging_buffers.get_mut(staging_buffer_handle);
                staging_buffer.curr_write_pointer = 0;
            }
        }

        // TODO: Reimplement and fix by freeing any descriptor groups
        // references referencing the deleted descriptor set.
        // Garbage collect descriptor sets.
        // let mut descriptor_sets = self.descriptor_sets.write();
        // let mut to_remove_handles = Vec::new();
        // for (handle, descriptor_set) in descriptor_sets.iter_with_handle() {
        //     if descriptor_set.usage_score.should_delete() {
        //         to_remove_handles.push(handle);
        //     }
        //     descriptor_set.usage_score.increment_unused();
        // }

        // for handle in to_remove_handles {
        //     // TODO: Remove descriptor set reference in the descriptor group.
        //     let descriptor_set = descriptor_sets.remove(handle);
        //     unsafe {
        //         self.ctx
        //             .device
        //             .free_descriptor_sets(self.descriptor_pool, &[descriptor_set.descriptor_set])
        //     };
        // }

        // Wipe cache slots for the new frame.
        self.cache_slots.write().clear();
    }

    /// Returns a vkEvent that is valid for the current cpu recording next gpu frame.
    /// Used only for command buffer synchronization within a queue submit.
    fn get_frame_vk_event(&self) -> ash::vk::Event {
        let mut event_pool = self.event_pools[self.ctx.curr_cpu_frame_index() as usize].write();
        if let Some(free_event_idx) = event_pool.free_events.pop() {
            return event_pool.event_pool[free_event_idx as usize];
        }

        let create_info =
            ash::vk::EventCreateInfo::default().flags(ash::vk::EventCreateFlags::DEVICE_ONLY);
        let vk_event = unsafe { self.ctx.device.create_event(&create_info, None) }
            .expect("Failed to create vulkan event.");
        event_pool.event_pool.push(vk_event);
        debug!("Creating vkEvent.");

        vk_event
    }

    fn create_shader_module(&self, shader: &Shader) -> anyhow::Result<ash::vk::ShaderModule> {
        let create_info = ash::vk::ShaderModuleCreateInfo::default().code(shader.as_u32_slice());
        let shader_module = unsafe { self.ctx.device.create_shader_module(&create_info, None) }?;
        Ok(shader_module)
    }

    fn create_descriptor_set_layout(
        &self,
        set_binding: &ShaderSetBinding,
    ) -> anyhow::Result<ash::vk::DescriptorSetLayout> {
        debug!("Creating descriptor set layout");
        let mut layout_bindings = set_binding
            .bindings
            .iter()
            .filter_map(|(binding_name, (binding, _is_used))| 'map: {
                let vk_binding_type = match binding.binding_slot_type() {
                    Some(ty) => match ty {
                        ShaderBindingType::Sampler => ash::vk::DescriptorType::SAMPLER,
                        ShaderBindingType::SampledImage => ash::vk::DescriptorType::SAMPLED_IMAGE,
                        ShaderBindingType::StorageImage => ash::vk::DescriptorType::STORAGE_IMAGE,
                        ShaderBindingType::UniformBuffer => ash::vk::DescriptorType::UNIFORM_BUFFER,
                        ShaderBindingType::StorageBuffer => ash::vk::DescriptorType::STORAGE_BUFFER,
                    },
                    None => {
                        break 'map None;
                    }
                };
                let vk_binding = ash::vk::DescriptorSetLayoutBinding::default()
                    .binding(binding.binding_index(set_binding))
                    .descriptor_count(1)
                    .descriptor_type(vk_binding_type)
                    .stage_flags(ash::vk::ShaderStageFlags::ALL);

                Some(vk_binding)
            })
            .collect::<Vec<_>>();

        if let Some(global_uniform_binding_index) = set_binding.global_uniform_binding_index {
            layout_bindings.push(
                ash::vk::DescriptorSetLayoutBinding::default()
                    .binding(global_uniform_binding_index)
                    .descriptor_count(1)
                    .descriptor_type(ash::vk::DescriptorType::UNIFORM_BUFFER)
                    .stage_flags(ash::vk::ShaderStageFlags::ALL),
            )
        }

        let create_info =
            ash::vk::DescriptorSetLayoutCreateInfo::default().bindings(&layout_bindings);
        let set_layout = unsafe {
            self.ctx
                .device
                .create_descriptor_set_layout(&create_info, None)
        }?;
        Ok(set_layout)
    }

    /// Accepts multiple shaders that may can use the same set bindings.
    fn create_pipeline_layout(&self, shaders: &[&Shader]) -> anyhow::Result<u64> {
        debug!("Creating pipeline layout");
        use std::collections::hash_map::Entry;

        let mut aggregated_sets = vec![];
        for shader in shaders {
            for set in shader.bindings() {
                if let Some(existing_set) = aggregated_sets
                    .iter_mut()
                    .find(|s: &&mut ShaderSetBinding| set.set_index == s.set_index)
                {
                    for (binding_name, (binding, is_used)) in &set.bindings {
                        let Some((existing_binding, existing_used)) =
                            existing_set.bindings.get_mut(binding_name)
                        else {
                            panic!(
                                "Shader sets with the same index should have the same bindings."
                            );
                        };
                        assert_eq!(
                            binding, existing_binding,
                            "Set bindings should be equivalent."
                        );
                        // If one of the shader sets uses the binding, mark the binding as
                        // used.
                        *existing_used = *existing_used || *is_used;
                    }
                } else {
                    aggregated_sets.push(set.clone());
                }
            }
        }

        let mut shader_pipeline_layout_map = self.shader_pipeline_layout_map.write();
        if let Some(layout_index) = shader_pipeline_layout_map.get(&aggregated_sets) {
            return Ok(*layout_index);
        };

        let mut vk_set_layouts = Vec::new();
        let mut descriptor_set_map = self.descriptor_set_groups.write();

        for shader_set in &aggregated_sets {
            // Get or create the descriptor set layout that relates to the shader set bindings.
            let layout = if let Some(layout) = descriptor_set_map.get_mut(&shader_set) {
                layout.pipeline_ref_count += 1;
                layout.layout
            } else {
                let new_layout = self.create_descriptor_set_layout(shader_set)?;
                descriptor_set_map.insert(
                    shader_set.clone(),
                    VulkanDescriptorSetGroup::new(new_layout, self.ctx.frames_in_flight),
                );

                new_layout
            };

            vk_set_layouts.push(layout);
        }

        // TODO: Use push constants in the future.
        let create_info = ash::vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&vk_set_layouts)
            .push_constant_ranges(&[]);
        let pipeline_layout =
            unsafe { self.ctx.device.create_pipeline_layout(&create_info, None) }?;
        let mut pipeline_layouts = self.pipeline_layouts.write();
        pipeline_layouts.push(VulkanPipelineLayout {
            layout: pipeline_layout,
            shader_bindings: aggregated_sets.into_iter().map(|set| set.clone()).collect(),
        });
        let layout_index = pipeline_layouts.len() as u64 - 1;

        Ok(layout_index)
    }

    fn create_raster_pipeline(
        &self,
        create_info: GfxRasterPipelineCreateInfo,
    ) -> anyhow::Result<ResourceId<RasterPipeline>> {
        let resource_id = self.next_resource_id();

        let vertex_shader = create_info.vertex_shader;
        let fragment_shader = create_info.fragment_shader;
        debug!(
            "Creating raster pipeline id: {}, vertex: `{}::{}`, fragment: `{}::{}`.",
            resource_id,
            vertex_shader.module_name(),
            vertex_shader.entry_point_name(),
            fragment_shader.module_name(),
            fragment_shader.entry_point_name()
        );

        let vertex_shader_module = self.create_shader_module(vertex_shader)?;
        let fragment_shader_module = self.create_shader_module(fragment_shader)?;

        let pipeline_layout_index =
            self.create_pipeline_layout(&[&vertex_shader, &fragment_shader])?;
        let vk_pipeline_layout = self
            .pipeline_layouts
            .read()
            .get(pipeline_layout_index as usize)
            .unwrap()
            .layout;

        let c_vertex_entry_point_name = CString::new(vertex_shader.entry_point_name()).unwrap();
        let c_fragment_entry_point_name = CString::new(fragment_shader.entry_point_name()).unwrap();
        let shader_stages = [
            ash::vk::PipelineShaderStageCreateInfo::default()
                .module(vertex_shader_module)
                .name(&c_vertex_entry_point_name)
                .stage(ash::vk::ShaderStageFlags::VERTEX),
            ash::vk::PipelineShaderStageCreateInfo::default()
                .module(fragment_shader_module)
                .name(&c_fragment_entry_point_name)
                .stage(ash::vk::ShaderStageFlags::FRAGMENT),
        ];

        let input_assembly_state = ash::vk::PipelineInputAssemblyStateCreateInfo::default()
            .primitive_restart_enable(false)
            .topology(ash::vk::PrimitiveTopology::TRIANGLE_LIST);

        let mut vertex_attribute_descriptions = create_info.vertex_format.attributes;
        vertex_attribute_descriptions.sort_by_key(|attr| attr.location);
        let mut vertex_stride = 0u32;
        let vertex_attribute_descriptions = vertex_attribute_descriptions
            .iter()
            .map(|attr| {
                let desc = ash::vk::VertexInputAttributeDescription::default()
                    .binding(0)
                    .location(attr.location)
                    .format(attr.format.into())
                    .offset(vertex_stride);
                vertex_stride += attr.format.byte_size();
                desc
            })
            .collect::<Vec<_>>();
        debug!("Atrrs {:?}", vertex_attribute_descriptions);
        let vertex_binding_descriptions = [ash::vk::VertexInputBindingDescription::default()
            .binding(0)
            .input_rate(ash::vk::VertexInputRate::VERTEX)
            .stride(vertex_stride)];
        let vertex_input_state = ash::vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&vertex_binding_descriptions)
            .vertex_attribute_descriptions(&vertex_attribute_descriptions);

        let attachment_blend_states = create_info
            .blend_state
            .attachments
            .iter()
            .map(|info| info.into())
            .collect::<Vec<_>>();
        let blend_state = ash::vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(&attachment_blend_states)
            .logic_op_enable(false);
        let raster_state = ash::vk::PipelineRasterizationStateCreateInfo::default()
            .rasterizer_discard_enable(false)
            .polygon_mode(ash::vk::PolygonMode::FILL)
            .cull_mode(create_info.cull_mode.into())
            .front_face(create_info.front_face.into())
            .depth_clamp_enable(false)
            .depth_bias_enable(false);
        let multisample_state = ash::vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(ash::vk::SampleCountFlags::TYPE_1);

        let viewport = ash::vk::Viewport::default()
            .x(0.0)
            .y(0.0)
            .width(1.0)
            .height(1.0)
            .min_depth(0.0)
            .max_depth(1.0);
        let scissor = ash::vk::Rect2D::default()
            .offset(ash::vk::Offset2D::default())
            .extent(ash::vk::Extent2D::default().width(1).height(1));
        // Viewport is ignored because of the dynamic state but we need them anyways to
        // make vulkan happy.
        let viewport_state = ash::vk::PipelineViewportStateCreateInfo::default()
            .viewports(std::slice::from_ref(&viewport))
            .scissors(std::slice::from_ref(&scissor));

        let dynamic_states = [
            ash::vk::DynamicState::VIEWPORT,
            ash::vk::DynamicState::SCISSOR,
        ];
        let dynamic_state =
            ash::vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let color_attachment_formats = create_info
            .color_formats
            .into_iter()
            .map(|format| format.into())
            .collect::<Vec<_>>();
        let mut dynamic_rendering = ash::vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&color_attachment_formats);
        if let Some(format) = create_info.depth_format {
            dynamic_rendering = dynamic_rendering.depth_attachment_format(format.into());
        }

        let create_infos = [ash::vk::GraphicsPipelineCreateInfo::default()
            .push_next(&mut dynamic_rendering)
            .layout(vk_pipeline_layout)
            .stages(&shader_stages)
            .input_assembly_state(&input_assembly_state)
            .vertex_input_state(&vertex_input_state)
            .color_blend_state(&blend_state)
            .multisample_state(&multisample_state)
            .rasterization_state(&raster_state)
            .vertex_input_state(&vertex_input_state)
            .viewport_state(&viewport_state)
            .dynamic_state(&dynamic_state)];

        let raster_pipeline = unsafe {
            self.ctx.device.create_graphics_pipelines(
                ash::vk::PipelineCache::null(),
                &create_infos,
                None,
            )
        }
        .map_err(|(_, err)| anyhow!("Failed to create vulkan raster pipeline. Error: {}", err))?
        .remove(0);

        unsafe {
            self.ctx
                .device
                .destroy_shader_module(vertex_shader_module, None)
        };
        unsafe {
            self.ctx
                .device
                .destroy_shader_module(fragment_shader_module, None)
        };

        self.raster_pipelines.write().insert(
            resource_id,
            VulkanRasterPipeline {
                pipeline_layout: pipeline_layout_index,
                pipeline: raster_pipeline,
            },
        );

        Ok(resource_id)
    }

    fn create_compute_pipeline(
        &self,
        create_info: GfxComputePipelineCreateInfo,
    ) -> anyhow::Result<ResourceId<ComputePipeline>> {
        let shader = create_info.shader;
        debug!(
            "Creating compute pipeline `{}::{}`.",
            shader.module_name(),
            shader.entry_point_name()
        );
        let resource_id = self.next_resource_id();

        let shader_module = self.create_shader_module(shader)?;

        let pipeline_layout_index = self.create_pipeline_layout(&[&shader])?;
        let vk_pipeline_layout = self
            .pipeline_layouts
            .read()
            .get(pipeline_layout_index as usize)
            .unwrap()
            .layout;

        let c_entry_point_name = CString::new(shader.entry_point_name()).unwrap();
        let create_infos = [ash::vk::ComputePipelineCreateInfo::default()
            .layout(vk_pipeline_layout)
            .stage(
                ash::vk::PipelineShaderStageCreateInfo::default()
                    .module(shader_module)
                    .name(&c_entry_point_name)
                    .stage(ash::vk::ShaderStageFlags::COMPUTE),
            )];
        let compute_pipeline = unsafe {
            self.ctx.device.create_compute_pipelines(
                ash::vk::PipelineCache::null(),
                &create_infos,
                None,
            )
        }
        .map_err(|_| anyhow!("Failed to create vulkan compute pipeline."))?
        .remove(0);

        unsafe { self.ctx.device.destroy_shader_module(shader_module, None) };

        self.compute_pipelines.write().insert(
            resource_id,
            VulkanComputePipeline {
                pipeline_layout: pipeline_layout_index,
                pipeline: compute_pipeline,
                workgroup_size: shader.pipeline_info().workgroup_size.unwrap(),
            },
        );

        Ok(resource_id)
    }

    /// Writes the descriptor sets for the given binding data, not writing any set data that has
    /// already been cached.
    pub fn write_shader_sets(
        &self,
        allocator: &mut VulkanAllocator,
        binding_data: HashMap<&ShaderSetBinding, ShaderSetData>,
    ) {
        let mut descriptor_set_groups = self.descriptor_set_groups.write();
        let mut descriptor_sets = self.descriptor_sets.write();
        let mut cache_slots = self.cache_slots.write();

        let mut vk_image_infos = Vec::new();
        let mut vk_buffer_infos = Vec::new();
        let mut vk_descriptor_set_writes = Vec::new();

        for (set_binding, mut new_set) in binding_data {
            if new_set.is_using_cache() {
                continue;
            }

            if !descriptor_set_groups.contains_key(set_binding) {
                let new_layout = self.create_descriptor_set_layout(set_binding).expect(&format!("Failed to create descriptor set layout for global shader set write for set `{}`", set_binding.name));
                descriptor_set_groups.insert(
                    set_binding.clone(),
                    VulkanDescriptorSetGroup::new(new_layout, self.ctx.frames_in_flight),
                );
                debug!("Making new descriptor group for set `{}`", set_binding.name);
            }
            let descriptor_set_group = descriptor_set_groups.get_mut(set_binding).unwrap();

            // Create hashed descriptor set bindings.
            let mut vulkan_set_data = VulkanShaderSetData {
                bindings: Vec::new(),
            };
            for (binding_idx, binding) in new_set.bindings() {
                vulkan_set_data
                    .bindings
                    .push((*binding_idx, binding.clone()));
            }
            vulkan_set_data
                .bindings
                .sort_by_key(|(binding_idx, _)| *binding_idx);

            let (descriptor_set_handle, uniform_buffer_id) = descriptor_set_group.binding_set_maps
                [self.ctx.curr_cpu_frame_index() as usize]
                .entry(vulkan_set_data.clone())
                .or_insert_with_key(|bindings| {
                    let set_layouts = [descriptor_set_group.layout];
                    let create_info = ash::vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(self.descriptor_pool)
                        .set_layouts(&set_layouts);

                    let hashed_set_info = new_set.clone();
                    let new_set = unsafe { self.ctx.device.allocate_descriptor_sets(&create_info) }
                        .expect("Failed to create global descriptor set")
                        .remove(0);
                    debug!("Creating descriptor set for set `{}`.", set_binding.name);

                    let uniform_buffer_id =
                        set_binding
                            .global_uniform_binding_index
                            .map(|uniform_binding_index| {
                                let new_buffer = self
                                    .create_buffer(
                                        allocator,
                                        GfxBufferCreateInfo {
                                            name: format!(
                                                "set_{}_uniform_buffer",
                                                set_binding.name
                                            ),
                                            size: set_binding.global_uniforms_size as u64,
                                        },
                                        VulkanAllocationType::GpuLocal,
                                        false,
                                    )
                                    .expect("Failed to create descriptor set uniform buffer");

                                let buffer_info = self.get_buffer_info(&new_buffer);
                                vk_buffer_infos.push(
                                    ash::vk::DescriptorBufferInfo::default()
                                        .buffer(buffer_info.buffer)
                                        .offset(0)
                                        .range(ash::vk::WHOLE_SIZE),
                                );
                                vk_descriptor_set_writes.push((
                                    ash::vk::WriteDescriptorSet::default()
                                        .dst_set(new_set)
                                        .descriptor_count(1)
                                        .dst_binding(uniform_binding_index)
                                        .descriptor_type(ash::vk::DescriptorType::UNIFORM_BUFFER),
                                    VulkanSetWriteIndex::BufferInfo(vk_buffer_infos.len() - 1),
                                ));

                                new_buffer
                            });

                    for (binding_idx, binding) in bindings.bindings.iter() {
                        let mut write = ash::vk::WriteDescriptorSet::default()
                            .dst_set(new_set)
                            .descriptor_count(1)
                            .dst_binding(*binding_idx);
                        let mut info = None;

                        // Due to the image infos possibly resizing while pushing, we must store
                        // indexes to the image info and populate that in the vulkan write struct
                        // once we are no longer pushing writes.
                        match binding {
                            Binding::StorageImage { image } => {
                                let vk_image_info = self.get_image(*image);
                                vk_image_infos.push(
                                    ash::vk::DescriptorImageInfo::default()
                                        .image_view(
                                            vk_image_info
                                                .view
                                                .expect("Storage image should have an image view."),
                                        )
                                        .image_layout(ash::vk::ImageLayout::GENERAL),
                                );
                                info =
                                    Some(VulkanSetWriteIndex::ImageInfo(vk_image_infos.len() - 1));
                                write =
                                    write.descriptor_type(ash::vk::DescriptorType::STORAGE_IMAGE);
                            }
                            Binding::SampledImage { image } => {
                                let vk_image_info = self.get_image(*image);
                                vk_image_infos.push(
                                    ash::vk::DescriptorImageInfo::default()
                                        .image_view(
                                            vk_image_info
                                                .view
                                                .expect("Sampled image should have an image view."),
                                        )
                                        .image_layout(
                                            ash::vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                        ),
                                );
                                info =
                                    Some(VulkanSetWriteIndex::ImageInfo(vk_image_infos.len() - 1));
                                write =
                                    write.descriptor_type(ash::vk::DescriptorType::SAMPLED_IMAGE);
                            }
                            Binding::Sampler { sampler } => {
                                vk_image_infos.push(
                                    ash::vk::DescriptorImageInfo::default()
                                        .sampler(self.get_sampler(sampler)),
                                );
                                info =
                                    Some(VulkanSetWriteIndex::ImageInfo(vk_image_infos.len() - 1));
                                write = write.descriptor_type(ash::vk::DescriptorType::SAMPLER);
                            }
                            Binding::UniformBuffer { buffer } => {
                                let vk_buffer_info = self.get_buffer_info(buffer);
                                vk_buffer_infos.push(
                                    ash::vk::DescriptorBufferInfo::default()
                                        .buffer(vk_buffer_info.buffer)
                                        .offset(0)
                                        .range(ash::vk::WHOLE_SIZE),
                                );
                                info = Some(VulkanSetWriteIndex::BufferInfo(
                                    vk_buffer_infos.len() - 1,
                                ));
                                write =
                                    write.descriptor_type(ash::vk::DescriptorType::UNIFORM_BUFFER);
                            }
                            Binding::StorageBuffer { buffer } => {
                                let vk_buffer_info = self.get_buffer_info(buffer);
                                vk_buffer_infos.push(
                                    ash::vk::DescriptorBufferInfo::default()
                                        .buffer(vk_buffer_info.buffer)
                                        .offset(0)
                                        .range(ash::vk::WHOLE_SIZE),
                                );
                                info = Some(VulkanSetWriteIndex::BufferInfo(
                                    vk_buffer_infos.len() - 1,
                                ));
                                write =
                                    write.descriptor_type(ash::vk::DescriptorType::STORAGE_BUFFER);
                            }
                        }

                        vk_descriptor_set_writes.push((write, info.unwrap()));
                    }

                    let set_handle = descriptor_sets.push(VulkanDescriptorSet {
                        descriptor_set: new_set,
                        usage_score: VulkanUsageScore::zero(),
                    });

                    (set_handle, uniform_buffer_id)
                });

            if !new_set.uniform_data().data.is_empty() {
                let uniform_buffer_id = uniform_buffer_id.expect(
                    "Should not have uniform data written if there are no uniform bindings.",
                );

                let uniform_data = new_set.take_uniform_data();
                let len = uniform_data.data.len();

                let dst_ptr = self.write_buffer(allocator, &uniform_buffer_id, 0, len as u64);
                unsafe { std::slice::from_raw_parts_mut(dst_ptr, len) }
                    .copy_from_slice(&uniform_data.data);
            }

            if let Some(write_to_cache_slot) = new_set.cache_slot() {
                cache_slots.insert(
                    write_to_cache_slot,
                    (vulkan_set_data, *descriptor_set_handle),
                );
            }
        }

        if !vk_descriptor_set_writes.is_empty() {
            let vk_descriptor_set_writes = vk_descriptor_set_writes
                .into_iter()
                .map(|(write, info)| match info {
                    VulkanSetWriteIndex::ImageInfo(idx) => {
                        write.image_info(std::slice::from_ref(&vk_image_infos[idx]))
                    }
                    VulkanSetWriteIndex::BufferInfo(idx) => {
                        write.buffer_info(std::slice::from_ref(&vk_buffer_infos[idx]))
                    }
                })
                .collect::<Vec<_>>();

            unsafe {
                self.ctx
                    .device
                    .update_descriptor_sets(&vk_descriptor_set_writes, &[])
            };
        }
    }

    fn bind_uniforms(
        &self,
        allocator: &mut VulkanAllocator,
        pipeline: ResourceId<Untyped>,
        mut binding_data: HashMap<u32, ShaderSetData>,
    ) -> VulkanUniformBinding {
        let mut pipeline_layout_index = None;
        if let Some(pipeline) = self
            .compute_pipelines
            .read()
            .get(&ResourceId::new(pipeline.id()))
        {
            pipeline_layout_index = Some(pipeline.pipeline_layout);
        } else if let Some(pipeline) = self
            .raster_pipelines
            .read()
            .get(&ResourceId::new(pipeline.id()))
        {
            pipeline_layout_index = Some(pipeline.pipeline_layout);
        }
        assert!(
            pipeline_layout_index.is_some(),
            "Pipeline id {} is invalid.",
            pipeline.id()
        );

        let pipeline_layouts = self.pipeline_layouts.read();
        let pipeline_layout = pipeline_layouts
            .get(pipeline_layout_index.unwrap() as usize)
            .unwrap();

        // Updates the needed descriptor sets for this pipeline using the provided set data, any
        // sets marked as cached are not updated.
        self.write_shader_sets(
            allocator,
            binding_data
                .iter()
                .map(|(set_idx, set_data)| {
                    let set_binding = pipeline_layout
                        .shader_bindings
                        .iter()
                        .find(|set| set.set_index == *set_idx)
                        .expect("Set index doesn't exist.");
                    (set_binding, set_data.clone())
                })
                .collect::<HashMap<_, _>>(),
        );

        let mut descriptor_set_groups = self.descriptor_set_groups.read();
        let mut descriptor_sets = self.descriptor_sets.write();
        let cache_slots = self.cache_slots.read();

        let mut expected_image_layouts = HashMap::new();
        let mut vk_descriptor_sets = pipeline_layout
            .shader_bindings
            .iter()
            .filter_map(|set_binding| {
                let Some(mut new_set_data) = binding_data.remove(&set_binding.set_index) else {
                    if set_binding.name == "u_frame" {
                        // TODO: Figure out why this is included in the egui pipeline so we can
                        // remove this work around and require the cache slots to be used.
                        return None;
                    }

                    panic!(
                        "Expected set index {} for set `{}`, to be set but it was not.",
                        set_binding.set_index, set_binding.name
                    );
                };
                let descriptor_set_group = descriptor_set_groups.get(set_binding).unwrap();

                let vk_set_idx = if new_set_data.is_using_cache() {
                    let cache_slot = new_set_data.cache_slot().unwrap();
                    let (set_data, set_idx) = cache_slots.get(&cache_slot).expect(&format!(
                        "Expected set `{}` to be in cache slot `{}` but it was not.",
                        set_binding.name, cache_slot
                    ));

                    for (binding_idx, binding) in set_data.bindings.iter() {
                        match binding {
                            Binding::StorageImage { image } => {
                                expected_image_layouts.insert(
                                    *image,
                                    (
                                        ash::vk::ImageLayout::GENERAL,
                                        ash::vk::AccessFlags::SHADER_WRITE,
                                    ),
                                );
                            }
                            Binding::SampledImage { image } => {
                                expected_image_layouts.insert(
                                    *image,
                                    (
                                        ash::vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                        ash::vk::AccessFlags::SHADER_READ,
                                    ),
                                );
                            }
                            _ => {}
                        }
                    }
                    *set_idx
                } else {
                    let mut vulkan_set_data = VulkanShaderSetData {
                        bindings: Vec::new(),
                    };
                    for (binding_idx, binding) in new_set_data.bindings() {
                        vulkan_set_data
                            .bindings
                            .push((*binding_idx, binding.clone()));
                        match binding {
                            Binding::StorageImage { image } => {
                                expected_image_layouts.insert(
                                    *image,
                                    (
                                        ash::vk::ImageLayout::GENERAL,
                                        ash::vk::AccessFlags::SHADER_WRITE,
                                    ),
                                );
                            }
                            Binding::SampledImage { image } => {
                                expected_image_layouts.insert(
                                    *image,
                                    (
                                        ash::vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                        ash::vk::AccessFlags::SHADER_READ,
                                    ),
                                );
                            }
                            _ => {}
                        }
                    }
                    vulkan_set_data
                        .bindings
                        .sort_by_key(|(binding_idx, _)| *binding_idx);

                    descriptor_set_group.binding_set_maps[self.ctx.curr_cpu_frame_index() as usize]
                        .get(&vulkan_set_data)
                        .expect("TODO: Make a good message.")
                        .0
                };
                let mut vk_set = descriptor_sets.get_mut(vk_set_idx);
                vk_set.usage_score.set_used();

                Some((set_binding.set_index, vk_set.descriptor_set))
            })
            .collect::<Vec<_>>();
        vk_descriptor_sets.sort_by_key(|(idx, _)| *idx);

        let first_set = vk_descriptor_sets
            .iter()
            .fold(u32::MAX, |min_set, (set_idx, _)| min_set.min(*set_idx));
        let vk_descriptor_sets = vk_descriptor_sets
            .into_iter()
            .map(|(_, set)| set)
            .collect::<Vec<_>>();

        VulkanUniformBinding {
            pipeline_layout: pipeline_layout.layout,
            first_set,
            descriptor_sets: vk_descriptor_sets,
            expected_image_layouts,
        }
    }

    pub fn get_pipeline_layout(&self, pipeline: ResourceId<Untyped>) -> VulkanPipelineLayout {
        let mut pipeline_layout_index = None;
        if let Some(pipeline) = self
            .compute_pipelines
            .read()
            .get(&ResourceId::new(pipeline.id()))
        {
            pipeline_layout_index = Some(pipeline.pipeline_layout);
        } else if let Some(pipeline) = self
            .raster_pipelines
            .read()
            .get(&ResourceId::new(pipeline.id()))
        {
            pipeline_layout_index = Some(pipeline.pipeline_layout);
        }
        assert!(
            pipeline_layout_index.is_some(),
            "Pipeline id {} is invalid.",
            pipeline.id()
        );

        let mut descriptor_set_map = self.descriptor_set_groups.write();
        let pipeline_layouts = self.pipeline_layouts.read();
        pipeline_layouts
            .get(pipeline_layout_index.unwrap() as usize)
            .unwrap()
            .clone()
    }

    fn create_image_borrowed(
        &self,
        image_info: VulkanBorrowedImageCreateInfo,
    ) -> anyhow::Result<ResourceId<Image>> {
        debug!("Creating borrowed image.");
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

    fn destroy_image_borrowed(&self, image_id: &ResourceId<Image>) {
        debug!("Destroyed borrowed image.");
        let old = self.borrowed_images.write().remove(image_id);
        assert!(
            old.is_some(),
            "Tried to destroy image with an invalid image handle.",
        );

        let Some(borrowed_image_view) = old.unwrap().view else {
            return;
        };

        unsafe {
            self.ctx
                .device
                .destroy_image_view(borrowed_image_view, None)
        };
    }

    fn create_buffer(
        &self,
        allocator: &mut VulkanAllocator,
        create_info: GfxBufferCreateInfo,
        allocation_type: VulkanAllocationType,
        mapped_ptr: bool,
    ) -> anyhow::Result<ResourceId<Buffer>> {
        debug!(
            "Creating vulkan buffer `{}`, size={}, mapped={}.",
            create_info.name, create_info.size, mapped_ptr
        );
        anyhow::ensure!(create_info.size > 0);
        let create_info = ash::vk::BufferCreateInfo::default()
            .size(create_info.size)
            .usage(
                ash::vk::BufferUsageFlags::STORAGE_BUFFER
                    | ash::vk::BufferUsageFlags::UNIFORM_BUFFER
                    | ash::vk::BufferUsageFlags::TRANSFER_DST
                    | ash::vk::BufferUsageFlags::TRANSFER_SRC
                    | ash::vk::BufferUsageFlags::VERTEX_BUFFER
                    | ash::vk::BufferUsageFlags::INDEX_BUFFER,
            )
            .sharing_mode(ash::vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { self.ctx.device.create_buffer(&create_info, None) }?;

        let buffer_memory_requirements =
            unsafe { self.ctx.device.get_buffer_memory_requirements(buffer) };
        let allocation = allocator.allocate_memory(
            buffer_memory_requirements.size,
            buffer_memory_requirements.alignment as u32,
            allocation_type,
            mapped_ptr,
        )?;
        let allocation_info = allocator.get_allocation_info(&allocation);
        unsafe {
            self.ctx.device.bind_buffer_memory(
                buffer,
                allocation_info.memory.device_memory,
                allocation_info.offset,
            )
        }?;

        let resource_id = self.next_resource_id();
        self.owned_buffers.write().insert(
            resource_id,
            VulkanBuffer {
                buffer,
                size: create_info.size,
                allocation,
            },
        );

        Ok(resource_id)
    }

    fn get_or_create_staging_buffer(
        &self,
        allocator: &mut VulkanAllocator,
        min_size: u64,
    ) -> FreeListHandle<VulkanStagingBuffer> {
        let mut staging_buffer_free_list = self.staging_buffers.write();

        for (index, staging_buffer) in staging_buffer_free_list.iter().enumerate() {
            let handle = FreeListHandle::new(index);
            if self.in_use_staging_buffers.read().contains(&handle) {
                continue;
            }

            let remaining_size = staging_buffer.size - staging_buffer.curr_write_pointer;
            if remaining_size >= min_size {
                return handle;
            }
        }

        let staging_buffer_handle = staging_buffer_free_list.next_free_handle();

        let new_buffer_size = min_size.max(VK_STAGING_BUFFER_MIN_SIZE);
        let new_buffer = self
            .create_buffer(
                allocator,
                GfxBufferCreateInfo {
                    name: format!("staging_buffer_{}", staging_buffer_handle.index()),
                    size: new_buffer_size,
                },
                VulkanAllocationType::CpuLocal,
                true,
            )
            .expect("Failed to create staging buffer.");
        let new_buffer_info = self.get_buffer_info(&new_buffer);
        let new_allocation_info = allocator.get_allocation_info(&new_buffer_info.allocation);

        let new_buffer_mapped_pointer = new_allocation_info
            .mapped_ptr
            .expect("Should have created staging buffer with mapped pointer");
        let staging_desc = VulkanStagingBuffer {
            buffer: new_buffer,
            mapped_pointer: new_buffer_mapped_pointer as *mut u8,
            curr_write_pointer: 0,
            size: new_buffer_size,
        };

        let new_handle = staging_buffer_free_list.push(staging_desc);
        assert_eq!(
            new_handle, staging_buffer_handle,
            "Pre-emptive free index and actual free index should be the same."
        );

        new_handle
    }

    fn write_buffer(
        &self,
        allocator: &mut VulkanAllocator,
        dst_buffer: &ResourceId<Buffer>,
        dst_offset: u64,
        write_len: u64,
    ) -> *mut u8 {
        // TODO: only write staging buffers rwlock list here and pass it into
        // `get_or_create_staging_buffer`.
        let staging_buffer_index = self.get_or_create_staging_buffer(allocator, write_len);

        let mut staging_buffers = self.staging_buffers.write();
        let staging_buffer = staging_buffers.get_mut(staging_buffer_index);

        let write_ptr = unsafe {
            staging_buffer
                .mapped_pointer
                .byte_add(staging_buffer.curr_write_pointer as usize)
        };

        let src_offset = staging_buffer.curr_write_pointer;
        staging_buffer.curr_write_pointer += write_len;
        assert!(staging_buffer.curr_write_pointer <= staging_buffer.size);

        let mut copy_tasks = self.copy_tasks.write();
        let mut staging_buffer_copy_tasks = copy_tasks.entry(staging_buffer_index).or_default();
        staging_buffer_copy_tasks.push(VulkanStagingCopyTask::Buffer {
            dst_buffer: *dst_buffer,
            src_offset,
            dst_offset,
            copy_size: write_len,
        });

        write_ptr
    }

    fn write_image(&self, allocator: &mut VulkanAllocator, info: GfxImageWrite) {
        let image_info = self.get_image(info.image);
        assert!(
            info.extent.x > 0 && info.extent.y > 0,
            "Writing image extent must be greater than zero."
        );
        assert!(
            image_info.info.extent.width >= info.offset.x + info.extent.x
                && image_info.info.extent.height >= info.offset.y + info.extent.y,
            "Tried to overwrite an image's bounds, image has extent {:?} but recieved {:?}.",
            image_info.info.extent,
            info.extent,
        );
        let write_len =
            image_info.info.pixel_byte_size() as u64 * info.extent.x as u64 * info.extent.y as u64;
        assert_eq!(
            write_len,
            info.data.len() as u64,
            "Expected image data of {} bytes but we got {} bytes of data were provided.",
            write_len,
            info.data.len(),
        );

        // TODO: only write staging buffers rwlock list here and pass it into
        // `get_or_create_staging_buffer`.
        let staging_buffer_index = self.get_or_create_staging_buffer(allocator, write_len);

        let mut staging_buffers = self.staging_buffers.write();
        let staging_buffer = staging_buffers.get_mut(staging_buffer_index);

        let write_ptr = unsafe {
            staging_buffer
                .mapped_pointer
                .byte_add(staging_buffer.curr_write_pointer as usize)
        };

        let src_offset = staging_buffer.curr_write_pointer;
        staging_buffer.curr_write_pointer += write_len;
        assert!(staging_buffer.curr_write_pointer < staging_buffer.size);

        let mut copy_tasks = self.copy_tasks.write();
        let mut staging_buffer_copy_tasks = copy_tasks.entry(staging_buffer_index).or_default();
        staging_buffer_copy_tasks.push(VulkanStagingCopyTask::Image {
            dst_image: info.image,
            src_offset,
            image_offset: ash::vk::Offset3D {
                x: info.offset.x as i32,
                y: info.offset.y as i32,
                z: 0,
            },
            image_extent: ash::vk::Extent3D {
                width: info.extent.x,
                height: info.extent.y,
                depth: 1,
            },
        });

        unsafe { write_ptr.copy_from_nonoverlapping(info.data.as_ptr(), info.data.len()) };
    }

    fn get_buffer_info(&self, buffer: &ResourceId<Buffer>) -> VulkanBuffer {
        let owned_buffers = self.owned_buffers.read();
        let Some(buffer_info) = owned_buffers.get(buffer) else {
            panic!("Tried to get buffer info of a buffer that doesn't exist.");
        };

        // TODO: RAII read guard on the rwlock as long as the returned reference is not dropped.
        buffer_info.clone()
    }

    // TODO: buffer write group api with write group handle so we can write buffers on multiple
    // threads.
    fn record_buffer_writes(&self, recorder: &mut VulkanRecorder) {
        let staging_buffers = self.staging_buffers.read();
        let mut staging_buffer_gpu_timeline =
            self.staging_buffer_gpu_timeline[self.ctx.curr_cpu_frame_index() as usize].write();
        let mut in_use_staging_buffers = self.in_use_staging_buffers.write();
        let mut copy_tasks = self.copy_tasks.write();

        let mut buffer_barriers = Vec::new();

        // Transition image
        let mut image_barriers = Vec::new();
        for (_, copy_tasks) in copy_tasks.iter() {
            for task in copy_tasks {
                match task {
                    VulkanStagingCopyTask::Buffer { .. } => {}
                    VulkanStagingCopyTask::Image {
                        dst_image,
                        src_offset,
                        image_offset,
                        image_extent,
                    } => {
                        let image_info = self.get_image(*dst_image);
                        image_barriers.push(
                            ash::vk::ImageMemoryBarrier::default()
                                .image(image_info.image)
                                .old_layout(ash::vk::ImageLayout::UNDEFINED)
                                .new_layout(ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                                .src_access_mask(ash::vk::AccessFlags::empty())
                                .dst_access_mask(ash::vk::AccessFlags::TRANSFER_WRITE)
                                .subresource_range(image_info.full_subresource_range()),
                        );
                    }
                }
            }
        }
        if !image_barriers.is_empty() {
            unsafe {
                self.ctx.device.cmd_pipeline_barrier(
                    recorder.command_buffer(),
                    ash::vk::PipelineStageFlags::TOP_OF_PIPE,
                    ash::vk::PipelineStageFlags::TRANSFER,
                    ash::vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &image_barriers,
                )
            }
        }

        for (staging_buffer_index, copy_tasks) in copy_tasks.drain() {
            in_use_staging_buffers.insert(staging_buffer_index);
            staging_buffer_gpu_timeline.push(staging_buffer_index);
            let staging_buffer = staging_buffers.get(staging_buffer_index);

            let mut dst_buffer_copy_map: HashMap<ResourceId<Buffer>, Vec<ash::vk::BufferCopy>> =
                HashMap::new();
            let mut dst_image_copy_map: HashMap<ash::vk::Image, Vec<ash::vk::BufferImageCopy>> =
                HashMap::new();
            for task in &copy_tasks {
                match task {
                    VulkanStagingCopyTask::Buffer {
                        dst_buffer,
                        src_offset,
                        dst_offset,
                        copy_size,
                    } => {
                        let mut vec = dst_buffer_copy_map.entry(*dst_buffer).or_default();
                        vec.push(
                            ash::vk::BufferCopy::default()
                                .src_offset(*src_offset)
                                .dst_offset(*dst_offset)
                                .size(*copy_size),
                        );
                    }
                    VulkanStagingCopyTask::Image {
                        dst_image,
                        src_offset,
                        image_offset,
                        image_extent,
                    } => {
                        let dst_image_info = self.get_image(*dst_image);
                        let mut vec = dst_image_copy_map.entry(dst_image_info.image).or_default();
                        vec.push(
                            ash::vk::BufferImageCopy::default()
                                .image_offset(*image_offset)
                                .image_extent(*image_extent)
                                .image_subresource(dst_image_info.full_subresource_layer())
                                .buffer_offset(*src_offset)
                                // TODO: Set to zero for now so that the buffer data is interpreted
                                // as tightly packed.
                                .buffer_row_length(0)
                                .buffer_image_height(0),
                        )
                    }
                }
            }

            let src_buffer = self.get_buffer_info(&staging_buffer.buffer);
            for (dst_buffer_id, regions) in dst_buffer_copy_map.into_iter() {
                let dst_buffer = self.get_buffer_info(&dst_buffer_id);

                for region in &regions {
                    buffer_barriers.push(
                        ash::vk::BufferMemoryBarrier::default()
                            .buffer(dst_buffer.buffer)
                            .offset(region.dst_offset)
                            .size(region.size)
                            .src_access_mask(ash::vk::AccessFlags::TRANSFER_WRITE)
                            .dst_access_mask(ash::vk::AccessFlags::SHADER_READ),
                    );
                }

                unsafe {
                    self.ctx.device.cmd_copy_buffer(
                        recorder.command_buffer(),
                        src_buffer.buffer,
                        dst_buffer.buffer,
                        &regions,
                    )
                };
            }

            for (dst_image, regions) in dst_image_copy_map.into_iter() {
                unsafe {
                    self.ctx.device.cmd_copy_buffer_to_image(
                        recorder.command_buffer(),
                        src_buffer.buffer,
                        dst_image,
                        ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &regions,
                    )
                }
            }
        }

        unsafe {
            self.ctx.device.cmd_pipeline_barrier(
                recorder.command_buffer(),
                ash::vk::PipelineStageFlags::TRANSFER,
                ash::vk::PipelineStageFlags::ALL_COMMANDS,
                ash::vk::DependencyFlags::empty(),
                &[],
                &buffer_barriers,
                &[],
            )
        };
    }

    /// Images have dedicated memory allocations by default.
    fn create_image(
        &self,
        allocator: &mut VulkanAllocator,
        create_info: GfxImageCreateInfo,
    ) -> anyhow::Result<ResourceId<Image>> {
        debug!("creating vulkan owned image {}.", create_info.name);
        anyhow::ensure!(create_info.extent.x > 0 && create_info.extent.y > 0);
        let image_info = VulkanImageInfo {
            image_type: create_info.image_type,
            format: create_info.format.into(),
            extent: ash::vk::Extent2D {
                width: create_info.extent.x,
                height: create_info.extent.y,
            },
        };

        let mut type_specific_usages = match image_info.image_type {
            GfxImageType::DepthD2 => ash::vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            _ => ash::vk::ImageUsageFlags::COLOR_ATTACHMENT | ash::vk::ImageUsageFlags::STORAGE,
        };

        match create_info.format {
            // Srgb images do not support storage usages.
            GfxImageFormat::Rgba8Srgb => {
                type_specific_usages = type_specific_usages & !ash::vk::ImageUsageFlags::STORAGE
            }
            _ => {}
        }

        let create_info = ash::vk::ImageCreateInfo::default()
            .image_type(image_info.image_type.into())
            .format(image_info.format.into())
            .usage(
                ash::vk::ImageUsageFlags::SAMPLED
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
            image_memory_requirements.alignment as u32,
            VulkanAllocationType::GpuLocalDedicated,
            false,
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

    pub fn get_sampler(&self, sampler_id: &ResourceId<Sampler>) -> ash::vk::Sampler {
        self.samplers
            .read()
            .get(sampler_id)
            .expect(&format!(
                "Tried to get sampler {} but it does not exist.",
                sampler_id
            ))
            .clone()
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

    fn get_compute_pipeline(&self, id: ResourceId<ComputePipeline>) -> VulkanComputePipeline {
        self.compute_pipelines
            .read()
            .get(&id)
            .expect("Tried to fetch a vulkan compute pipeline doesn't exist.")
            .clone()
    }

    fn get_raster_pipeline(&self, id: ResourceId<RasterPipeline>) -> VulkanRasterPipeline {
        self.raster_pipelines
            .read()
            .get(&id)
            .expect("Tried to fetch a vulkan raster pipeline doesn't exist.")
            .clone()
    }

    fn create_sampler(&self, create_info: GfxSamplerCreateInfo) -> ResourceId<Sampler> {
        let resource_id = self.next_resource_id();
        let create_info: ash::vk::SamplerCreateInfo<'_> = create_info.into();
        let sampler = unsafe { self.ctx.device.create_sampler(&create_info, None) }
            .expect("TODO: Turn this into a result.");
        let samplers = self.samplers.write().insert(resource_id, sampler);
        resource_id
    }
}

impl Drop for VulkanResourceManager {
    // Don't worry about freeing memory allocations since the `VulkanAllocator` will be dropped
    // as well, destroying any gpu memory allocation.
    fn drop(&mut self) {
        unsafe {
            self.ctx.device.device_wait_idle();
            self.ctx
                .device
                .destroy_descriptor_pool(self.descriptor_pool, None)
        }
        println!("Dropping vkResourceManager");

        for event_pool in &self.event_pools {
            let event_pool = event_pool.write();
            for event in &event_pool.event_pool {
                unsafe { self.ctx.device.destroy_event(*event, None) };
            }
        }

        for (_, pipeline) in self.compute_pipelines.write().iter() {
            unsafe { self.ctx.device.destroy_pipeline(pipeline.pipeline, None) };
        }
        for (_, pipeline) in self.raster_pipelines.write().iter() {
            unsafe { self.ctx.device.destroy_pipeline(pipeline.pipeline, None) };
        }

        for pipeline_layout in self.pipeline_layouts.write().iter() {
            unsafe {
                self.ctx
                    .device
                    .destroy_pipeline_layout(pipeline_layout.layout, None)
            };
        }

        for (_, set) in self.descriptor_set_groups.write().iter() {
            unsafe {
                self.ctx
                    .device
                    .destroy_descriptor_set_layout(set.layout, None)
            };
        }

        for (_, borrowed_image) in self.borrowed_images.write().iter() {
            if let Some(image_view) = borrowed_image.view {
                unsafe { self.ctx.device.destroy_image_view(image_view, None) };
            }
        }

        for (_, sampler) in self.samplers.write().iter() {
            unsafe { self.ctx.device.destroy_sampler(*sampler, None) };
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
