use std::collections::{HashMap, HashSet};

use rogue_macros::Resource;

use crate::{
    asset::asset::{AssetHandle, Assets, GameAssetPath},
    common::geometry::ray::Ray,
    entity::{
        RenderableVoxelEntity,
        ecs_world::{ECSWorld, Entity},
    },
    physics::transform::Transform,
    resource::ResMut,
    voxel::{
        voxel::VoxelModelTrace,
        voxel_registry::{VoxelModelId, VoxelModelRegistry},
    },
    world::entity_bvh::EntityBVH,
};

/// Manages the streaming of entities and their assets.
#[derive(Resource)]
pub struct WorldEntities {
    loaded_entity_models: HashSet<VoxelModelId>,

    /// Entities waiting on the voxel asset to be loaded.
    loading_renderable_entities: HashMap<GameAssetPath, Vec<Entity>>,
}

pub struct WorldEntityRaycastHit {
    pub entity: Entity,
    pub model_id: VoxelModelId,
    pub model_trace: VoxelModelTrace,
}

impl WorldEntities {
    pub fn new() -> Self {
        Self {
            loaded_entity_models: HashSet::new(),

            loading_renderable_entities: HashMap::new(),
        }
    }

    pub fn raycast_voxel_entities(
        ray: &Ray,
        ecs_world: &ECSWorld,
        voxel_registry: &VoxelModelRegistry,
    ) -> Option<WorldEntityRaycastHit> {
        let mut hit: Option<WorldEntityRaycastHit> = None;
        for (entity, (transform, renderable)) in ecs_world
            .query::<(&Transform, &RenderableVoxelEntity)>()
            .into_iter()
        {
            let Some(model_id) = renderable.voxel_model_id() else {
                continue;
            };
            let model = voxel_registry.get_dyn_model(model_id);
            let model_side_length = model.length();
            let obb = transform.as_voxel_model_obb(model_side_length);
            let rotated_ray_pos = obb
                .rotation
                .transform_vector(&(ray.origin - transform.position))
                + transform.position;
            let rotated_ray_dir = obb.rotation.transform_vector(&ray.dir);
            let rotated_ray = Ray::new(rotated_ray_pos, rotated_ray_dir);
            if let Some(trace) = model.trace(&rotated_ray, &obb.aabb) {
                let should_update_hit = match &hit {
                    Some(current_hit) => trace.depth_t < current_hit.model_trace.depth_t,
                    None => true,
                };
                if should_update_hit {
                    hit = Some(WorldEntityRaycastHit {
                        entity,
                        model_id,
                        model_trace: trace,
                    });
                }
            }
        }

        hit
    }

    pub fn load_entity_models(
        mut entities: ResMut<WorldEntities>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        ecs_world: ResMut<ECSWorld>,
        assets: ResMut<Assets>,
    ) {
        for (entity, renderable) in ecs_world.query::<&mut RenderableVoxelEntity>().into_iter() {
            if !renderable.needs_loading() {
                continue;
            }
            let asset_path = renderable
                .model_asset_path()
                .expect("If needs_loading is true, then an asset path should exist");
            if !voxel_registry.static_asset_models.contains_key(asset_path) {
                voxel_registry.load_asset_model(asset_path);
            }
            // Put all in here since then we only need to update renderables in one spot down
            // below.
            entities
                .loading_renderable_entities
                .entry(asset_path.clone())
                .or_default()
                .push(entity);
        }

        let mut finished_assets = Vec::new();
        for (asset_path, entities) in &entities.loading_renderable_entities {
            let Some(model_id) = voxel_registry.static_asset_models.get(&asset_path) else {
                // Model is still loading.
                continue;
            };
            let model_id = model_id.clone();
            finished_assets.push(asset_path.clone());

            for entity in entities {
                let Ok(mut renderable) = ecs_world.get::<&mut RenderableVoxelEntity>(*entity)
                else {
                    continue;
                };
                if renderable.model_asset_path() != Some(&asset_path) {
                    // Asset path of model must've changed while we were loading.
                    continue;
                }
                if renderable.is_dynamic() {
                    let unique_model_id = voxel_registry.clone_model(model_id);
                    renderable.set_model_id(unique_model_id);
                } else {
                    renderable.set_model_id(model_id);
                }
            }
        }

        for asset_path in finished_assets {
            entities.loading_renderable_entities.remove(&asset_path);
        }
    }
}
