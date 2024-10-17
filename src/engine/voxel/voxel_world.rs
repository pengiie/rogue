use std::{
    borrow::Borrow,
    ops::{Deref, DerefMut},
};

use hecs::Entity;
use log::debug;
use nalgebra::{allocator, Vector3};
use rogue_macros::Resource;

use crate::{
    common::aabb::AABB,
    engine::{
        ecs::ecs_world::ECSWorld,
        graphics::device::DeviceResource,
        physics::transform::Transform,
        resource::{Res, ResMut},
        voxel::vox_consts,
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
pub struct VoxelWorld {}

impl VoxelWorld {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Resource)]
pub struct VoxelWorldGpu {
    /// The acceleration buffer for voxel model bounds interaction.
    world_acceleration_buffer: Option<wgpu::Buffer>,
    // Rendered voxel models, count of models in acceleration buffer.
    rendered_voxel_model_count: u32,

    /// The allocator that owns and manages the world data buffer holding all the voxel model
    /// information.
    allocator: Option<VoxelAllocator>,

    // Some gpu object was changed (handle-wise), signals bind group recreation.
    is_dirty: bool,
}

impl VoxelWorldGpu {
    pub fn new() -> Self {
        Self {
            world_acceleration_buffer: None,
            rendered_voxel_model_count: 0,
            allocator: None,
            is_dirty: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    pub fn update_gpu_objects(
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        device: Res<DeviceResource>,
        ecs_world: Res<ECSWorld>,
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

        if voxel_world_gpu.allocator.is_none() {
            voxel_world_gpu.allocator = Some(VoxelAllocator::new(&device, 1 << 24));
            voxel_world_gpu.is_dirty = true;
        }
        let allocator = voxel_world_gpu.allocator.as_mut().unwrap();

        let mut renderable_voxel_models_query = ecs_world.query::<(
            &VoxelModel<VoxelModelESVO>,
            &mut VoxelModelGpu<<VoxelModelESVO as VoxelModelImplConcrete>::Gpu>,
        )>();
        let mut renderable_voxel_models = renderable_voxel_models_query
            .into_iter()
            .map(|(entity, (model, model_gpu))| {
                (
                    entity,
                    (
                        model.deref() as &dyn VoxelModelImpl,
                        model_gpu.deref_mut() as &mut dyn VoxelModelGpuImpl,
                    ),
                )
            })
            .collect::<Vec<_>>();
        for (entity, (model, model_gpu)) in renderable_voxel_models {
            model_gpu.update_gpu_objects(allocator, model);
        }
    }

    pub fn write_render_data(
        mut voxel_world: ResMut<VoxelWorld>,
        mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
        ecs_world: Res<ECSWorld>,
        device: Res<DeviceResource>,
    ) {
        let Some(allocator) = &mut voxel_world_gpu.allocator else {
            return;
        };
        // Update gpu model buffer data (Do this first so the allocation data is ready )
        {
            for (entity, (voxel_model, voxel_model_gpu)) in ecs_world
                .query::<((
                    &mut VoxelModel<VoxelModelESVO>,
                    &mut VoxelModelGpu<VoxelModelESVOGpu>,
                ))>()
                .into_iter()
            {
                voxel_model_gpu.deref_mut().write_gpu_updates(
                    &device,
                    allocator,
                    voxel_model.deref_mut() as &mut dyn VoxelModelImpl,
                );
            }
        }

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
                let Some(mut model_info) = voxel_model_gpu.aggregate_model_info() else {
                    continue;
                };
                assert!(!model_info.is_empty());

                let world_min = transform.isometry.translation.vector;
                let world_max = world_min
                    + voxel_model
                        .length()
                        .map(|x| x as f32 * vox_consts::VOXEL_WORLD_UNIT_LENGTH);
                let aabb = AABB::new(world_min, world_max);
                let min_bits = aabb.min.map(|x| x.to_bits()).data.0[0];
                let max_bits = aabb.max.map(|x| x.to_bits()).data.0[0];

                let model_data_size = 8 + model_info.len();
                acceleration_data.push(model_data_size as u32);
                acceleration_data.push(voxel_model.schema() as u32);
                acceleration_data.push(min_bits[0]);
                acceleration_data.push(min_bits[1]);
                acceleration_data.push(min_bits[2]);
                acceleration_data.push(max_bits[0]);
                acceleration_data.push(max_bits[1]);
                acceleration_data.push(max_bits[2]);
                acceleration_data.append(&mut model_info);
            }

            // Used in the gpu world info to know the limits of the acceleration buffer.
            voxel_world_gpu.rendered_voxel_model_count = renderable_voxel_model_count as u32;

            device.queue().write_buffer(
                voxel_world_gpu.world_acceleration_buffer(),
                0,
                bytemuck::cast_slice(&acceleration_data),
            );
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

    pub fn world_data_buffer(&self) -> Option<&wgpu::Buffer> {
        self.allocator
            .as_ref()
            .map(|allocator| allocator.world_data_buffer())
    }
}
