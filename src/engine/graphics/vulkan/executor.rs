use std::{any::Any, collections::HashMap, ops::Deref, sync::Arc};

use log::{debug, warn};

use crate::engine::graphics::{
    backend::{
        Buffer, ComputePipeline, GfxComputePipelineCreateInfo, GfxComputePipelineInfo,
        GfxImageCreateInfo, GraphicsBackendFrameGraphExecutor, Image, ResourceId, Untyped,
    },
    frame_graph::{
        self, FGResourceBackendId, FrameGraph, FrameGraphContext, FrameGraphContextImpl,
        FrameGraphImageInfo, FrameGraphResource, FrameGraphResourceImpl, IntoFrameGraphResource,
    },
    shader::ShaderCompiler,
};

use super::{
    device::{VulkanContext, VulkanContextHandle},
    recorder::{VulkanImageTransition, VulkanRecorder},
};

pub struct VulkanFrameGraphExecutor {
    ctx: Arc<VulkanContext>,
    session: Option<FrameSession>,
    command_pools: Vec<ash::vk::CommandPool>,

    resource_manager: VulkanExecutorResourceManager,
}

struct VulkanExecutorResourceManager {
    ctx: Arc<VulkanContext>,
    cached_frame_images: HashMap<FrameGraphImageInfo, ResourceId<Image>>,
    cached_compute_pipelines: HashMap<GfxComputePipelineCreateInfo, ResourceId<ComputePipeline>>,
    // Resources that can be safely cached when the gpu finishes their frame index.
    // E.g: We use image 1 so on frame 0 so whenever we begin execution of a frame graph, we check
    // if we can move resources to the cache for reuse.
    cache_timeline: Vec<Vec<VulkanExecutorCachedResource>>,
}

enum VulkanExecutorCachedResource {
    Image {
        id: ResourceId<Image>,
        image_info: FrameGraphImageInfo,
    },
    Buffer {
        id: ResourceId<Buffer>,
    },
    ComputePipeline {
        id: ResourceId<ComputePipeline>,
        info: GfxComputePipelineCreateInfo,
    },
}

impl VulkanExecutorResourceManager {
    fn new(ctx: &Arc<VulkanContext>) -> Self {
        Self {
            ctx: ctx.clone(),
            cached_frame_images: HashMap::new(),
            cached_compute_pipelines: HashMap::new(),
            cache_timeline: (0..ctx.frames_in_flight())
                .map(|_| Vec::new())
                .collect::<Vec<_>>(),
        }
    }

    fn retire_resources(&mut self) {
        let curr_gpu_frame = self.ctx.curr_gpu_frame();
        let curr_cpu_frame = self.ctx.curr_cpu_frame();
        // This is called after we wait for our gpu timeline semaphore n - 2 so
        // we know this is our minimum.
        let minimum_gpu_frame = (curr_cpu_frame.saturating_sub(self.ctx.frames_in_flight() as u64));
        for i in minimum_gpu_frame..curr_cpu_frame {
            if curr_gpu_frame < i {
                continue;
            }

            for resource in self.cache_timeline[self.ctx.curr_cpu_frame_index() as usize].drain(..)
            {
                match resource {
                    VulkanExecutorCachedResource::Image { id, image_info } => {
                        self.cached_frame_images.insert(image_info, id);
                    }
                    VulkanExecutorCachedResource::Buffer { id } => todo!(),
                    VulkanExecutorCachedResource::ComputePipeline { id, info } => {
                        self.cached_compute_pipelines.insert(info, id);
                    }
                }
            }
        }
    }

    /// Gets an image which is only valid for the context of the
    /// current cpu frame, and the next gpu frame to be submitted.
    fn get_or_create_frame_image(
        &mut self,
        resource_name: &str,
        create_info: FrameGraphImageInfo,
    ) -> ResourceId<Image> {
        let (create_info, image_id) = self
            .cached_frame_images
            .remove_entry(&create_info)
            .take()
            .unwrap_or_else(|| {
                let gfx_create_info = GfxImageCreateInfo {
                    name: format!("frame_image_{}", resource_name),
                    image_type: create_info.image_type,
                    format: create_info.format,
                    extent: create_info.extent,
                };
                let new_image = self.ctx.create_image(gfx_create_info).expect(&format!(
                    "Failed to create frame image for resource {}.",
                    resource_name
                ));

                (create_info, new_image)
            });

        self.cache_timeline[self.ctx.curr_cpu_frame_index() as usize].push(
            VulkanExecutorCachedResource::Image {
                id: image_id,
                image_info: create_info,
            },
        );

        image_id
    }

    fn get_or_create_image(
        &mut self,
        resource_name: &str,
        create_info: FrameGraphImageInfo,
    ) -> ResourceId<Image> {
        let (create_info, image_id) = self
            .cached_frame_images
            .remove_entry(&create_info)
            .take()
            .unwrap_or_else(|| {
                let gfx_create_info = GfxImageCreateInfo {
                    name: format!("frame_image_{}", resource_name),
                    image_type: create_info.image_type,
                    format: create_info.format,
                    extent: create_info.extent,
                };
                let new_image = self.ctx.create_image(gfx_create_info).expect(&format!(
                    "Failed to create frame image for resource {}.",
                    resource_name
                ));

                (create_info, new_image)
            });

        self.cache_timeline[self.ctx.curr_cpu_frame_index() as usize].push(
            VulkanExecutorCachedResource::Image {
                id: image_id,
                image_info: create_info,
            },
        );

        image_id
    }

    fn get_or_create_compute_pipeline(
        &mut self,
        shader_compiler: &mut ShaderCompiler,
        create_info: &GfxComputePipelineCreateInfo,
    ) -> ResourceId<ComputePipeline> {
        let (create_info, compute_pipeline) = self
            .cached_compute_pipelines
            .remove_entry(&create_info)
            .take()
            .unwrap_or_else(|| {
                let compute_pipeline = self
                    .ctx
                    .create_compute_pipeline(shader_compiler, create_info.clone())
                    .expect("Failed to create graphics compute pipeline.");

                (create_info.clone(), compute_pipeline)
            });

        self.cache_timeline[self.ctx.curr_cpu_frame_index() as usize].push(
            VulkanExecutorCachedResource::ComputePipeline {
                id: compute_pipeline,
                info: create_info,
            },
        );

        compute_pipeline
    }
}

struct FrameSession {
    frame_graph: FrameGraph,
    resource_map: HashMap<FrameGraphResource<Untyped>, FGResourceBackendId>,
    recorded_command_buffers: Vec<VulkanRecorder>,
}

impl FrameSession {
    fn new(frame_graph: FrameGraph) -> Self {
        Self {
            frame_graph,
            resource_map: HashMap::new(),
            recorded_command_buffers: Vec::new(),
        }
    }
}

impl VulkanFrameGraphExecutor {
    pub fn new(ctx: &VulkanContextHandle) -> Self {
        Self {
            ctx: ctx.clone(),
            session: None,
            command_pools: (0..ctx.frames_in_flight())
                .map(|_| {
                    unsafe {
                        ctx.device()
                            .create_command_pool(&ash::vk::CommandPoolCreateInfo::default(), None)
                    }
                    .expect("Failed to create vk command pool.")
                })
                .collect::<Vec<_>>(),

            resource_manager: VulkanExecutorResourceManager::new(ctx),
        }
    }

    fn session_mut(&mut self) -> &mut FrameSession {
        self.session
            .as_mut()
            .expect("Tried to access frame session but no session exists currently.")
    }

    fn initialize_session_pipelines(&mut self, shader_compiler: &mut ShaderCompiler) {
        let session = self.session.as_mut().unwrap();
        for (frame_resource, compute_pipeline_info) in &session.frame_graph.compute_pipelines {
            let compute_pipeline = self
                .ctx
                .create_compute_pipeline(shader_compiler, compute_pipeline_info.clone())
                .expect("Failed to create graphics compute pipeline.");
            session.resource_map.insert(
                frame_resource.as_untyped(),
                FGResourceBackendId {
                    resource_id: compute_pipeline.as_untyped(),
                    expected_type: std::any::TypeId::of::<ComputePipeline>(),
                },
            );
        }
    }

    fn acquire_command_buffers(
        &mut self,
        count: u32,
    ) -> anyhow::Result<Vec<ash::vk::CommandBuffer>> {
        let allocate_info = ash::vk::CommandBufferAllocateInfo::default()
            .command_pool(self.command_pools[self.ctx.curr_cpu_frame_index() as usize])
            .level(ash::vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(count);
        Ok(unsafe { self.ctx.device().allocate_command_buffers(&allocate_info) }?)
    }

    fn flush(&mut self) {
        let command_buffer = self
            .acquire_command_buffers(1)
            .expect("Failed to acquire command buffers.")
            .into_iter()
            .next()
            .unwrap();
        let mut recorder = VulkanRecorder::new(&self.ctx, command_buffer);
        recorder.begin();

        let mut session = self.session.as_mut().unwrap();
        for pass in &session.frame_graph.passes {
            for input in &pass.inputs {
                if session.resource_map.contains_key(input) {
                    // Resource has already been populated.
                    continue;
                }

                let input_info = &session.frame_graph.resource_infos[input.id() as usize];
                if input_info.type_id == std::any::TypeId::of::<Image>()
                    && session
                        .frame_graph
                        .frame_image_infos
                        .contains_key(&input.as_typed())
                {
                    let frame_image_info = session
                        .frame_graph
                        .frame_image_infos
                        .get(&input.as_typed())
                        .unwrap()
                        .clone();
                    let frame_image = self
                        .resource_manager
                        .get_or_create_frame_image(&input_info.name, frame_image_info);

                    session.resource_map.insert(
                        *input,
                        FGResourceBackendId {
                            resource_id: frame_image.as_untyped(),
                            expected_type: std::any::TypeId::of::<Image>(),
                        },
                    );
                }
            }

            let ctx = FrameGraphContext {
                frame_graph: &session.frame_graph,
                resource_map: &session.resource_map,
            };
            (&pass.pass)(&mut recorder, &ctx);
        }
        session.recorded_command_buffers.push(recorder);
    }
}

impl Drop for VulkanFrameGraphExecutor {
    fn drop(&mut self) {
        // TODO: Resource tracking so we dont block like this.
        unsafe { self.ctx.device().device_wait_idle() };
        for command_pool in &self.command_pools {
            unsafe { self.ctx.device().destroy_command_pool(*command_pool, None) }
        }
    }
}

impl GraphicsBackendFrameGraphExecutor for VulkanFrameGraphExecutor {
    fn begin_frame(&mut self, shader_comiler: &mut ShaderCompiler, frame_graph: FrameGraph) {
        let curr_cmd_pool = self.command_pools[self.ctx.curr_cpu_frame_index() as usize];
        unsafe {
            self.ctx.device().reset_command_pool(
                curr_cmd_pool,
                ash::vk::CommandPoolResetFlags::RELEASE_RESOURCES,
            )
        };

        self.session = Some(FrameSession::new(frame_graph));
        self.resource_manager.retire_resources();
        self.initialize_session_pipelines(shader_comiler);
    }

    fn end_frame(&mut self) -> FrameGraph {
        self.flush();
        let mut session = self.session.take().unwrap();
        if session.recorded_command_buffers.is_empty() {
            warn!("Frame graph was executed but didn't record any command buffers.");
            return session.frame_graph;
        }

        let swapchain_image_id = session
            .resource_map
            .get(&session.frame_graph.swapchain_image.as_untyped())
            .unwrap();
        let recorder_count = session.recorded_command_buffers.len();
        for (i, recorder) in session.recorded_command_buffers.iter_mut().enumerate() {
            if i == recorder_count - 1 {
                recorder.transition_images(
                    &[VulkanImageTransition {
                        image_id: ResourceId::new(swapchain_image_id.resource_id.id()),
                        new_layout: ash::vk::ImageLayout::PRESENT_SRC_KHR,
                        new_access_flags: ash::vk::AccessFlags::empty(),
                    }],
                    ash::vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                );
            }
            recorder.finish();
        }

        let command_buffer_infos = session
            .recorded_command_buffers
            .iter()
            .map(|recorder| {
                ash::vk::CommandBufferSubmitInfo::default()
                    .command_buffer(recorder.command_buffer())
            })
            .collect::<Vec<_>>();
        let wait_semaphore_infos = [ash::vk::SemaphoreSubmitInfo::default()
            .semaphore(self.ctx.curr_image_acquire_semaphore())
            .stage_mask(ash::vk::PipelineStageFlags2::TOP_OF_PIPE)];
        let signal_semaphore_infos = [
            ash::vk::SemaphoreSubmitInfo::default()
                .semaphore(self.ctx.gpu_timeline_semaphore())
                .value(self.ctx.curr_cpu_frame())
                .stage_mask(ash::vk::PipelineStageFlags2::BOTTOM_OF_PIPE),
            ash::vk::SemaphoreSubmitInfo::default()
                .semaphore(self.ctx.curr_image_ready_semaphore())
                .stage_mask(ash::vk::PipelineStageFlags2::BOTTOM_OF_PIPE),
        ];
        let mut submit_info_2 = ash::vk::SubmitInfo2::default()
            .command_buffer_infos(&command_buffer_infos)
            .wait_semaphore_infos(&wait_semaphore_infos)
            .signal_semaphore_infos(&signal_semaphore_infos);
        unsafe {
            self.ctx.device().queue_submit2(
                self.ctx.main_queue(),
                &[submit_info_2],
                ash::vk::Fence::null(),
            )
        };

        let swapchains = [self.ctx.swapchain().swapchain];
        let image_indices = [self.ctx.curr_swapchain_image_index()];
        let wait_semaphores = [self.ctx.curr_image_ready_semaphore()];
        let present_info = ash::vk::PresentInfoKHR::default()
            .swapchains(&swapchains)
            .image_indices(&image_indices)
            .wait_semaphores(&wait_semaphores);
        unsafe {
            self.ctx
                .swapchain_loader()
                .queue_present(self.ctx.main_queue(), &present_info)
        };

        session.frame_graph
    }

    fn supply_image_ref(&mut self, name: &str, image: &ResourceId<Image>) {
        let session = self.session_mut();
        let resource = session
            .frame_graph
            .resource_name_map
            .get(name)
            .expect(&format!(
                "The resource `{}` doesn't exist in the executing frame graph",
                name
            ));
        assert_eq!(resource.type_id, std::any::TypeId::of::<Image>());

        let prev = session.resource_map.insert(
            FrameGraphResource::new(resource.id),
            FGResourceBackendId {
                resource_id: image.as_untyped(),
                expected_type: std::any::TypeId::of::<Image>(),
            },
        );
        if prev.is_some() {
            panic!("Can't supply a frame graph input twice in one frame.");
        }
    }
}
