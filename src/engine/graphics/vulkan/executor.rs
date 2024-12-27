use std::{any::Any, collections::HashMap, sync::Arc};

use log::{debug, warn};

use crate::engine::graphics::{
    backend::{GraphicsBackendFrameGraphExecutor, Image, ResourceId, Untyped},
    frame_graph::{
        self, FGResourceBackendId, FrameGraph, FrameGraphContext, FrameGraphContextImpl,
        FrameGraphResource, FrameGraphResourceImpl, IntoFrameGraphResource,
    },
};

use super::{
    device::{VulkanContext, VulkanContextHandle},
    recorder::VulkanRecorder,
};

pub struct VulkanFrameGraphExecutor {
    ctx: Arc<VulkanContext>,
    session: Option<FrameSession>,
    command_pools: Vec<ash::vk::CommandPool>,
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
        }
    }

    fn session_mut(&mut self) -> &mut FrameSession {
        self.session
            .as_mut()
            .expect("Tried to access frame session but no session exists currently.")
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

        let session = self.session_mut();
        let ctx = FrameGraphContext {
            frame_graph: &session.frame_graph,
            resource_map: &session.resource_map,
        };
        for pass in &session.frame_graph.passes {
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
    fn begin_frame(&mut self, frame_graph: FrameGraph) {
        let curr_cmd_pool = self.command_pools[self.ctx.curr_cpu_frame_index() as usize];
        unsafe {
            self.ctx.device().reset_command_pool(
                curr_cmd_pool,
                ash::vk::CommandPoolResetFlags::RELEASE_RESOURCES,
            )
        };
        self.session = Some(FrameSession::new(frame_graph));
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
                recorder.transition_image(
                    ResourceId::new(swapchain_image_id.resource_id.id()),
                    ash::vk::ImageLayout::PRESENT_SRC_KHR,
                    ash::vk::AccessFlags::empty(),
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
