use std::collections::{HashMap, HashSet};

use rogue_macros::Resource;

use crate::{
    animation::animation::Animation,
    asset::asset::{AssetHandle, AssetStatus, Assets, GameAssetPath},
    common::freelist::{FreeList, FreeListHandle},
    resource::ResMut,
};

pub type AnimationId = FreeListHandle<Animation>;

// Why not just use Assets and store an AssetHandle? Because the current api for the assets
// as i write this requires me to save a filepath alongside an asset and i dont want to touch
// that api rn so we are doing this instead :p.
#[derive(Resource)]
pub struct AnimationBank {
    animations: FreeList<Animation>,
    pub asset_to_animation: HashMap<GameAssetPath, AnimationId>,

    to_load_animations: HashSet<GameAssetPath>,
    loading_animations: HashMap<GameAssetPath, AssetHandle>,
}

impl AnimationBank {
    pub fn new() -> Self {
        Self {
            animations: FreeList::new(),
            asset_to_animation: HashMap::new(),

            to_load_animations: HashSet::new(),
            loading_animations: HashMap::new(),
        }
    }

    pub fn insert_animation(
        &mut self,
        game_asset_path: GameAssetPath,
        animation: Animation,
    ) -> AnimationId {
        let id = self.animations.push(animation);
        self.asset_to_animation.insert(game_asset_path, id);
        return id;
    }

    pub fn update_loaded_animations(
        mut animation_bank: ResMut<AnimationBank>,
        mut assets: ResMut<Assets>,
    ) {
        let animation_bank = &mut *animation_bank;
        let Some(project_dir) = assets.project_dir().clone() else {
            return;
        };
        for animation_path in animation_bank.to_load_animations.drain() {
            if animation_bank
                .loading_animations
                .contains_key(&animation_path)
                || animation_bank
                    .asset_to_animation
                    .contains_key(&animation_path)
            {
                continue;
            }

            let asset_handle =
                assets.load_asset::<Animation>(animation_path.as_file_asset_path(&project_dir));
            animation_bank
                .loading_animations
                .insert(animation_path, asset_handle);
        }

        let mut finished_loading_animations = Vec::new();
        for (loading_animation_path, asset_handle) in &animation_bank.loading_animations {
            match assets.get_asset_status(&asset_handle) {
                AssetStatus::InProgress => {
                    continue;
                }
                AssetStatus::Saved => unreachable!(),
                AssetStatus::Loaded => {
                    let animation = assets.take_asset::<Animation>(&asset_handle).unwrap();
                    let animation_id = animation_bank.animations.push(*animation);
                    animation_bank
                        .asset_to_animation
                        .insert(loading_animation_path.clone(), animation_id);
                }
                AssetStatus::NotFound => {
                    log::error!(
                        "Animation asset not found at path: {}",
                        loading_animation_path.as_relative_path_str()
                    );
                }
                AssetStatus::Error(error) => {
                    log::error!(
                        "Error loading animation asset at path: {}: {}",
                        loading_animation_path.as_relative_path_str(),
                        error
                    );
                }
            }
            finished_loading_animations.push(loading_animation_path.clone());
        }

        for finished_animation in finished_loading_animations {
            animation_bank
                .loading_animations
                .remove(&finished_animation);
        }
    }

    pub fn animation_exists(&self, animation_path: &GameAssetPath) -> bool {
        self.asset_to_animation.contains_key(animation_path)
    }

    pub fn get_animation_by_path(&self, animation_path: &GameAssetPath) -> Option<&Animation> {
        let animation_id = self.asset_to_animation.get(animation_path)?;
        self.animations.get(*animation_id)
    }

    pub fn get_animation_by_path_mut(
        &mut self,
        animation_path: &GameAssetPath,
    ) -> Option<&mut Animation> {
        let animation_id = self.asset_to_animation.get(animation_path)?;
        self.animations.get_mut(*animation_id)
    }

    pub fn get_animation(&self, animation_path: &GameAssetPath) -> Option<&Animation> {
        let animation_id = self.asset_to_animation.get(animation_path)?;
        self.animations.get(*animation_id)
    }

    pub fn get_animation_mut(&mut self, animation_path: &GameAssetPath) -> Option<&mut Animation> {
        let animation_id = self.asset_to_animation.get(animation_path)?;
        self.animations.get_mut(*animation_id)
    }

    pub fn request_animation(&mut self, animation_path: &GameAssetPath) {
        self.to_load_animations.insert(animation_path.clone());
    }
}
