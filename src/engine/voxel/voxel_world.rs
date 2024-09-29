use std::borrow::Borrow;

use hecs::Entity;
use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    common::aabb::AABB,
    engine::{
        ecs::ecs_world::ECSWorld,
        graphics::device::DeviceResource,
        physics::transform::Transform,
        resource::{Res, ResMut},
    },
};

use super::{
    allocator::VoxelAllocator,
    voxel::{VoxelModel, VoxelModelSchema, VoxelRange},
};

#[derive(Resource)]
pub struct VoxelWorld {
    allocator: VoxelAllocator,
}

impl VoxelWorld {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            allocator: VoxelAllocator::new(device),
        }
    }

    // pub fn get_acceleration_data(&self) -> Box<[u32]> {
    //     let mut data = Vec::with_capacity(self.voxel_models.len() * 8); // ptr + type + vec3 + vec3
    //     for model in &self.voxel_models {
    //         let aabb = model.aabb();
    //         let min_bits = aabb.min.map(|x| x.to_bits()).data.0[0];
    //         let max_bits = aabb.max.map(|x| x.to_bits()).data.0[0];

    //         data.push(0);
    //         data.push(model.schema() as u32);
    //         data.push(min_bits[0]);
    //         data.push(min_bits[1]);
    //         data.push(min_bits[2]);
    //         data.push(max_bits[0]);
    //         data.push(max_bits[1]);
    //         data.push(max_bits[2]);
    //     }

    //     data.into_boxed_slice()
    // }
}

#[derive(Resource)]
pub struct VoxelWorldGpu {
    /// The acceleration buffer for voxel model bounds interaction.
    world_acceleration_buffer: Option<wgpu::Buffer>,
    // Rendered voxel models, count of models in acceleration buffer.
    rendered_voxel_model_count: u32,

    /// The buffer that holds the structure and attachment data for all the voxel models in the
    /// world.
    world_data_buffer: Option<wgpu::Buffer>,

    // Some gpu object was changed (handle-wise), signals bind group recreation.
    is_dirty: bool,
}

impl VoxelWorldGpu {
    pub fn new() -> Self {
        Self {
            world_acceleration_buffer: None,
            rendered_voxel_model_count: 0,
            world_data_buffer: None,
            is_dirty: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    pub fn update_gpu_objects(
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        device: Res<DeviceResource>,
    ) {
        // Refresh any dirty flag from the last frame.
        voxel_world_gpu.is_dirty = false;

        if voxel_world_gpu.world_acceleration_buffer.is_none() {
            voxel_world_gpu.world_acceleration_buffer =
                Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("world_acceleration_buffer"),
                    size: 4 * 1000, // 1000 voxel models
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            voxel_world_gpu.is_dirty = true;
        }
        if voxel_world_gpu.world_data_buffer.is_none() {
            voxel_world_gpu.world_data_buffer =
                Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("world_data_buffer"),
                    size: 1 << 28, // 268 MB
                    usage: wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_DST
                        | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }));
            voxel_world_gpu.is_dirty = true;
        }
    }

    pub fn write_render_data(
        voxel_world: Res<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        device: Res<DeviceResource>,
    ) {
        let mut renderable_voxel_models_query = ecs_world.query::<(&VoxelModel, &Transform)>();

        let renderable_voxel_model_count = renderable_voxel_models_query.iter().len();
        let mut acceleration_data = Vec::with_capacity(renderable_voxel_model_count * 8); // ptr + type + vec3 + vec3
        for (entity, (voxel_model, transform)) in renderable_voxel_models_query.iter() {
            let aabb = AABB::new(
                transform.isometry.translation.vector,
                transform.isometry.translation.vector + voxel_model.length().map(|x| x as f32),
            );
            let min_bits = aabb.min.map(|x| x.to_bits()).data.0[0];
            let max_bits = aabb.max.map(|x| x.to_bits()).data.0[0];

            acceleration_data.push(0);
            acceleration_data.push(voxel_model.schema() as u32);
            acceleration_data.push(min_bits[0]);
            acceleration_data.push(min_bits[1]);
            acceleration_data.push(min_bits[2]);
            acceleration_data.push(max_bits[0]);
            acceleration_data.push(max_bits[1]);
            acceleration_data.push(max_bits[2]);
        }

        voxel_world_gpu.rendered_voxel_model_count = renderable_voxel_model_count as u32;
        device.queue().write_buffer(
            voxel_world_gpu.world_acceleration_buffer(),
            0,
            bytemuck::cast_slice(&acceleration_data),
        );
    }

    pub fn renderable_voxel_model_count(&self) -> u32 {
        self.rendered_voxel_model_count
    }

    pub fn world_acceleration_buffer(&self) -> &wgpu::Buffer {
        self.world_acceleration_buffer
            .as_ref()
            .expect("world_acceleration_buffer not initialized when it should have been by now")
    }

    pub fn world_data_buffer(&self) -> &wgpu::Buffer {
        self.world_data_buffer
            .as_ref()
            .expect("world_data_buffer not initialized when it should have been by now")
    }
}
