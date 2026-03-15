use std::collections::HashSet;

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    entity::{
        RenderableVoxelEntity,
        ecs_world::{ECSWorld, Entity},
    },
    event::{EventReader, Events},
    graphics::{
        backend::{Buffer, ResourceId, ShaderWriter},
        device::DeviceResource,
    },
    material::{MaterialBank, material_gpu::MaterialBankGpu},
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
    loaded_models: HashSet<VoxelModelId>,
    model_event_reader: EventReader<VoxelModelEvent>,
}

impl WorldEntitiesGpu {
    pub fn new() -> Self {
        Self {
            entity_accel_buf: None,
            written_entity_count: 0,

            loaded_models: HashSet::new(),
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
        let mut updated_entity_models = HashSet::new();
        for event in entities_gpu.model_event_reader.read(&events) {
            match event {
                VoxelModelEvent::UpdatedModel(voxel_model_id) => {
                    updated_entity_models.insert(*voxel_model_id);
                }
            }
        }

        // Only load entities if all the material are loaded.
        // TODO: Figure out possible per voxel model material depenencies so we have proper
        // palettes but idk that takes time this is easy to wait for all materials for now.
        let mut can_load_models = true;
        if material_bank.loading_materials() {
            can_load_models = false;
        }
        for (material_id, _) in material_bank.materials.iter_with_handle() {
            if !material_bank_gpu.is_material_loaded(material_id) {
                can_load_models = false;
                break;
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
            let Some(gpu_model_ptr) = voxel_registry_gpu.get_model_gpu_ptr(&voxel_model_id) else {
                if !can_load_models {
                    // We can't load the model yet, so skip for now.
                    continue;
                }

                // Try and load the gpu model for the entity.
                if entities_gpu.loaded_models.contains(&voxel_model_id) {
                    // We encountered this model before and already requested to load it.
                    continue;
                }
                voxel_registry_gpu.load_gpu_model(voxel_model_id);
                let side_length = voxel_registry.get_dyn_model(voxel_model_id).length();
                baker_gpu.create_model_bake_request(
                    voxel_model_id,
                    ModelBakeRequest {
                        offset: Vector3::new(0, 0, 0),
                        size: side_length,
                    },
                );
                entities_gpu.loaded_models.insert(voxel_model_id);
                continue;
            };

            assert!(entities_gpu.loaded_models.contains(&voxel_model_id));
            if updated_entity_models.contains(&voxel_model_id) {
                voxel_registry_gpu.mark_gpu_model_update(&voxel_model_id);
                let model_side_length = voxel_registry.get_dyn_model(voxel_model_id).length();
                baker_gpu.create_model_bake_request(
                    voxel_model_id,
                    ModelBakeRequest {
                        offset: Vector3::new(0, 0, 0),
                        size: model_side_length,
                    },
                );
            }

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
            let obb = transform.as_voxel_model_obb(voxel_model.length());
            let aabb = obb.aabb;
            // Transpose cause slang is column major.
            let rotation = transform.rotation.to_homogeneous();
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
