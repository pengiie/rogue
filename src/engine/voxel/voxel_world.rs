use std::{
    borrow::Borrow,
    ops::{Deref, DerefMut},
};

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
    esvo::{VoxelModelESVO, VoxelModelESVOGpu},
    voxel::{
        VoxelModel, VoxelModelGpu, VoxelModelGpuImpl, VoxelModelImpl, VoxelModelImplConcrete,
        VoxelModelSchema, VoxelRange,
    },
    voxel_allocator::VoxelAllocator,
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
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        device: Res<DeviceResource>,
    ) {
        // Write acceleration buffer data.
        {
            // TODO: These will get quite ugly with more voxel models, in the short term we make a make
            // to quickly make these and zip them up into dyn impls. Long term we fork hecs or
            // hand-roll our own EC lib with dynamic typeid component queries so we can have better
            // voxel model queries with a typeid registry of the different model types. I could've
            // just used an enum with some match statements but... now we are here.
            let mut renderable_voxel_models_query = ecs_world.query::<(
                &VoxelModel<VoxelModelESVO>,
                &VoxelModelGpu<<VoxelModelESVO as VoxelModelImplConcrete>::Gpu>,
                &Transform,
            )>();
            let mut renderable_voxel_models = renderable_voxel_models_query
                .into_iter()
                .map(|(entity, (model, model_gpu, transform))| {
                    (
                        entity,
                        (
                            model.deref() as &dyn VoxelModelImpl,
                            model_gpu.deref() as &dyn VoxelModelGpuImpl,
                            transform,
                        ),
                    )
                })
                .collect::<Vec<_>>();

            let renderable_voxel_model_count = renderable_voxel_models.len();
            let mut acceleration_data = Vec::with_capacity(renderable_voxel_model_count * 8); // ptr + type + vec3 + vec3
            for (entity, (voxel_model, voxel_model_gpu, transform)) in &renderable_voxel_models {
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

            // Used in the gpu world info to know the limits of the acceleration buffer.
            voxel_world_gpu.rendered_voxel_model_count = renderable_voxel_model_count as u32;

            device.queue().write_buffer(
                voxel_world_gpu.world_acceleration_buffer(),
                0,
                bytemuck::cast_slice(&acceleration_data),
            );
        }

        // Update gpu model buffer data.
        {
            for (entity, (voxel_model, voxel_model_gpu)) in ecs_world
                .query::<((
                    &mut VoxelModel<VoxelModelESVO>,
                    &mut VoxelModelGpu<VoxelModelESVOGpu>,
                ))>()
                .into_iter()
            {
                voxel_model_gpu.deref_mut().write_gpu_updates(
                    &mut voxel_world.allocator,
                    voxel_model.deref_mut() as &mut dyn VoxelModelImpl,
                );
            }
        }
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