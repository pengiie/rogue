use std::collections::HashSet;

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::material::material_bank::MaterialBank;
use crate::voxel::voxel_registry_gpu::GpuModelAllocationContext;
use crate::{
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    event::{EventReader, Events},
    graphics::{
        backend::{Buffer, ResourceId, ShaderWriter},
        device::DeviceResource,
    },
    material::material_gpu::MaterialBankGpu,
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::{
        baker_gpu::{ModelBakeRequest, VoxelBakerGpu},
        voxel_registry::{VoxelModelEvent, VoxelModelId, VoxelModelRegistry},
        voxel_registry_gpu::VoxelModelRegistryGpu,
    },
};

pub enum WorldEntityGpuEvent {}

#[derive(Resource)]
pub struct WorldEntitiesGpu {
    entity_accel_buf: Option<ResourceId<Buffer>>,
    written_entity_count: u32,

    /// Entity models which have their gpu model loaded
    pending_loading_models: HashSet<VoxelModelId>,
    pending_update_models: HashSet<VoxelModelId>,
    model_event_reader: EventReader<VoxelModelEvent>,
}

impl WorldEntitiesGpu {
    pub fn new() -> Self {
        Self {
            entity_accel_buf: None,
            written_entity_count: 0,

            pending_loading_models: HashSet::new(),
            pending_update_models: HashSet::new(),
            model_event_reader: EventReader::new(),
        }
    }

    pub fn write_render_data(
        mut entities_gpu: ResMut<WorldEntitiesGpu>,
        mut device_resource: ResMut<DeviceResource>,
        ecs_world: Res<ECSWorld>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
        voxel_registry: Res<VoxelModelRegistry>,
        mut baker_gpu: ResMut<VoxelBakerGpu>,
        material_bank: Res<MaterialBank>,
        material_bank_gpu: Res<MaterialBankGpu>,
        events: Res<Events>,
    ) {
        let entities_gpu = &mut *entities_gpu;
        for event in entities_gpu.model_event_reader.read(&events) {
            match event {
                VoxelModelEvent::UpdatedModel(voxel_model_id) => {
                    entities_gpu.pending_update_models.insert(*voxel_model_id);
                }
            }
        }

        let mut entity_accel_data = Vec::new();

        entities_gpu.written_entity_count = 0;
        for (entity, (transform, renderable)) in ecs_world
            .query::<(&Transform, &RenderableVoxelEntity)>()
            .into_iter()
        {
            let Some(voxel_model_id) = renderable.voxel_model_id() else {
                continue;
            };

            if entities_gpu.pending_update_models.contains(&voxel_model_id)
                || voxel_registry_gpu
                    .get_model_gpu_ptr(&voxel_model_id)
                    .is_none()
            {
                // Try and load the gpu model for the entity.
                let side_length = voxel_registry.get_dyn_model(voxel_model_id).length();
                let success = voxel_registry_gpu.allocate_or_update_model(
                    &mut GpuModelAllocationContext {
                        registry: &voxel_registry,
                        device: &mut device_resource,
                        material_bank: &material_bank,
                        material_bank_gpu: &material_bank_gpu,
                    },
                    voxel_model_id,
                );
                if success {
                    entities_gpu.pending_update_models.remove(&voxel_model_id);
                    baker_gpu.create_model_bake_request(
                        voxel_model_id,
                        ModelBakeRequest {
                            offset: Vector3::new(0, 0, 0),
                            size: side_length,
                        },
                    );
                }

                continue;
            };
            let gpu_model_ptr = voxel_registry_gpu
                .get_model_gpu_ptr(&voxel_model_id)
                .expect("Model ptr should exist since we checked above");

            #[repr(C)]
            #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
            struct EntityInfo {
                // 4 floats to account for alignment
                aabb_min: [f32; 4],
                aabb_max: [f32; 4],
                rotation_1: [f32; 4],
                rotation_2: [f32; 4],
                rotation_3: [f32; 3],
                model_ptr: u32,
            }
            let voxel_model = voxel_registry.get_dyn_model(voxel_model_id);
            let world_transform = ecs_world.get_world_transform(entity, &transform);
            let obb = world_transform.as_voxel_model_obb(voxel_model.length());
            let aabb = obb.aabb;
            // TODO: It feels like the matrix is getting inverted somewhere being sent to the
            // shader but i havent debuged it yet so im not sure if it is or what, its
            // just weird the math works currently cause theoretically it shouldnt be working i
            // need to send the inverted rotation which im not explicitly doing idk.
            let rotation = world_transform.rotation.to_homogeneous();
            let entity_info = EntityInfo {
                aabb_min: [aabb.min.x, aabb.min.y, aabb.min.z, 0.0],
                aabb_max: [aabb.max.x, aabb.max.y, aabb.max.z, 0.0],
                rotation_1: [rotation.m11, rotation.m21, rotation.m31, 0.0],
                rotation_2: [rotation.m12, rotation.m22, rotation.m32, 0.0],
                rotation_3: [rotation.m13, rotation.m23, rotation.m33],
                model_ptr: gpu_model_ptr,
            };
            entity_accel_data.extend_from_slice(bytemuck::bytes_of(&entity_info));
            entities_gpu.written_entity_count += 1;
        }

        let req_bytes = entity_accel_data.len() as u64;
        device_resource.create_or_reallocate_buffer(
            &mut entities_gpu.entity_accel_buf,
            crate::graphics::backend::GfxBufferCreateInfo {
                name: "entities_accel_buffer".to_owned(),
                size: req_bytes.max(16), // Can't do zero sized buffer and needed in descriptor i
                                         // need a better way for this.
            },
        );

        if !entity_accel_data.is_empty() {
            device_resource.write_buffer_slice(
                entities_gpu
                    .entity_accel_buf
                    .as_ref()
                    .expect("Bad if this failed to allocate."),
                0,
                &entity_accel_data,
            );
        }
    }

    pub fn write_global_uniforms(&self, writer: &mut ShaderWriter) {
        writer.write_uniform::<u32>(
            "u_frame.voxel.entity_data.entity_count",
            self.written_entity_count,
        );
        writer.write_binding(
            "u_frame.voxel.entity_data.accel_buf",
            self.entity_accel_buf
                .expect("Accelleration buffer should exist by now"),
        );
    }
}
