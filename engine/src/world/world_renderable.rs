use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::graphics::{
    backend::{Buffer, GfxBufferCreateInfo, Image, ResourceId},
    device::DeviceResource,
    frame_graph::{
        FrameGraphBuilder, FrameGraphComputeInfo, FrameGraphComputePipelineInfo,
        FrameGraphResource, IntoFrameGraphResource, Pass,
    },
    gpu_allocator::GpuBufferAllocator,
    renderer::Renderer,
    shader::ShaderPath,
};
use crate::resource::ResMut;
use crate::world::region_map::RegionPos;

struct TerrainRenderableGraphConstants {
    rt_pass_name: &'static str,
    rt_compute_pipeline_name: &'static str,
    rt_compute_pipeline_info: FrameGraphComputeInfo<'static>,
}

// Flat array of chunks which acts as a sliding window as the player.
struct TerrainRenderableWindow {
    pub side_length: u32,
    pub gpu_region_ptrs: Vec<u32>,

    pub window_offset: Vector3<u32>,
    pub region_anchor: RegionPos,

    pub region_window_buffer: Option<ResourceId<Buffer>>,
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
            region_anchor: center_region.map(|x| x - render_distance as i32),
            region_window_buffer: None,
        }
    }

    pub fn write_render_data(&mut self, device: &mut DeviceResource) {
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
}

#[derive(Resource)]
pub struct WorldRenderable {
    region_data_buffer: GpuBufferAllocator,
    window: TerrainRenderableWindow,

    // The center region we are rendering from.
    region_center: RegionPos,
    region_render_distance: u32,

    entities_accel_buf: Option<ResourceId<Buffer>>,
    entities_accel_buf_count: u32,
}

impl WorldRenderable {
    const GRAPH: TerrainRenderableGraphConstants = TerrainRenderableGraphConstants {
        rt_pass_name: "world_render_pass",
        rt_compute_pipeline_name: "world_render_compute_pipeline",
        rt_compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "rt_prepass",
            entry_point_fn: "main",
        },
    };

    const INITIAL_REGION_DATA_BUFFER_SIZE: u64 = 16 * 1024 * 1024; // 16 MB

    pub fn new(device: &mut DeviceResource) -> Self {
        let render_distance = 8;
        let region_center = RegionPos::zeros();
        Self {
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
        }
    }

    /// Handles the iterator of which regions should be loaded next for rendering.
    pub fn update(world: ResMut<WorldRenderable>) {}

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

    pub fn write_render_data(
        mut world: ResMut<WorldRenderable>,
        mut device: ResMut<DeviceResource>,
    ) {
        world.write_entity_render_data(&mut device);
        world.window.write_render_data(&mut device);
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
