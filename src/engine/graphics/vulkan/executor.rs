use core::panic;
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
    time::Duration,
};

use ash::vk::CommandBuffer;
use log::{debug, warn};

use crate::engine::{
    graphics::{
        backend::{
            Buffer, ComputePipeline, GfxBufferCreateInfo, GfxComputePipelineCreateInfo,
            GfxComputePipelineInfo, GfxCullMode, GfxFrontFace, GfxImageCreateInfo, GfxImageFormat,
            GfxPassOnceImpl, GfxRasterPipelineBlendStateCreateInfo, GfxRasterPipelineCreateInfo,
            GfxVertexFormat, GraphicsBackendFrameGraphExecutor, Image, RasterPipeline, ResourceId,
            ShaderWriter, Untyped,
        },
        frame_graph::{
            self, FGResourceBackendId, FrameGraph, FrameGraphBufferInfo,
            FrameGraphComputePipelineInfo, FrameGraphContext, FrameGraphContextImpl,
            FrameGraphImageInfo, FrameGraphPass, FrameGraphRasterPipelineInfo, FrameGraphResource,
            IntoFrameGraphResource, Pass,
        },
        shader::{
            Shader, ShaderCompilationOptions, ShaderCompilationTarget, ShaderCompiler, ShaderDesc,
            ShaderModificationTree, ShaderSetBinding, ShaderStage,
        },
    },
    window::time::{Instant, Timer},
};

use super::{
    device::{VulkanContext, VulkanContextHandle},
    recorder::{VulkanImageTransition, VulkanRecorder},
};

pub struct VulkanFrameGraphExecutor {
    ctx: Arc<VulkanContext>,
    session: Option<FrameSession>,
    command_pools: Vec<VulkanCommandPool>,

    resource_manager: VulkanExecutorResourceManager,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct VulkanExecutorComputePipelineDesc {
    shader_desc: ShaderDesc,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct VulkanExecutorRasterPipelineDesc {
    vertex_shader_desc: ShaderDesc,
    fragment_shader_desc: ShaderDesc,

    vertex_format: GfxVertexFormat,
    cull_mode: GfxCullMode,
    front_face: GfxFrontFace,
    blend_state: GfxRasterPipelineBlendStateCreateInfo,
    color_formats: Vec<GfxImageFormat>,
    depth_format: Option<GfxImageFormat>,
}

struct VulkanExecutorResourceManager {
    ctx: Arc<VulkanContext>,
    // Usable images to use in a frame, since they can't be directly mapped on the cpu we don't
    // have to worry about frames in flight since images are only used during a gpu frame.
    frame_images: HashMap<FrameGraphImageInfo, Vec<ResourceId<Image>>>,
    // Usable buffers to use in a frame, since they can't be directly mapped on the cpu since we
    // upload data through staging buffers, we dont have to worry about synchronization and frames
    // in flight since these buffers are only references during a gpu frame.
    frame_buffers: Vec<(FrameGraphBufferInfo, ResourceId<Buffer>)>,
    compute_pipelines: HashMap<VulkanExecutorComputePipelineDesc, ResourceId<ComputePipeline>>,
    raster_pipelines: HashMap<VulkanExecutorRasterPipelineDesc, ResourceId<RasterPipeline>>,
    // Holds the in use buffers and images during the current execution context to prevent using
    // the same image or buffer accidentally since we don't have proper aliasing yet.
    in_use_resources: HashSet<ResourceId<Untyped>>,

    invalidate_shader_timer: Timer,
    shader_modification_tree: ShaderModificationTree,
    shader_compiler: ShaderCompiler,
    cached_library_bindings: Option<(Vec<ShaderSetBinding>, bool)>,
    // new_compute_pipeline: HashMap<VulkanExecutorComputePipelineDesc, ResourceId<ComputePipeline>>,
    // // The timeline in which the new compute pipeline is old news, meaning all the frames in flight
    // // have updated to the new pipeline.
    // new_compute_pipeline_deletion_timeline: Vec<HashSet<GfxComputePipelineCreateInfo>>,

    // new_raster_pipeline: HashMap<GfxRasterPipelineCreateInfo, ResourceId<RasterPipeline>>,
    // // The timeline in which the new raster pipeline is old news, meaning all the frames in flight
    // // have updated to the new pipeline.
    // new_raster_pipeline_deletion_timeline: Vec<HashSet<GfxRasterPipelineCreateInfo>>,
    // Resources that can be safely cached when the gpu finishes their frame index.
    // E.g: We use image 1 so on frame 0 so whenever we begin execution of a frame graph, we check
    // if we can move resources to the cache for reuse.
    // cache_timeline: Vec<Vec<VulkanExecutorCachedResource>>,
}

// #[derive(PartialEq, Eq, Hash)]
// enum VulkanExecutorCachedResource {
//     Image {
//         id: ResourceId<Image>,
//         image_info: FrameGraphImageInfo,
//     },
//     Buffer {
//         id: ResourceId<Buffer>,
//         buffer_info: FrameGraphBufferInfo,
//     },
//     ComputePipeline {
//         id: ResourceId<ComputePipeline>,
//         info: GfxComputePipelineCreateInfo,
//     },
//     RasterPipeline {
//         id: ResourceId<RasterPipeline>,
//         info: GfxRasterPipelineCreateInfo,
//     },
// }

impl VulkanExecutorResourceManager {
    fn new(ctx: &Arc<VulkanContext>) -> Self {
        Self {
            ctx: ctx.clone(),
            //            cached_frame_images: HashMap::new(),
            //            cached_frame_buffers: Vec::new(),
            //            cached_compute_pipelines: HashMap::new(),
            //            cached_raster_pipelines: HashMap::new(),
            frame_images: HashMap::new(),
            frame_buffers: Vec::new(),
            compute_pipelines: HashMap::new(),
            raster_pipelines: HashMap::new(),
            in_use_resources: HashSet::new(),

            shader_compiler: ShaderCompiler::new(),
            invalidate_shader_timer: Timer::new(Duration::from_millis(250)),
            shader_modification_tree: ShaderModificationTree::from_current_state(),
            cached_library_bindings: None,
            //             new_compute_pipeline: HashMap::new(),
            //             new_compute_pipeline_deletion_timeline: (0..ctx.frames_in_flight())
            //                 .map(|_| HashSet::new())
            //                 .collect::<Vec<_>>(),
            //             new_raster_pipeline: HashMap::new(),
            //             new_raster_pipeline_deletion_timeline: (0..ctx.frames_in_flight())
            //                 .map(|_| HashSet::new())
            //                 .collect::<Vec<_>>(),
            //             cache_timeline: (0..ctx.frames_in_flight())
            //                 .map(|_| Vec::new())
            //                 .collect::<Vec<_>>(),
        }
    }

    fn try_invalidate_shaders(&mut self) {
        if !self.invalidate_shader_timer.try_complete() {
            return;
        }

        let curr_shader_state = ShaderModificationTree::from_current_state();
        if curr_shader_state == self.shader_modification_tree {
            return;
        }

        // Invalidate available pipelines.
        debug!("Invalidating pipelines due to shader change.");
        self.shader_compiler.invalidate_cache();
        self.shader_modification_tree = curr_shader_state;
        if let Some((_, lib_binding_invalid)) = &mut self.cached_library_bindings {
            *lib_binding_invalid = true;
        }

        // let mut recreation_compute_pipelines = Vec::new();
        // for (info, id) in self.cached_compute_pipelines.iter() {
        //     recreation_compute_pipelines.push((*id, info.clone()));
        // }
        // for timeline in self.cache_timeline.iter() {
        //     for resource in timeline {
        //         match resource {
        //             VulkanExecutorCachedResource::ComputePipeline { id, info } => {
        //                 recreation_compute_pipelines.push((*id, info.clone()))
        //             }
        //             _ => {}
        //         }
        //     }
        // }

        // Compute pipeline updates.
        let mut new_pipelines = Vec::new();
        for (desc, compute_id) in &self.compute_pipelines {
            let opts = Self::create_shader_compilation_options(
                desc.shader_desc.clone(),
                ShaderStage::Compute,
            );
            match self.shader_compiler.compile_shader(opts) {
                Ok(shader) => {
                    let new_pipeline = self
                        .ctx
                        .create_compute_pipeline(GfxComputePipelineCreateInfo { shader })
                        .expect("Failed to create new compute pipeline.");

                    new_pipelines.push((desc.clone(), new_pipeline));
                    // TODO: Destroy pipeline.
                }
                Err(err) => {
                    log::error!("Got error compiling new slang compute shader:");
                    log::error!("{}", err)
                }
            }
        }

        for (desc, new_pipeline) in new_pipelines {
            let old_pipeline = self.compute_pipelines.get_mut(&desc).unwrap();
            *old_pipeline = new_pipeline;
        }

        // Raster pipeline updates.
        let mut new_pipelines = Vec::new();
        for (desc, raster_id) in &self.raster_pipelines {
            let vertex_opts = Self::create_shader_compilation_options(
                desc.vertex_shader_desc.clone(),
                ShaderStage::Vertex,
            );
            let fragment_opts = Self::create_shader_compilation_options(
                desc.fragment_shader_desc.clone(),
                ShaderStage::Fragment,
            );
            let (vertex_shader, fragment_shader) = match self
                .shader_compiler
                .compile_shader_pair(vertex_opts, fragment_opts)
            {
                Ok(shaders) => shaders,
                Err(err) => {
                    log::error!("Got error compiling new slang shaders:");
                    log::error!("{}", err);
                    continue;
                }
            };

            let new_pipeline = self
                .ctx
                .create_raster_pipeline(GfxRasterPipelineCreateInfo {
                    vertex_shader,
                    fragment_shader,
                    cull_mode: desc.cull_mode,
                    front_face: desc.front_face,
                    vertex_format: desc.vertex_format.clone(),
                    blend_state: desc.blend_state.clone(),
                    color_formats: desc.color_formats.clone(),
                    depth_format: desc.depth_format,
                })
                .expect("Failed to create graphics raster pipeline.");

            new_pipelines.push((desc.clone(), new_pipeline));
            // TODO: Destroy old raster pipeline.
        }

        for (desc, new_pipeline) in new_pipelines {
            let old_pipeline = self.raster_pipelines.get_mut(&desc).unwrap();
            *old_pipeline = new_pipeline;
        }
    }

    fn retire_resources(&mut self) {
        self.in_use_resources.clear();
        // for resource in self.cache_timeline[self.ctx.curr_cpu_frame_index() as usize].drain(..) {
        //     match resource {
        //         VulkanExecutorCachedResource::Image { id, image_info } => {
        //             self.cached_frame_images.insert(image_info, id);
        //         }
        //         VulkanExecutorCachedResource::Buffer { id, buffer_info } => {
        //             self.cached_frame_buffers.push((buffer_info, id));
        //         }
        //         VulkanExecutorCachedResource::ComputePipeline { id, info } => {
        //             if let Some(new_pipeline) = self.new_compute_pipeline.get(&info) {
        //                 self.cached_compute_pipelines.insert(info, *new_pipeline);
        //                 self.ctx.destroy_compute_pipeline(id);
        //             } else {
        //                 self.cached_compute_pipelines.insert(info, id);
        //             }
        //         }
        //         VulkanExecutorCachedResource::RasterPipeline { id, info } => {
        //             todo!("Retire cached raster pipeline.")
        //         }
        //     }
        // }

        // // If this was the frame index the pipeline invalidation was submitted, then that means
        // // since we retired that frame, we have updated all the frames in flight and can no longer
        // // qualify this as a "new" pipeline.
        // for info in self.new_compute_pipeline_deletion_timeline
        //     [self.ctx.curr_cpu_frame_index() as usize]
        //     .drain()
        // {
        //     self.new_compute_pipeline.remove(&info);
        // }
    }

    /// Gets an image which is only valid for the context of the
    /// current cpu frame, and the next gpu frame to be submitted.
    fn get_or_create_frame_image(
        &mut self,
        resource_name: &str,
        create_info: FrameGraphImageInfo,
    ) -> ResourceId<Image> {
        let frame_images = self
            .frame_images
            .entry(create_info.clone())
            .or_insert(Vec::new());

        for image in frame_images.iter() {
            let untyped_id = image.as_untyped();
            if !self.in_use_resources.contains(&untyped_id) {
                self.in_use_resources.insert(untyped_id);
                return *image;
            }
        }

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
        frame_images.push(new_image);
        self.in_use_resources.insert(new_image.as_untyped());

        new_image
    }

    /// Gets an buffer which is only valid for the context of the
    /// current cpu frame, and the next gpu frame to be submitted.
    fn get_or_create_frame_buffer(
        &mut self,
        resource_name: &str,
        create_info: FrameGraphBufferInfo,
    ) -> ResourceId<Buffer> {
        let buffer_id = self
            .frame_buffers
            .iter()
            .find_map(|(frame_buffer_info, frame_buffer)| {
                if !self.in_use_resources.contains(&frame_buffer.as_untyped())
                    && frame_buffer_info.size >= create_info.size
                    && frame_buffer_info.size <= create_info.size * 2
                {
                    return Some(*frame_buffer);
                }

                None
            });

        let buffer_id = buffer_id.unwrap_or_else(|| {
            let gfx_create_info = GfxBufferCreateInfo {
                name: format!("frame_buffer_{}", resource_name),
                size: create_info.size,
            };
            let new_buffer = self.ctx.create_buffer(gfx_create_info).expect(&format!(
                "Failed to create frame game for resource {}.",
                resource_name
            ));
            self.frame_buffers.push((create_info, new_buffer));

            new_buffer
        });
        self.in_use_resources.insert(buffer_id.as_untyped());

        buffer_id
    }

    fn get_or_create_compute_pipeline(
        &mut self,
        create_info: &FrameGraphComputePipelineInfo,
    ) -> ResourceId<ComputePipeline> {
        let compute_desc = VulkanExecutorComputePipelineDesc {
            shader_desc: ShaderDesc {
                module: create_info.shader_path.clone(),
                entry_point_name: create_info.entry_point_fn.clone(),
            },
        };

        if !self.compute_pipelines.contains_key(&compute_desc) {
            let opts = Self::create_shader_compilation_options(
                compute_desc.shader_desc.clone(),
                ShaderStage::Compute,
            );
            let shader = self
                .shader_compiler
                .compile_shader(opts)
                .expect("Failed to compile pipeline shader.");

            let compute_pipeline = self
                .ctx
                .create_compute_pipeline(GfxComputePipelineCreateInfo { shader })
                .expect("Failed to create graphics compute pipeline.");

            self.compute_pipelines
                .insert(compute_desc.clone(), compute_pipeline);

            return compute_pipeline;
        }

        *self.compute_pipelines.get(&compute_desc).unwrap()
    }

    fn get_or_create_raster_pipeline(
        &mut self,
        create_info: &FrameGraphRasterPipelineInfo,
        color_attachment_ids: Vec<ResourceId<Image>>,
        depth_attachment_id: Option<ResourceId<Image>>,
    ) -> ResourceId<RasterPipeline> {
        let color_formats = color_attachment_ids
            .into_iter()
            .map(|attachment| self.ctx.get_image_info(&attachment).format)
            .collect::<Vec<_>>();
        let depth_format =
            depth_attachment_id.map(|attachment| self.ctx.get_image_info(&attachment).format);

        let raster_desc = VulkanExecutorRasterPipelineDesc {
            vertex_shader_desc: ShaderDesc {
                module: create_info.vertex_shader_path.clone(),
                entry_point_name: create_info.vertex_entry_point_fn.clone(),
            },
            fragment_shader_desc: ShaderDesc {
                module: create_info.fragment_shader_path.clone(),
                entry_point_name: create_info.fragment_entry_point_fn.clone(),
            },
            vertex_format: create_info.vertex_format.clone(),
            cull_mode: create_info.cull_mode,
            front_face: create_info.front_face,
            blend_state: create_info.blend_state.clone(),
            color_formats,
            depth_format,
        };

        if !self.raster_pipelines.contains_key(&raster_desc) {
            let vertex_opts = Self::create_shader_compilation_options(
                raster_desc.vertex_shader_desc.clone(),
                ShaderStage::Vertex,
            );
            let fragment_opts = Self::create_shader_compilation_options(
                raster_desc.fragment_shader_desc.clone(),
                ShaderStage::Fragment,
            );
            let (vertex_shader, fragment_shader) = self
                .shader_compiler
                .compile_shader_pair(vertex_opts, fragment_opts)
                .expect("Failed to compile raster pipeline shaders.");

            let raster_pipeline = self
                .ctx
                .create_raster_pipeline(GfxRasterPipelineCreateInfo {
                    vertex_shader,
                    fragment_shader,
                    cull_mode: raster_desc.cull_mode,
                    front_face: raster_desc.front_face,
                    vertex_format: raster_desc.vertex_format.clone(),
                    blend_state: raster_desc.blend_state.clone(),
                    color_formats: raster_desc.color_formats.clone(),
                    depth_format: raster_desc.depth_format,
                })
                .expect("Failed to create graphics raster pipeline.");

            self.raster_pipelines
                .insert(raster_desc.clone(), raster_pipeline);

            return raster_pipeline;
        }

        *self.raster_pipelines.get(&raster_desc).unwrap()
    }

    fn create_shader_compilation_options(
        desc: ShaderDesc,
        stage: ShaderStage,
    ) -> ShaderCompilationOptions {
        ShaderCompilationOptions {
            module: desc.module.module(),
            entry_point: desc.entry_point_name.clone(),
            stage,
            target: ShaderCompilationTarget::SpirV,
            macro_defines: HashMap::new(),
        }
    }
}

struct FrameSession {
    frame_graph: FrameGraph,
    resource_map: HashMap<FrameGraphResource<Untyped>, FGResourceBackendId>,
    recorded_command_buffers: Vec<VulkanRecorder>,
    recorded_pass_refs: HashMap<FrameGraphResource<Pass>, VulkanRecorder>,
    /// Same length as `frame_graph.passes.len() - 1`.
    pass_set_events: Vec<Option<ash::vk::Event>>,

    supplied_inputs: HashMap<FrameGraphResource<Untyped>, Box<dyn Any>>,

    buffer_writes_event: Option<ash::vk::Event>,
}

impl FrameSession {
    fn new(frame_graph: FrameGraph) -> Self {
        let pass_len = frame_graph.passes.len();
        Self {
            frame_graph,
            resource_map: HashMap::new(),
            recorded_command_buffers: Vec::new(),
            recorded_pass_refs: HashMap::new(),
            pass_set_events: vec![None; pass_len.saturating_sub(1)],
            supplied_inputs: HashMap::new(),
            buffer_writes_event: None,
        }
    }

    fn create_context(&self) -> FrameGraphContext {
        FrameGraphContext {
            frame_graph: &self.frame_graph,
            resource_map: &self.resource_map,
            supplied_inputs: &self.supplied_inputs,
        }
    }
}

pub struct VulkanCommandPool {
    command_pool: ash::vk::CommandPool,
    in_use_command_buffers: Vec<ash::vk::CommandBuffer>,
    free_command_buffers: Vec<ash::vk::CommandBuffer>,
}

impl VulkanFrameGraphExecutor {
    pub fn new(ctx: &VulkanContextHandle) -> Self {
        Self {
            ctx: ctx.clone(),
            session: None,
            command_pools: (0..ctx.frames_in_flight())
                .map(|_| {
                    let vk_command_pool = unsafe {
                        ctx.device()
                            .create_command_pool(&ash::vk::CommandPoolCreateInfo::default(), None)
                    }
                    .expect("Failed to create vk command pool.");

                    VulkanCommandPool {
                        command_pool: vk_command_pool,
                        in_use_command_buffers: Vec::new(),
                        free_command_buffers: Vec::new(),
                    }
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

    fn initialize_session_resources(&mut self) {
        let session = self.session.as_mut().unwrap();

        // Re-reflect library module set bindings.
        if let Some((old_bindings, invalid)) = &mut self.resource_manager.cached_library_bindings {
            if *invalid {
                match self.resource_manager.shader_compiler.get_library_bindings() {
                    Ok(new_bindings) => *old_bindings = new_bindings,
                    Err(err) => {
                        log::error!("Got error compiling slang library module:");
                        log::error!("{}", err)
                    }
                }
                *invalid = false;
            }
        } else {
            self.resource_manager.cached_library_bindings = Some((
                self.resource_manager
                    .shader_compiler
                    .get_library_bindings()
                    .expect("Failed to reflect library bindings."),
                false,
            ));
        }

        // Initialize compute pipelines since they don't rely on any graph dependencies.
        for (frame_resource, compute_pipeline_info) in &session.frame_graph.compute_pipelines {
            let compute_pipeline = self
                .resource_manager
                .get_or_create_compute_pipeline(compute_pipeline_info);
            session.resource_map.insert(
                frame_resource.as_untyped(),
                FGResourceBackendId {
                    resource_id: compute_pipeline.as_untyped(),
                    expected_type: std::any::TypeId::of::<ComputePipeline>(),
                },
            );
        }

        session.buffer_writes_event = Some(self.ctx.create_frame_event());
    }

    fn acquire_command_buffer(
        ctx: &VulkanContextHandle,
        command_pools: &mut Vec<VulkanCommandPool>,
    ) -> anyhow::Result<ash::vk::CommandBuffer> {
        let command_pool = &mut command_pools[ctx.curr_cpu_frame_index() as usize];

        if let Some(command_buffer) = command_pool.free_command_buffers.pop() {
            command_pool.in_use_command_buffers.push(command_buffer);
            return Ok(command_buffer);
        }

        let allocate_info = ash::vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool.command_pool)
            .level(ash::vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer =
            unsafe { ctx.device().allocate_command_buffers(&allocate_info) }?.remove(0);
        command_pool.in_use_command_buffers.push(command_buffer);

        Ok(command_buffer)
    }

    fn prep_pass_inputs(
        frame_graph: &FrameGraph,
        resource_map: &mut HashMap<FrameGraphResource<Untyped>, FGResourceBackendId>,
        supplied_inputs: &HashMap<FrameGraphResource<Untyped>, Box<dyn std::any::Any>>,
        resource_manager: &mut VulkanExecutorResourceManager,
        pass: &FrameGraphPass,
    ) {
        // Iterate over outputs and inputs since we outputs implicity define inputs.
        for input in pass.inputs.iter().chain(pass.outputs.iter()) {
            if resource_map.contains_key(input) {
                // Resource has already been populated.
                continue;
            }

            let input_info = &frame_graph.resource_infos[input.id() as usize];
            if input_info.type_id == std::any::TypeId::of::<Image>() {
                let image_info = if let Some(frame_image_info) =
                    frame_graph.frame_image_infos.get(&input.as_typed())
                {
                    Some(frame_image_info.clone())
                } else if let Some(info_create_fn) =
                    frame_graph.frame_image_infos_delayed.get(&input.as_typed())
                {
                    let ctx = FrameGraphContext {
                        frame_graph,
                        resource_map,
                        supplied_inputs,
                    };
                    let image_info = info_create_fn(&ctx);
                    Some(image_info)
                } else {
                    None
                };

                if let Some(image_info) = image_info {
                    let frame_image =
                        resource_manager.get_or_create_frame_image(&input_info.name, image_info);

                    resource_map.insert(
                        *input,
                        FGResourceBackendId {
                            resource_id: frame_image.as_untyped(),
                            expected_type: std::any::TypeId::of::<Image>(),
                        },
                    );
                    continue;
                }

                if frame_graph.inputs.contains_key(&input) {
                    panic!(
                        "User defined image input `{}` hasn't been populated in the executor yet, and it is required for pass `{}`.", input_info.name, frame_graph.resource_infos[pass.id.id() as usize].name
                    );
                } else {
                    panic!(
                        "Pass has an image input that is somehow not contained in the executor's session frame graph definition."
                    );
                }
            }

            if input_info.type_id == std::any::TypeId::of::<Buffer>() {
                if frame_graph
                    .frame_buffers
                    .contains(&input.as_typed::<Buffer>())
                {
                    log::warn!(
                        "Frame graph frame buffer `{}` hasn't been written to yet, and it is required for pass `{}`.", input_info.name, frame_graph.resource_infos[pass.id.id() as usize].name
                    );
                    continue;
                } else if frame_graph.inputs.contains_key(&input) {
                    panic!(
                        "User defined buffer input `{}` hasn't been populated in the executor yet, and it is required for pass `{}`.", input_info.name, frame_graph.resource_infos[pass.id.id() as usize].name
                    );
                } else {
                    panic!(
                        "Pass has a buffer input that is somehow not contained in the executor's session frame graph definition."
                    );
                }
            }

            if input_info.type_id == std::any::TypeId::of::<ComputePipeline>() {
                if let Some(info) = frame_graph
                    .compute_pipelines
                    .get(&input.as_typed::<ComputePipeline>())
                {
                    let compute_pipeline = resource_manager.get_or_create_compute_pipeline(info);
                    resource_map.insert(
                        input.as_untyped(),
                        FGResourceBackendId {
                            resource_id: compute_pipeline.as_untyped(),
                            expected_type: std::any::TypeId::of::<ComputePipeline>(),
                        },
                    );

                    continue;
                } else {
                    panic!(
                        "Pass has a compute
                        pipeline input that is somehow not contained in the executor's session frame graph definition."
)
                }
            }

            // Process raster pipeline later since we need to process all image inputs first.
            if input_info.type_id == std::any::TypeId::of::<RasterPipeline>() {
                continue;
            }

            panic!(
                "Unknown frame graph input type for input name `{}` and type {:?}.",
                input_info.name, input_info.type_id
            );
        }

        // Iterate raster pipeline inputs since we need to ensure all other image inputs have been
        // made, since the raster pipeline input attachments implicity define those images as an
        // input to this pass.
        for input in pass.inputs.iter() {
            let input_info = &frame_graph.resource_infos[input.id() as usize];
            if input_info.type_id == std::any::TypeId::of::<RasterPipeline>() {
                let Some(info) = frame_graph
                    .raster_pipelines
                    .get(&input.as_typed::<RasterPipeline>())
                else {
                    panic!(
                        "Pass has a raster 
                        pipeline input that is somehow not contained in the executor's session frame graph definition."
);
                };
                let color_attachment_ids = info
                    .color_attachments
                    .iter()
                    .map(|attachment| {
                        let Some(image_id) = resource_map.get(&attachment.as_untyped()) else {
                            panic!("Somehow we have a raster pipeline color attachment that wasn't initialized as an implicit input prior.");
                        };
                        assert_eq!(image_id.expected_type, std::any::TypeId::of::<Image>());

                        image_id.resource_id.as_typed::<Image>()
                    })
                    .collect::<Vec<_>>();
                let depth_attachment_id = info.depth_attachment.map(|attachment| {

                        let Some(image_id) = resource_map.get(&attachment.as_untyped()) else {
                            panic!("Somehow we have a raster pipeline depth attachment that wasn't initialized as an implicit input prior.");
                        };
                        assert_eq!(image_id.expected_type, std::any::TypeId::of::<Image>());

                        image_id.resource_id.as_typed::<Image>()
                });

                // TODO: make color attachments an input to this pass and then process the
                // raster pipeline last so we know we have all the required images loaded in
                // the resource map.
                let raster_pipeline = resource_manager.get_or_create_raster_pipeline(
                    info,
                    color_attachment_ids,
                    depth_attachment_id,
                );
                resource_map.insert(
                    input.as_untyped(),
                    FGResourceBackendId {
                        resource_id: raster_pipeline.as_untyped(),
                        expected_type: std::any::TypeId::of::<RasterPipeline>(),
                    },
                );
            }
        }
    }

    fn flush(&mut self) {
        let mut session = self.session.as_mut().unwrap();
        for (pass_idx, pass) in session.frame_graph.passes.iter().enumerate() {
            Self::prep_pass_inputs(
                &session.frame_graph,
                &mut session.resource_map,
                &session.supplied_inputs,
                &mut self.resource_manager,
                pass,
            );

            let recorder = if let Some(pass_fn) = &pass.pass {
                let command_buffer =
                    Self::acquire_command_buffer(&self.ctx, &mut self.command_pools)
                        .expect("Failed to acquire a command buffer.");
                let mut recorder = VulkanRecorder::new(&self.ctx, command_buffer);
                recorder.begin();

                let wait_event = if pass_idx == 0 {
                    session.buffer_writes_event.as_mut().unwrap()
                } else {
                    session.pass_set_events[pass_idx - 1]
                        .get_or_insert_with(|| self.ctx.create_frame_event())
                };
                recorder.wait_event(*wait_event);

                let ctx = FrameGraphContext {
                    frame_graph: &session.frame_graph,
                    resource_map: &session.resource_map,
                    supplied_inputs: &session.supplied_inputs,
                };

                pass_fn(&mut recorder, &ctx);

                // True for all but the last pass.
                if pass_idx < session.pass_set_events.len() {
                    let curr_pass_event = session.pass_set_events[pass_idx]
                        .get_or_insert_with(|| self.ctx.create_frame_event());

                    recorder.set_event(*curr_pass_event);
                }

                recorder
            } else {
                // TODO: Delay this until end_frame so flush can amortize building passes.
                session.recorded_pass_refs.remove(&pass.id).expect(&format!(
                    "Pass reference for pass {} was not provided before the end of the frame.",
                    session.frame_graph.resource_infos[pass.id.id() as usize].name
                ))
            };

            session.recorded_command_buffers.push(recorder);
        }
    }
}

impl Drop for VulkanFrameGraphExecutor {
    fn drop(&mut self) {
        println!("Dropping executor");
        // TODO: Resource tracking so we dont block like this.
        unsafe { self.ctx.device().device_wait_idle() };
        for command_pool in &self.command_pools {
            unsafe {
                self.ctx
                    .device()
                    .destroy_command_pool(command_pool.command_pool, None)
            }
        }
    }
}

impl GraphicsBackendFrameGraphExecutor for VulkanFrameGraphExecutor {
    fn begin_frame(&mut self, frame_graph: FrameGraph) {
        let curr_cmd_pool = &mut self.command_pools[self.ctx.curr_cpu_frame_index() as usize];
        //debug!("Resetting command pool {:?}", curr_cmd_pool.command_pool);
        unsafe {
            self.ctx.device().reset_command_pool(
                curr_cmd_pool.command_pool,
                ash::vk::CommandPoolResetFlags::empty(),
            )
        };
        curr_cmd_pool
            .free_command_buffers
            .append(&mut curr_cmd_pool.in_use_command_buffers);

        self.session = Some(FrameSession::new(frame_graph));
        self.resource_manager.retire_resources();

        // Invalidate pipelines before initializing resources so we can ensure we have the most up
        // to date shader modification tree state.
        self.resource_manager.try_invalidate_shaders();
        self.initialize_session_resources();
    }

    fn end_frame(&mut self) -> FrameGraph {
        self.flush();
        let mut session = self.session.take().unwrap();
        if session.recorded_command_buffers.is_empty() {
            warn!("Frame graph was executed but didn't record any command buffers.");
            return session.frame_graph;
        }

        // Record staging buffer transfer operations.
        let staging_buffer_copies_vk_command_buffer = {
            let command_buffer = Self::acquire_command_buffer(&self.ctx, &mut self.command_pools)
                .expect("Failed to acquire command buffer.");
            let mut transition_recorder = VulkanRecorder::new(&self.ctx, command_buffer);
            transition_recorder.begin();
            self.ctx.record_buffer_writes(&mut transition_recorder);
            transition_recorder.set_event(session.buffer_writes_event.take().unwrap());
            transition_recorder.finish();

            command_buffer
        };

        let swapchain_image_id = session
            .resource_map
            .get(&session.frame_graph.swapchain_image.as_untyped())
            .unwrap();
        let recorder_count = session.recorded_command_buffers.len();
        for (i, recorder) in session.recorded_command_buffers.iter_mut().enumerate() {
            let is_last = i == recorder_count - 1;
            if is_last {
                recorder.transition_images(
                    &[VulkanImageTransition {
                        image_id: ResourceId::new(swapchain_image_id.resource_id.id()),
                        new_layout: ash::vk::ImageLayout::PRESENT_SRC_KHR,
                        new_access_flags: ash::vk::AccessFlags::empty(),
                    }],
                    None,
                    ash::vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                );
            }
            recorder.finish();
        }

        let mut command_buffer_infos = vec![ash::vk::CommandBufferSubmitInfo::default()
            .command_buffer(staging_buffer_copies_vk_command_buffer)];
        for recorder in session.recorded_command_buffers.iter() {
            command_buffer_infos.push(
                ash::vk::CommandBufferSubmitInfo::default()
                    .command_buffer(recorder.command_buffer()),
            );
        }

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
        let present_timer = Instant::now();
        unsafe {
            self.ctx
                .swapchain_loader()
                .queue_present(self.ctx.main_queue(), &present_info)
        };
        // debug!(
        //     "Took {}ms to present swapchain image with present_mode {:?}.",
        //     present_timer.elapsed().as_micros() as f32 / 1000.0,
        //     self.ctx.swapchain().create_info.present_mode
        // );

        session.frame_graph
    }

    fn write_buffer(&mut self, name: &str, size: u64) -> &mut [u8] {
        let session = self.session.as_mut().unwrap();
        let Some(resource_info) = session.frame_graph.resource_name_map.get(name) else {
            panic!(
                "Resource with name `{}` doesn't exists in the frame_graph.",
                name
            );
        };
        assert_eq!(resource_info.type_id, std::any::TypeId::of::<Buffer>());

        let frame_graph_resource = FrameGraphResource::new(resource_info.id);
        if !session
            .frame_graph
            .frame_buffers
            .contains(&frame_graph_resource)
        {
            panic!("Can only write to frame owned, executor owned buffers from the executor.");
        }

        if let Some(existing_buffer) = session.resource_map.get(&frame_graph_resource.as_untyped())
        {
            panic!("Already wrote to the buffer once this frame, will support multiple writes to the same buffer some times in the future, with custom task ordering.");
        }

        let frame_buffer_id = self
            .resource_manager
            .get_or_create_frame_buffer(name, FrameGraphBufferInfo { size });
        session.resource_map.insert(
            frame_graph_resource.as_untyped(),
            FGResourceBackendId {
                resource_id: frame_buffer_id.as_untyped(),
                expected_type: std::any::TypeId::of::<Buffer>(),
            },
        );

        let write_ptr = self.ctx.write_buffer(&frame_buffer_id, 0, size);
        unsafe { std::slice::from_raw_parts_mut(write_ptr, size as usize) }
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
            panic!("Can't supply a frame graph image input twice in one frame.");
        }
    }

    fn supply_buffer_ref(&mut self, name: &str, buffer: &ResourceId<Buffer>) {
        let session = self.session_mut();
        let resource = session
            .frame_graph
            .resource_name_map
            .get(name)
            .expect(&format!(
                "The resource `{}` doesn't exist in the executing frame graph",
                name
            ));
        assert_eq!(resource.type_id, std::any::TypeId::of::<Buffer>());

        let prev = session.resource_map.insert(
            FrameGraphResource::new(resource.id),
            FGResourceBackendId {
                resource_id: buffer.as_untyped(),
                expected_type: std::any::TypeId::of::<Buffer>(),
            },
        );
        if prev.is_some() {
            panic!("Can't supply a frame graph buffer input twice in one frame.");
        }
    }

    fn supply_pass_ref(&mut self, name: &str, mut pass: &mut dyn GfxPassOnceImpl) {
        let command_buffer = Self::acquire_command_buffer(&self.ctx, &mut self.command_pools)
            .expect("Failed to acquire command buffer.");

        let session = self.session.as_mut().unwrap();
        let pass_resource_id = session
            .frame_graph
            .resource_name_map
            .get(name)
            .expect(&format!(
                "Resource pass reference of name `{}` doesn't exist in frame graph.",
                name
            ));
        let (pass_idx, pass_info) = session
            .frame_graph
            .passes
            .iter()
            .enumerate()
            .find(|(i, info)| info.id.id() == pass_resource_id.id)
            .expect("Tried to supply a pass input to a pass with unnecessary outputs.");

        Self::prep_pass_inputs(
            &session.frame_graph,
            &mut session.resource_map,
            &session.supplied_inputs,
            &mut self.resource_manager,
            pass_info,
        );

        let mut recorder = VulkanRecorder::new(&self.ctx, command_buffer);
        let ctx = FrameGraphContext {
            frame_graph: &session.frame_graph,
            resource_map: &session.resource_map,
            supplied_inputs: &session.supplied_inputs,
        };

        recorder.begin();

        let wait_event = if pass_idx == 0 {
            session.buffer_writes_event.as_mut().unwrap()
        } else {
            session.pass_set_events[pass_idx - 1]
                .get_or_insert_with(|| self.ctx.create_frame_event())
        };
        recorder.wait_event(*wait_event);

        pass.run(&mut recorder, &ctx);

        // True for all but the last pass.
        if pass_idx < session.pass_set_events.len() {
            let curr_pass_event = session.pass_set_events[pass_idx]
                .get_or_insert_with(|| self.ctx.create_frame_event());

            recorder.set_event(*curr_pass_event);
        }

        let old = session
            .recorded_pass_refs
            .insert(FrameGraphResource::new(pass_resource_id.id), recorder);
        assert!(
            old.is_none(),
            "Pass reference was already inserted prior in this frame."
        );
    }

    fn supply_input(&mut self, name: &str, input_data: Box<dyn std::any::Any>) {
        let session = self.session_mut();
        let resource = session.frame_graph.get_handle_untyped(name);

        session.supplied_inputs.insert(resource, input_data);
    }

    /// Writes any library exposed uniforms
    fn write_uniforms(&mut self, write_fn: &mut dyn FnMut(&mut ShaderWriter, &FrameGraphContext)) {
        let session = self.session.as_mut().unwrap();

        let (library_bindings, _) = self
            .resource_manager
            .cached_library_bindings
            .as_ref()
            .expect("Library bindings should be reflected by now.");

        let mut writer = ShaderWriter::new(library_bindings, true);
        write_fn(&mut writer, &session.create_context());
        writer.validate();

        let writes = writer
            .take_set_data()
            .into_iter()
            .map(|(set_index, set)| {
                let set_info = library_bindings
                    .iter()
                    .find(|s| s.set_index == set_index)
                    .unwrap();
                (set_info, set)
            })
            .collect::<HashMap<_, _>>();
        self.ctx.write_shader_sets(writes);
    }
}
