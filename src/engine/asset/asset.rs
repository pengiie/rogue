use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    io::Read,
    sync::atomic::AtomicU64,
    time::Instant,
};

use log::debug;
use regex::Regex;
use rogue_macros::Resource;

use crate::engine::resource::ResMut;

#[derive(Resource)]
pub struct Assets {
    // TODO: Create homogenous arrays based off of type info as the key so every asset isn't a
    // separate heap allocation.
    assets: HashMap<AssetId, AssetData>,
    id_counter: AtomicU64,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            assets: HashMap::new(),
            id_counter: AtomicU64::new(0),
        }
    }

    pub fn update_assets(mut assets: ResMut<Assets>) {
        // Check if any assets are dirty.
        for (id, data) in &mut assets.assets {}
    }

    pub fn load_asset<T, N>(&mut self, path: AssetPath) -> AssetHandle
    where
        T: AssetLoader<N> + 'static,
        N: AssetStorage + 'static,
    {
        let storage = &N::from_path(&path);
        let handle = AssetHandle {
            asset_type: std::any::TypeId::of::<T>(),
            storage_type: std::any::TypeId::of::<N>(),
            path,
            id: self.next_id(),
        };

        // TODO: Turn into async.
        {
            let asset_hash = storage.calculate_hash();
            let asset_data = Box::new(T::load(storage));
            let asset_info = AssetData {
                data: asset_data,
                is_touched: false,
                last_hash: asset_hash,
            };
            self.assets.insert(handle.id, asset_info);
        }

        handle
    }

    pub fn update_asset<T, N>(&mut self, handle: &AssetHandle)
    where
        T: AssetLoader<N> + 'static,
        N: AssetStorage + 'static,
    {
        assert_eq!(
            handle.storage_type,
            std::any::TypeId::of::<AssetFile>(),
            "Can only update assets loaded from files."
        );
        assert_eq!(handle.storage_type, std::any::TypeId::of::<N>());
        assert_eq!(handle.asset_type, std::any::TypeId::of::<T>());

        let Some(asset_data) = self.assets.get_mut(&handle.id) else {
            panic!("This handle is invalid for some reason");
        };

        let storage = N::from_path(&handle.path);

        let new_asset_hash = storage.calculate_hash();
        let new_asset_data = Box::new(T::load(&storage));

        asset_data.last_hash = new_asset_hash;
        asset_data.data = new_asset_data;
    }

    pub fn get_asset<T: 'static>(&self, handle: &AssetHandle) -> Option<&T> {
        assert_eq!(std::any::TypeId::of::<T>(), handle.asset_type);

        self.assets.get(&handle.id).map(|asset| {
            asset.data.downcast_ref::<T>().expect(&format!(
                "Stored asset with id {} was expected to be type {:?} but was not.",
                handle.id, handle.asset_type
            ))
        })
    }

    /// Only works for assets loaded through files.
    pub fn is_asset_touched<T>(&mut self, handle: &AssetHandle) -> bool {
        assert_eq!(handle.storage_type, std::any::TypeId::of::<AssetFile>());

        let Some(asset_data) = self.assets.get(&handle.id) else {
            panic!("This handle is invalid for some reason");
        };

        let storage = AssetFile::from_path(&handle.path);

        return storage.calculate_hash() != asset_data.last_hash;
    }

    pub fn next_id(&mut self) -> AssetId {
        self.id_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

pub type AssetId = u64;

pub struct AssetData {
    data: Box<dyn std::any::Any>,
    // If the asset file has been touched in any way since the
    is_touched: bool,
    // Used to check if the asset has been modified since loading.
    last_hash: u64,
}

pub trait AssetStorage {
    fn from_path(path: &AssetPath) -> Self;
    fn calculate_hash(&self) -> u64;
}

pub trait AssetLoader<T: AssetStorage> {
    fn load(data: &T) -> Self;
}

#[derive(Debug, Clone)]
pub struct AssetHandle {
    asset_type: std::any::TypeId,
    storage_type: std::any::TypeId,
    path: AssetPath,
    id: AssetId,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetPath {
    path: String,
}

// In the form of module::module::asset

impl AssetPath {
    pub fn new(path: String) -> Self {
        let path_regex: Regex = Regex::new(r"^\w+(::\w+)*$").unwrap();
        assert!(path_regex.is_match(&path));

        Self { path }
    }

    pub fn into_file_path(&self) -> String {
        let parts = self.path.split("::").enumerate().collect::<Vec<_>>();
        let extension_index = parts.len() - 1;

        let mut path = "./assets".to_owned();
        for (i, part) in parts {
            if i == extension_index {
                path.push('.');
            } else {
                path.push('/');
            }
            path.push_str(part);
        }

        path
    }
}

pub struct AssetFile {
    file_handle: RefCell<std::fs::File>,
}

impl AssetFile {
    pub fn read_contents(&self) -> String {
        let mut s = String::new();
        self.file_handle.borrow_mut().read_to_string(&mut s);

        s
    }
}

impl AssetStorage for AssetFile {
    fn from_path(path: &AssetPath) -> Self {
        let file = std::fs::File::open(path.into_file_path()).expect("couldnt open file");
        Self {
            file_handle: RefCell::new(file),
        }
    }

    fn calculate_hash(&self) -> u64 {
        let last_modified = self
            .file_handle
            .borrow()
            .metadata()
            .unwrap()
            .modified()
            .unwrap();

        let mut hasher = DefaultHasher::new();
        last_modified.hash(&mut hasher);

        hasher.finish()
    }
}
