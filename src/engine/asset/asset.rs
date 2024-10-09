use std::{collections::HashMap, sync::atomic::AtomicU64};

use regex::Regex;
use rogue_macros::Resource;

use crate::engine::resource::ResMut;

#[derive(Resource)]
pub struct Assets {
    // TODO: Create homogenous arrays based off of type info as the key so every asset isn't a
    // separate heap allocation.
    assets: HashMap<AssetId, Box<dyn std::any::Any>>,
    id_counter: AtomicU64,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            assets: HashMap::new(),
            id_counter: AtomicU64::new(0),
        }
    }

    pub fn update_process_assets(assets: ResMut<Assets>) {
        todo!()
    }

    pub fn load_asset<T, N>(&mut self, path: impl Into<AssetPath>) -> AssetHandle
    where
        T: AssetLoader<N> + 'static,
        N: AssetStorage + 'static,
    {
        // TODO: load asynchronously.
        let handle = AssetHandle {
            asset_type: std::any::TypeId::of::<T>(),
            src_type: std::any::TypeId::of::<N>(),
            path: path.into(),
            id: self.next_id(),
        };
        let asset = Box::new(T::load(&N::from_path(&handle.path)));
        self.assets.insert(handle.id, asset);

        handle
    }

    pub fn get_asset<T: 'static>(&self, handle: &AssetHandle) -> Option<&T> {
        assert_eq!(std::any::TypeId::of::<T>(), handle.asset_type);

        self.assets.get(&handle.id).map(|asset| {
            asset.downcast_ref::<T>().expect(&format!(
                "Stored asset with id {} was expected to be type {:?} but was not.",
                handle.id, handle.asset_type
            ))
        })
    }

    pub fn next_id(&mut self) -> AssetId {
        self.id_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

pub type AssetId = u64;

pub trait AssetStorage {
    fn from_path(path: &AssetPath) -> Self;
}

pub trait AssetLoader<T: AssetStorage> {
    fn load(data: &T) -> Self;
}

pub struct AssetHandle {
    asset_type: std::any::TypeId,
    src_type: std::any::TypeId,
    path: AssetPath,
    id: AssetId,
}

pub struct AssetPath {
    path: String,
}

// In the form of module::module::asset

impl AssetPath {
    pub fn new(path: String) -> Self {
        let path_regex: Regex = Regex::new(r"^\w+(::\w+)*").unwrap();
        assert!(path_regex.is_match(&path));

        Self { path }
    }
}
