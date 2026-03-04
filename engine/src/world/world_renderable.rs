use std::collections::{HashMap, VecDeque};

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::consts;
use crate::event::{EventReader, Events};
use crate::graphics::backend::GraphicsBackendRecorder;
use crate::graphics::frame_graph::FrameGraphContext;
use crate::graphics::gpu_allocator::Allocation;
use crate::resource::{Res, ResMut};
use crate::voxel::voxel_registry_gpu::{self, VoxelModelRegistryGpu};
use crate::world::region::{RegionTree, WorldRegion, WorldRegionNode};
use crate::world::region_map::{
    ChunkEvent, ChunkEventType, ChunkId, ChunkLOD, ChunkPos, RegionEvent, RegionEventType,
    RegionMap, RegionPos,
};
use crate::{
    graphics::{
        backend::{Buffer, GfxBufferCreateInfo, Image, ResourceId},
        device::DeviceResource,
        frame_graph::{
            FrameGraphBuilder, FrameGraphComputeInfo, FrameGraphComputePipelineInfo,
            FrameGraphResource, IntoFrameGraphResource, Pass,
        },
        gpu_allocator::GpuBufferAllocator,
        renderer::Renderer,
        shader::ShaderPath,
    },
    world::region_iter::RegionIter,
};

struct ChunkBakeRequest {
    pub offset: Vector3<u32>,
    pub size: Vector3<u32>,
}

struct TerrainRenderableGraphConstants {
    rt_pass_name: &'static str,
    rt_compute_pipeline_name: &'static str,
    rt_compute_pipeline_info: FrameGraphComputeInfo<'static>,

    bake_pass_name: &'static str,
    bake_chunk_compute_pipeline_name: &'static str,
    bake_chunk_compute_pipeline_info: FrameGraphComputeInfo<'static>,
}

// Flat array of chunks which acts as a sliding window as the player.
struct TerrainRenderableWindow {
    pub side_length: u32,
    pub gpu_region_ptrs: Vec<u32>,

    pub window_offset: Vector3<u32>,
    pub region_anchor: RegionPos,

    pub region_window_buffer: Option<ResourceId<Buffer>>,
    // Buffer updates for the window buffer.
    window_updates: Vec<(RegionPos, /*region_ptr*/ u32)>,
}

impl TerrainRenderableWindow {
    const NULL_REGION_PTR: u32 = 0xFFFF_FFFF;

    pub fn new(center_region: RegionPos, render_distance: u32) -> Self {
        let side_length = render_distance * 2 + 1;
        let volume = side_length.pow(3);
        Self {
            side_length,
            gpu_region_ptrs: vec![Self::NULL_REGION_PTR; volume as usize],
            window_offset: Vector3::zeros(),
            region_anchor: center_region.map(|x| x - render_distance as i32).into(),
            region_window_buffer: None,
            window_updates: Vec::new(),
        }
    }

    pub fn contains_region(&self, region_pos: RegionPos) -> bool {
        let rel_pos = region_pos - self.region_anchor;
        if rel_pos.x < 0
            || rel_pos.y < 0
            || rel_pos.z < 0
            || rel_pos.x >= self.side_length as i32
            || rel_pos.y >= self.side_length as i32
            || rel_pos.z >= self.side_length as i32
        {
            return false;
        }
        true
    }

    pub fn mem_pos_to_index(side_length: u32, local_pos: Vector3<u32>) -> usize {
        (local_pos.x + (local_pos.y * side_length) + (local_pos.z * side_length * side_length))
            as usize
    }

    pub fn local_pos_to_mem_pos(
        local_pos: Vector3<u32>,
        window_offset: Vector3<u32>,
        side_length: u32,
    ) -> Vector3<u32> {
        Vector3::new(
            (local_pos.x + window_offset.x) % side_length,
            (local_pos.y + window_offset.y) % side_length,
            (local_pos.z + window_offset.z) % side_length,
        )
    }

    pub fn set_region_ptr(&mut self, region_pos: RegionPos, region_ptr: u32) {
        if !self.contains_region(region_pos) {
            log::error!("Tried to set region ptr for region outside of window!");
            return;
        }
        let index = Self::mem_pos_to_index(
            self.side_length,
            (region_pos - self.region_anchor).map(|x| x as u32),
        );
        if self.gpu_region_ptrs[index] != region_ptr {
            self.gpu_region_ptrs[index] = region_ptr;
            self.window_updates.push((region_pos, region_ptr));
        }
    }

    pub fn update_gpu_objects(&mut self, device: &mut DeviceResource) {
        let req_buffer_size = (self.gpu_region_ptrs.len() * 4) as u64;
        let mut needs_resize = false;
        if let Some(region_window_buffer) = &self.region_window_buffer {
            let buffer_info = device.get_buffer_info(region_window_buffer);
            if buffer_info.size != req_buffer_size {
                needs_resize = true;
            }
        } else {
            needs_resize = true;
        }

        if needs_resize {
            if let Some(old_buffer) = &self.region_window_buffer {
                // TODO: Delete old buffer.
            }
            let new_buffer = device.create_buffer(GfxBufferCreateInfo {
                name: "terrain_region_window_buffer".to_string(),
                size: req_buffer_size,
            });
            device.write_buffer_slice(
                &new_buffer,
                0,
                bytemuck::cast_slice(self.gpu_region_ptrs.as_slice()),
            );
            self.region_window_buffer = Some(new_buffer);
        }
    }

    pub fn write_render_data(&mut self, device: &mut DeviceResource) {
        let Some(region_window_buffer) = &self.region_window_buffer else {
            return;
        };

        for (region_pos, new_ptr) in self.window_updates.drain(..) {
            let local_pos = (region_pos - self.region_anchor).map(|x| x as u32);
            let mem_pos =
                Self::local_pos_to_mem_pos(local_pos, self.window_offset, self.side_length);
            let index = Self::mem_pos_to_index(self.side_length, mem_pos) as u64;
            device.write_buffer_slice(region_window_buffer, index * 4, &new_ptr.to_le_bytes());
        }
    }
}

#[derive(Resource)]
pub struct WorldRenderable {
    region_event_reader: EventReader<RegionEvent>,
    chunk_event_reader: EventReader<ChunkEvent>,
    to_write_regions: Vec<RegionPos>,

    chunk_bake_requests: HashMap<ChunkId, ChunkBakeRequest>,
    /// Queue for current frame.
    chunk_bake_request_queue: VecDeque<ChunkId>,
    curr_chunk_bakes: Vec<ChunkId>,

    region_gpu_allocations: HashMap<RegionPos, Allocation>,
    region_data_buffer: GpuBufferAllocator,
    window: TerrainRenderableWindow,

    // The center region we are rendering from.
    region_center: RegionPos,
    region_render_distance: u32,

    entities_accel_buf: Option<ResourceId<Buffer>>,
    entities_accel_buf_count: u32,

    bake_pass: Option<FrameGraphResource<Pass>>,
}

impl WorldRenderable {
    const GRAPH: TerrainRenderableGraphConstants = TerrainRenderableGraphConstants {
        rt_pass_name: "world_render_pass",
        rt_compute_pipeline_name: "world_render_compute_pipeline",
        rt_compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "rt_prepass",
            entry_point_fn: "main",
        },
        bake_pass_name: "world_bake_pass",
        bake_chunk_compute_pipeline_name: "world_bake_compute_pipeline",
        bake_chunk_compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "bake_chunk",
            entry_point_fn: "main",
        },
    };

    const INITIAL_REGION_DATA_BUFFER_SIZE: u64 = 16 * 1024 * 1024; // 16 MB

    pub fn new(device: &mut DeviceResource) -> Self {
        let render_distance = 16;
        let region_center = RegionPos::zeros();
        Self {
            region_event_reader: EventReader::new(),
            chunk_event_reader: EventReader::new(),
            to_write_regions: Vec::new(),

            chunk_bake_requests: HashMap::new(),

            region_gpu_allocations: HashMap::new(),
            region_data_buffer: GpuBufferAllocator::new(
                device,
                "region_data_buffer_allocator",
                Self::INITIAL_REGION_DATA_BUFFER_SIZE,
            ),
            window: TerrainRenderableWindow::new(region_center, render_distance),

            region_center,
            region_render_distance: render_distance,

            entities_accel_buf: None,
            entities_accel_buf_count: 0,
            bake_pass: None,
            chunk_bake_request_queue: VecDeque::new(),
            curr_chunk_bakes: Vec::new(),
        }
    }

    /// Gets the GPU byte representation of this region tree with the given LOD.
    pub fn convert_region_gpu(
        voxel_registry_gpu: &VoxelModelRegistryGpu,
        region: &WorldRegion,
    ) -> Vec<u8> {
        const HEADER_SIZE: usize = 8; // 4 bytes for header
        const NODE_SIZE: usize = 16; // 16 bytes per node, see voxel/region.slang
        let mut bytes = Vec::with_capacity(region.tree.nodes.len() * NODE_SIZE);
        // The LOD we should render at.
        let mut nodes = vec![(&region.tree.nodes[0], 0u32)];
        let mut max_depth = 0;
        while let Some((node, depth)) = nodes.pop() {
            if node.model_ptr != u32::MAX {
                max_depth = max_depth.max(depth);
            }
            if node.is_child_ptr_valid() {
                let child_ptr = node.child_ptr as usize;
                for i in 0..64 {
                    nodes.push((&region.tree.nodes[child_ptr + i], depth + 1));
                }
            }
        }
        let lod = ChunkLOD::from_tree_height(max_depth);
        bytes.extend_from_slice(&lod.0.to_le_bytes());
        for WorldRegionNode {
            model_ptr,
            parent_ptr,
            child_ptr,
            child_mask,
        } in &region.tree.nodes
        {
            let model_handle =
                (*model_ptr != u32::MAX).then(|| &region.model_handles[*model_ptr as usize]);
            let gpu_model_ptr = model_handle
                .map(|handle| voxel_registry_gpu.get_model_gpu_ptr(handle))
                .flatten()
                .unwrap_or(0xFFFF_FFFF);
            bytes.extend_from_slice(&gpu_model_ptr.to_le_bytes());
            bytes.extend_from_slice(&child_ptr.to_le_bytes());
            bytes.extend_from_slice(&child_mask.to_le_bytes());
        }

        bytes
    }

    /// Handles the iterator of which regions should be loaded next for rendering.
    pub fn update_region_gpu_repr(
        mut region_map: ResMut<RegionMap>,
        mut terrain_gpu: ResMut<WorldRenderable>,
        events: Res<Events>,
    ) {
        let terrain_gpu = &mut terrain_gpu as &mut WorldRenderable;
        for event in terrain_gpu.region_event_reader.read(&events) {
            match event.event_type {
                RegionEventType::Loaded | RegionEventType::Updated => {
                    terrain_gpu.to_write_regions.push(event.region_pos);
                }
                RegionEventType::Unloaded => {
                    log::error!("We should implement this or memory go up up up.");
                }
            }
        }
    }

    fn write_region_render_data(
        &mut self,
        device: &mut DeviceResource,
        region_map: &RegionMap,
        voxel_registry_gpu: &VoxelModelRegistryGpu,
    ) {
        for region_pos in self.to_write_regions.drain(..) {
            if !self.window.contains_region(region_pos) {
                // Region is outside our render distance.
                continue;
            }

            let region_data = region_map
                .get_region(&region_pos)
                .expect("If the region is in `to_write_regions` then it should be loaded");
            let gpu_region_data = Self::convert_region_gpu(voxel_registry_gpu, region_data);
            let mem_pos = TerrainRenderableWindow::local_pos_to_mem_pos(
                (region_pos - self.window.region_anchor).map(|x| x as u32),
                self.window.window_offset,
                self.window.side_length,
            );
            let Some(region_gpu_allocation) =
                (if let Some(old_allocation) = self.region_gpu_allocations.get(&region_pos) {
                    self.region_data_buffer
                        .reallocate(old_allocation, gpu_region_data.len() as u64)
                } else {
                    self.region_data_buffer
                        .allocate(gpu_region_data.len() as u64)
                })
            else {
                log::error!("Failed to allocate GPU memory for region.");
                continue;
            };

            self.region_data_buffer.write_allocation_data(
                device,
                &region_gpu_allocation,
                &gpu_region_data,
            );
            self.window.set_region_ptr(
                region_pos,
                region_gpu_allocation.start_index_stride_dword() as u32,
            );
            self.region_gpu_allocations
                .insert(region_pos, region_gpu_allocation);
        }
    }

    fn write_entity_render_data(&mut self, device: &mut DeviceResource) {
        const ENTITY_INFO_SIZE: u64 = 80; // 80 bytes per entity, see voxel/entity.slang
        const INITIAL_ENTITY_BUFFER_COUNT: u32 = 1024;
        let req_buffer_size = self.entities_accel_buf_count as u64 * ENTITY_INFO_SIZE;
        if let Some(entity_accel_buf) = &self.entities_accel_buf {
            let buffer_info = device.get_buffer_info(entity_accel_buf);
            if buffer_info.size < req_buffer_size {
                let new_buffer = device.create_buffer(GfxBufferCreateInfo {
                    name: "entity_acceleration_buffer".to_string(),
                    size: req_buffer_size,
                });
                self.entities_accel_buf = Some(new_buffer);
                todo!("Copy over old data");
            }
        } else {
            let new_buffer = device.create_buffer(GfxBufferCreateInfo {
                name: "entity_acceleration_buffer".to_string(),
                size: req_buffer_size.max(ENTITY_INFO_SIZE * INITIAL_ENTITY_BUFFER_COUNT as u64),
            });
            self.entities_accel_buf = Some(new_buffer);
        }
    }

    pub fn update_gpu_chunk_models(
        mut world: ResMut<WorldRenderable>,
        mut region_map: ResMut<RegionMap>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
        events: Res<Events>,
    ) {
        let world = &mut world as &mut WorldRenderable;

        // Handle region events.
        for event in world.region_event_reader.read(&events) {
            world.to_write_regions.push(event.region_pos);
        }

        for region_pos in &world.to_write_regions {
            if !world.window.contains_region(*region_pos) {
                // Region is outside our render distance.
                continue;
            }

            let region_data = region_map
                .get_region(&region_pos)
                .expect("If the region is in `to_write_regions` then it should be loaded");
            for model in &region_data.model_handles {
                voxel_registry_gpu.ensure_model_exists(model);
            }
        }

        // Handle chunk events.
        for chunk_event in world.chunk_event_reader.read(&events) {
            match chunk_event.event_type {
                ChunkEventType::Loaded | ChunkEventType::Updated => {
                    let mut chunks = chunk_event.chunk_id.neighbors();
                    chunks.push(chunk_event.chunk_id);
                    for chunk_id in chunks {
                        let model_id = region_map.get_chunk_model(&chunk_id);
                        let Some(model_id) = model_id else {
                            continue;
                        };

                        let bake_request = ChunkBakeRequest {
                            offset: Vector3::zeros(),
                            size: Vector3::new(
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                                consts::voxel::TERRAIN_CHUNK_VOXEL_LENGTH,
                            ),
                        };
                        world.chunk_bake_requests.insert(chunk_id, bake_request);
                        world.chunk_bake_request_queue.push_back(chunk_id);
                    }
                }
                _ => {}
            }
        }

        let mut i = 0;
        while let Some(chunk_id) = world.chunk_bake_request_queue.pop_front() {
            i += 1;
            if i > 12 {
                break;
            }
            let Some(bake_request) = world.chunk_bake_requests.get(&chunk_id) else {
                return;
            };
            world.curr_chunk_bakes.push(chunk_id);
            let model_id = region_map.get_chunk_model(&chunk_id).unwrap();
            voxel_registry_gpu.invalidate_model_gpu_material(&model_id);
        }
    }

    pub fn update_chunk_bake_requests(
        mut world: ResMut<WorldRenderable>,
        mut device: ResMut<DeviceResource>,
        mut region_map: ResMut<RegionMap>,
        mut events: Res<Events>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
    ) {
    }

    // Allocates and writes any necessary render data to the GPU.
    pub fn write_render_data(
        mut world: ResMut<WorldRenderable>,
        mut device: ResMut<DeviceResource>,
        mut region_map: ResMut<RegionMap>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
    ) {
        let world = &mut world as &mut WorldRenderable;

        world.window.update_gpu_objects(&mut device);
        world.write_region_render_data(&mut device, &region_map, &voxel_registry_gpu);
        world.write_entity_render_data(&mut device);
        world.window.write_render_data(&mut device);
    }

    pub fn write_graph_passes(mut world: ResMut<WorldRenderable>, mut renderer: ResMut<Renderer>) {
        let world = &mut *world;
        renderer.frame_graph_executor.supply_pass_ref(
            Self::GRAPH.bake_pass_name,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                let compute_pipeline =
                    ctx.get_compute_pipeline(Self::GRAPH.bake_chunk_compute_pipeline_name);
                let mut compute_pass = recorder.begin_compute_pass(compute_pipeline);

                let mut finished_chunk_ids = Vec::new();
                for chunk_id in world.curr_chunk_bakes.drain(..) {
                    let bake_request = world.chunk_bake_requests.get(&chunk_id).unwrap();
                    let bake_volume =
                        bake_request.size.x * bake_request.size.y * bake_request.size.z;
                    compute_pass.bind_uniforms(&mut |writer| {
                        writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                        writer.write_uniform::<Vector3<i32>>(
                            "u_shader.chunk_pos",
                            *chunk_id.chunk_pos,
                        );
                        writer.write_uniform::<u32>(
                            "u_shader.chunk_height",
                            chunk_id.chunk_lod.as_tree_height(),
                        );
                        writer.write_uniform::<Vector3<u32>>(
                            "u_shader.voxel_offset",
                            bake_request.offset,
                        );
                        writer.write_uniform::<Vector3<u32>>(
                            "u_shader.voxel_size",
                            bake_request.size,
                        );
                        writer.write_uniform("u_shader.bake_volume", bake_volume);
                    });

                    let wg_size = compute_pass.workgroup_size();
                    compute_pass.dispatch(bake_volume.div_ceil(wg_size.x), 1, 1);
                }

                for chunk_id in finished_chunk_ids {
                    world.chunk_bake_requests.remove(&chunk_id);
                }
            },
        );
    }

    pub fn set_graph_bake_pass(&mut self, fg: &mut FrameGraphBuilder) -> FrameGraphResource<Pass> {
        let compute_pipeline = fg.create_compute_pipeline(
            Self::GRAPH.bake_chunk_compute_pipeline_name,
            Self::GRAPH.bake_chunk_compute_pipeline_info,
        );

        let pass = fg.create_input_pass(Self::GRAPH.bake_pass_name, &[&compute_pipeline], &[]);
        self.bake_pass = Some(pass);

        pass
    }

    /// Adds the render pass for rendering the world (terrain and entities).
    pub fn set_graph_render_pass(
        &mut self,
        fg: &mut FrameGraphBuilder,
        framebuffer: impl IntoFrameGraphResource<Image>,
        framebuffer_depth: impl IntoFrameGraphResource<Image>,
    ) -> FrameGraphResource<Pass> {
        let compute_pipeline = fg.create_compute_pipeline(
            Self::GRAPH.rt_compute_pipeline_name,
            Self::GRAPH.rt_compute_pipeline_info,
        );

        let framebuffer_handle = framebuffer.handle(fg);
        let framebuffer_depth_handle = framebuffer_depth.handle(fg);
        let pass = fg.create_pass(
            Self::GRAPH.rt_pass_name,
            &[
                &framebuffer_handle,
                &framebuffer_depth_handle,
                &compute_pipeline,
            ],
            &[&framebuffer_handle, &framebuffer_depth_handle],
            move |recorder, ctx| {
                let framebuffer = ctx.get_image(framebuffer_handle);
                let framebuffer_depth = ctx.get_image(framebuffer_depth_handle);
                let framebuffer_size = recorder.get_image_info(&framebuffer).resolution_xy();

                let pipeline = ctx.get_compute_pipeline(&compute_pipeline);
                let mut compute_pass = recorder.begin_compute_pass(pipeline);
                let wg_size = compute_pass.workgroup_size();

                compute_pass.bind_uniforms(&mut |writer| {
                    writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                    writer.write_binding("u_shader.backbuffer", framebuffer);
                    writer.write_binding("u_shader.backbuffer_depth", framebuffer_depth);
                });

                compute_pass.dispatch(
                    (framebuffer_size.x as f32 / wg_size.x as f32).ceil() as u32,
                    (framebuffer_size.y as f32 / wg_size.y as f32).ceil() as u32,
                    1,
                );
            },
        );

        pass
    }

    pub fn entities_accel_buf(&self) -> &ResourceId<Buffer> {
        self.entities_accel_buf
            .as_ref()
            .expect("Acceleration buffer for entities should exist by now")
    }

    pub fn entities_accel_buf_count(&self) -> u32 {
        self.entities_accel_buf_count
    }

    pub fn region_data_buffer(&self) -> &ResourceId<Buffer> {
        self.region_data_buffer.buffer()
    }

    pub fn region_window_side_length(&self) -> Vector3<u32> {
        Vector3::new(
            self.window.side_length,
            self.window.side_length,
            self.window.side_length,
        )
    }

    pub fn region_window_anchor(&self) -> RegionPos {
        self.window.region_anchor
    }

    pub fn region_window_offset(&self) -> Vector3<u32> {
        self.window.window_offset
    }

    pub fn region_window_buffer(&self) -> &ResourceId<Buffer> {
        self.window
            .region_window_buffer
            .as_ref()
            .expect("Region window buffer should exist by now")
    }
}
