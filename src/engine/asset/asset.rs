use core::panic;
use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    future::{Future, IntoFuture},
    hash::{DefaultHasher, Hash, Hasher},
    io::{Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    pin::Pin,
    str::FromStr,
    sync::{
        atomic::AtomicU64,
        mpsc::{channel, Receiver},
    },
    time::Duration,
};

use log::{debug, info};
use regex::Regex;
use rogue_macros::Resource;

use crate::engine::{
    resource::ResMut,
    window::time::{Instant, Timer},
};

#[derive(Resource)]
pub struct Assets {
    // TODO: Create homogenous arrays based off of type info as the key so every asset isn't a
    // separate heap allocation.
    path_id_map: HashMap<AssetPath, AssetId>,
    saved_assets_path: HashMap<AssetId, AssetPath>,
    assets: HashMap<AssetId, AssetData>,
    asset_statuses: HashMap<AssetId, AssetStatus>,

    queued_assets: VecDeque<QueuedAsset>,
    processing_assets: Vec<ProcessingAsset>,
    currently_loading_assets: HashSet<AssetId>,
    currently_saving_assets: HashSet<AssetId>,
    id_counter: u64,

    assets_dir_touched: bool,
    assets_dir_modified: Option<Instant>,
    assets_dir_check_timer: Timer,

    thread_pool: rayon::ThreadPool,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            path_id_map: HashMap::new(),
            saved_assets_path: HashMap::new(),
            assets: HashMap::new(),
            asset_statuses: HashMap::new(),

            queued_assets: VecDeque::new(),
            processing_assets: Vec::new(),

            currently_loading_assets: HashSet::new(),
            currently_saving_assets: HashSet::new(),
            id_counter: 0,

            assets_dir_touched: false,
            assets_dir_modified: None,
            assets_dir_check_timer: Timer::new(Duration::from_millis(100)),

            thread_pool: rayon::ThreadPoolBuilder::default()
                .num_threads(1)
                .build()
                .unwrap(),
        }
    }

    pub fn check_assets_dir_for_updates(&mut self) {
        let assets_path = PathBuf::from("./assets").canonicalize().unwrap();

        let mut dir_metadata = std::fs::metadata(&assets_path).expect("Failed to read assets dir.");
        assert!(dir_metadata.is_dir());

        let mut last_modified = None;
        let mut dirs_to_process = vec![assets_path.clone()];
        while let Some(curr_dir) = dirs_to_process.pop() {
            let curr_dir_children =
                std::fs::read_dir(&curr_dir).expect("Failed to read assets dir.");
            for child in curr_dir_children {
                if let Ok(child) = child {
                    let metadata = child.metadata().unwrap();
                    if metadata.is_dir() {
                        dirs_to_process.push(child.path());
                    } else if metadata.is_file() {
                        if last_modified.is_none()
                            || last_modified.unwrap() < metadata.modified().unwrap()
                        {
                            last_modified = Some(metadata.modified().unwrap());
                        }
                    } else {
                        panic!("Symlinks in the asset dir are not supported.");
                    }
                }
            }
        }
        let last_modified = last_modified
            .expect("Asset directory should not be empty.")
            .into();
        if self.assets_dir_modified.is_none() || self.assets_dir_modified.unwrap() < last_modified {
            if self.assets_dir_modified.is_some() {
                self.assets_dir_touched = true;
            }
            self.assets_dir_modified = Some(last_modified);
        }
    }
    pub fn update_assets(mut assets: ResMut<Assets>) {
        assets.update_assets_impl();
    }

    fn update_assets_impl(&mut self) {
        let assets = self;
        assets.assets_dir_touched = false;

        if assets.assets_dir_check_timer.try_complete() {
            assets.check_assets_dir_for_updates();
        }

        // Process finished tasks from the thread pool.
        let mut finished_ids = Vec::new();
        'asset_loop: for asset in &assets.processing_assets {
            match asset {
                ProcessingAsset::Load { id, asset_recv } => {
                    match asset_recv.try_recv() {
                        Ok(res) => match res {
                            Ok(ProcessedAsset { data, path, hash }) => {
                                assets.assets.insert(
                                    *id,
                                    AssetData {
                                        data,
                                        path,
                                        is_touched: false,
                                        last_hash: Some(hash),
                                        dependencies: 0,
                                    },
                                );
                                assets.asset_statuses.insert(*id, AssetStatus::Loaded);
                            }
                            Err(err) => match err {
                                AssetLoadError::NotFound { path } => {
                                    assets.asset_statuses.insert(*id, AssetStatus::NotFound);
                                }
                                AssetLoadError::Other(err) => {
                                    log::error!("Error loading asset: {}", err.to_string());
                                    log::error!("Backtrace: {}", err.backtrace().to_string());
                                    assets.asset_statuses.insert(*id, AssetStatus::Error(err));
                                }
                            },
                        },
                        Err(err) => {
                            match err {
                                std::sync::mpsc::TryRecvError::Empty => {
                                    continue 'asset_loop;
                                }
                                std::sync::mpsc::TryRecvError::Disconnected => {
                                    log::error!("Error with asset thread disconnection while loading asset {}", id)
                                }
                            }
                        }
                    }
                    finished_ids.push(*id);
                }
                ProcessingAsset::Save { id, asset_recv } => {
                    match asset_recv.try_recv() {
                        Ok(res) => match res {
                            _ => {
                                assets.asset_statuses.insert(*id, AssetStatus::Saved);
                                log::debug!(
                                    "Saved asset to {:?}.",
                                    assets.saved_assets_path.get(id).unwrap()
                                );
                            }
                            Err(err) => {
                                log::error!("Error saving asset: {}", err.to_string());
                                assets.asset_statuses.insert(*id, AssetStatus::Error(err));
                            }
                        },
                        Err(err) => {
                            match err {
                                std::sync::mpsc::TryRecvError::Empty => {
                                    continue 'asset_loop;
                                }
                                std::sync::mpsc::TryRecvError::Disconnected => {
                                    log::error!("Error with asset thread disconnection while saving asset {}", id)
                                }
                            }
                        }
                    }
                    finished_ids.push(*id);
                }
            }
        }
        // Remove the finished assets from the statuses.
        for finished_id in finished_ids {
            assets.currently_saving_assets.remove(&finished_id);
            assets.currently_loading_assets.remove(&finished_id);
            assets
                .processing_assets
                .retain(|processing_asset| match processing_asset {
                    ProcessingAsset::Load { id, asset_recv } => finished_id != *id,
                    ProcessingAsset::Save { id, asset_recv } => finished_id != *id,
                });
        }

        // Push queued asset requests to the threads to get processed.
        // Amount of asset that can be loaded at a time.
        const PROCESSING_THRESHOLD: u32 = 3;
        let take_count = PROCESSING_THRESHOLD - assets.processing_assets.len() as u32;
        for _ in 0..take_count {
            let Some(enqueued_asset) = assets.queued_assets.pop_front() else {
                break;
            };

            match enqueued_asset {
                QueuedAsset::Load { id, load_fut } => {
                    let (send, recv) = channel::<ProcessingAssetLoadResult>();

                    assets.thread_pool.spawn(move || {
                        let asset = pollster::block_on(load_fut);
                        send.send(asset);
                    });

                    assets.processing_assets.push(ProcessingAsset::Load {
                        id,
                        asset_recv: recv,
                    });
                }
                QueuedAsset::Save { id, save_fut } => {
                    let (send, recv) = channel::<anyhow::Result<()>>();

                    assets.thread_pool.spawn(move || {
                        let save_res = pollster::block_on(save_fut);
                        send.send(save_res);
                    });

                    assets.processing_assets.push(ProcessingAsset::Save {
                        id,
                        asset_recv: recv,
                    });
                }
            }
        }
    }

    pub fn load_asset_sync<T>(path: AssetPath) -> std::result::Result<T, AssetLoadError>
    where
        T: AssetLoader + 'static,
    {
        let storage = AssetFile::from_path(&path);
        T::load(&storage).map_err(|err| match err {
            AssetLoadError::NotFound { .. } => AssetLoadError::NotFound { path: Some(path) },
            AssetLoadError::Other(e) => AssetLoadError::Other(e),
        })
    }

    /// Enqueues the asset to the loading queue. Status on the asset can be queried using the
    /// returned `AssetHandle`.
    pub fn load_asset<T>(&mut self, path: AssetPath) -> AssetHandle
    where
        T: AssetLoader + Send + 'static,
    {
        let handle = AssetHandle {
            asset_type: std::any::TypeId::of::<T>(),
            path: path.clone(),
            id: self.next_id(),
        };

        let load_fut = async move {
            let storage = AssetFile::from_path(&path);
            let hash = storage.calculate_hash();
            let contents = T::load(&storage);

            match contents {
                Ok(c) => Ok(ProcessedAsset {
                    data: Box::new(c) as Box<dyn Any + Send>,
                    hash,
                    path: path.clone(),
                }),
                Err(err) => Err(match err {
                    AssetLoadError::NotFound { .. } => {
                        AssetLoadError::NotFound { path: Some(path) }
                    }
                    AssetLoadError::Other(e) => AssetLoadError::Other(e),
                }),
            }
        };

        let pin_box = Box::pin(load_fut);

        self.currently_loading_assets.insert(handle.id);
        self.queued_assets.push_back(QueuedAsset::Load {
            id: handle.id,
            load_fut: pin_box,
        });
        self.asset_statuses
            .insert(handle.id, AssetStatus::InProgress);

        handle
    }

    pub fn save_asset<T>(&mut self, path: AssetPath, asset: T) -> AssetHandle
    where
        T: AssetSaver + Send + 'static,
    {
        let handle = AssetHandle {
            asset_type: std::any::TypeId::of::<T>(),
            path: path.clone(),
            id: self.next_id(),
        };

        let save_fut = async move {
            let storage = AssetFile::from_path(&path);
            return T::save(&asset, &storage);
        };
        self.currently_saving_assets.insert(handle.id);
        self.queued_assets.push_back(QueuedAsset::Save {
            id: handle.id,
            save_fut: Box::pin(save_fut),
        });
        self.asset_statuses
            .insert(handle.id, AssetStatus::InProgress);
        self.saved_assets_path
            .insert(handle.id, handle.path.clone());

        handle
    }

    pub fn update_asset<T, N, C>(&mut self, handle: &AssetHandle)
    where
        T: AssetLoader + Send + 'static,
    {
        let curr_data = self
            .assets
            .get(&handle.id)
            .expect("update_asset was calling with an invalid AssetHandle");

        let path_clone = curr_data.path.clone();
        let load_fut = async move {
            let storage = AssetFile::from_path(&path_clone);
            let hash = storage.calculate_hash();
            let contents = T::load(&storage);

            match contents {
                Ok(c) => Ok(ProcessedAsset {
                    data: Box::new(c) as Box<dyn Any + Send>,
                    hash,
                    path: path_clone.clone(),
                }),
                Err(err) => Err(match err {
                    AssetLoadError::NotFound { path } => AssetLoadError::NotFound {
                        path: Some(path_clone),
                    },
                    AssetLoadError::Other(e) => AssetLoadError::Other(e),
                }),
            }
        };

        self.currently_loading_assets.insert(handle.id);
        self.queued_assets.push_back(QueuedAsset::Load {
            id: handle.id,
            load_fut: Box::pin(load_fut),
        });
    }

    pub fn get_asset_status(&self, handle: &AssetHandle) -> &AssetStatus {
        self.asset_statuses
            .get(&handle.id)
            .expect(&format!("Got an invalid asset handle: {:?}", handle))
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

    pub fn take_asset<T: 'static>(&mut self, handle: &AssetHandle) -> Option<Box<T>> {
        assert_eq!(std::any::TypeId::of::<T>(), handle.asset_type);

        self.assets.remove(&handle.id).map(|asset| {
            asset.data.downcast::<T>().expect(&format!(
                "Stored asset with id {} was expected to be type {:?} but was not.",
                handle.id, handle.asset_type
            ))
        })
    }

    /// Loading refers to both loading or saving.
    pub fn is_asset_loading(&self, handle: &AssetHandle) -> bool {
        self.currently_loading_assets.contains(&handle.id)
            || self.currently_saving_assets.contains(&handle.id)
    }

    pub fn is_assets_dir_modified(&self) -> bool {
        self.assets_dir_touched
    }

    /// Only works for assets loaded through files currently, returns true if the asset was updated.
    pub fn is_asset_touched(&mut self, handle: &AssetHandle) -> bool {
        let Some(asset_data) = self.assets.get_mut(&handle.id) else {
            panic!("This handle is invalid for some reason");
        };

        if asset_data.is_touched {
            return true;
        }

        let storage = AssetFile::from_path(&handle.path);
        let current_hash = storage.calculate_hash();
        if asset_data.last_hash.is_some() && current_hash != asset_data.last_hash.unwrap() {
            asset_data.last_hash = Some(current_hash);
            asset_data.is_touched = true;
            return true;
        }

        return false;
    }

    pub fn next_id(&mut self) -> AssetId {
        let id = self.id_counter;
        self.id_counter += 1;
        return id;
    }

    pub fn get_asset_handle<T: 'static>(&self, asset_path: &AssetPath) -> Option<AssetHandle> {
        self.path_id_map.get(asset_path).map(|id| AssetHandle {
            asset_type: std::any::TypeId::of::<T>(),
            path: asset_path.clone(),
            id: *id,
        })
    }

    pub fn wait_until_all_saved(&mut self) {
        while !self.currently_saving_assets.is_empty() {
            self.update_assets_impl();
        }
    }
}

pub enum AssetStatus {
    // Still loading or saving.
    InProgress,
    // Saved successfully.
    Saved,
    // Loaded successfully.
    Loaded,
    // Asset path could not be found.
    NotFound,
    // Asset errored while loading.
    Error(anyhow::Error),
}

impl AssetStatus {
    pub fn is_saved(&self) -> bool {
        match &self {
            Self::Saved => true,
            _ => false,
        }
    }
}

enum QueuedAsset {
    Load {
        id: AssetId,
        load_fut: Pin<Box<dyn ProcessingAssetLoadFuture>>,
    },
    Save {
        id: AssetId,
        save_fut: Pin<Box<dyn ProcessingAssetSaveFuture>>,
    },
}

type AssetHash = u64;

trait ProcessingAssetLoadFuture: Future<Output = ProcessingAssetLoadResult> + Send {}
impl<T> ProcessingAssetLoadFuture for T where T: Future<Output = ProcessingAssetLoadResult> + Send {}

trait ProcessingAssetSaveFuture: Future<Output = anyhow::Result<()>> + Send {}
impl<T> ProcessingAssetSaveFuture for T where T: Future<Output = anyhow::Result<()>> + Send {}

struct ProcessedAsset {
    data: Box<dyn std::any::Any + Send>,
    path: AssetPath,
    hash: AssetHash,
}

#[derive(Debug)]
pub enum AssetLoadError {
    NotFound { path: Option<AssetPath> },
    Other(anyhow::Error),
}

impl From<anyhow::Error> for AssetLoadError {
    fn from(value: anyhow::Error) -> Self {
        AssetLoadError::Other(value)
    }
}

impl From<std::io::Error> for AssetLoadError {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::NotFound => AssetLoadError::NotFound { path: None },
            _ => AssetLoadError::Other(anyhow::format_err!(value)),
        }
    }
}

impl std::fmt::Display for AssetLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetLoadError::NotFound { path } => f.write_str(&format!(
                "Asset at {:?} not found.",
                path.as_ref().map_or("", |p| p.path_str())
            )),
            AssetLoadError::Other(err) => err.fmt(f),
        }
    }
}

type ProcessingAssetLoadResult = std::result::Result<ProcessedAsset, AssetLoadError>;
enum ProcessingAsset {
    Load {
        id: AssetId,
        asset_recv: Receiver<ProcessingAssetLoadResult>,
    },
    Save {
        id: AssetId,
        asset_recv: Receiver<anyhow::Result<()>>,
    },
}

pub type AssetId = u64;

pub struct AssetData {
    data: Box<dyn std::any::Any>,
    path: AssetPath,
    // If the asset file has been touched in any way since the
    is_touched: bool,
    // Used to check if the asset has been modified since loading.
    last_hash: Option<u64>,
    dependencies: u32,
}

pub trait AssetStorageCtor<T: AssetStorage> {
    fn from_path(path: &AssetPath) -> T;
}

pub trait AssetStorage {
    fn calculate_hash(&self) -> u64;
}

pub trait AssetLoadFuture<T>: Future<Output = anyhow::Result<T>> + Send
where
    T: Sized + std::any::Any,
{
}

impl<T, B> AssetLoadFuture<T> for B
where
    T: Sized + std::any::Any,
    B: Future<Output = anyhow::Result<T>> + Send,
{
}

pub trait AssetLoader {
    fn load(data: &AssetFile) -> std::result::Result<Self, AssetLoadError>
    where
        Self: Sized + std::any::Any;
}

pub trait AssetSaver {
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized;
}

impl<T> AssetLoader for T
where
    T: serde::de::DeserializeOwned,
{
    fn load(data: &AssetFile) -> std::result::Result<Self, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        match data.path.extension() {
            "json" => match data.read_contents() {
                Ok(contents) => serde_json::from_str::<T>(&contents).map_err(|_| {
                    AssetLoadError::Other(anyhow::anyhow!("Failed to deserialize file."))
                }),
                Err(err) => match err.kind() {
                    std::io::ErrorKind::NotFound => Err(AssetLoadError::NotFound { path: None }),
                    _ => Err(AssetLoadError::Other(anyhow::anyhow!(err.to_string()))),
                },
            },
            s => todo!("Support extension .{}", s),
        }
    }
}

impl<T> AssetSaver for T
where
    T: serde::Serialize,
{
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        match out_file.path.extension() {
            "json" => match out_file
                .write_contents(serde_json::to_string_pretty(data).expect("Failed to serialize."))
            {
                Ok(()) => Ok(()),
                Err(err) => match err.kind() {
                    _ => Err(anyhow::anyhow!(err.to_string())),
                },
            },
            s => todo!("Support extension .{}", s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetHandle {
    asset_type: std::any::TypeId,
    path: AssetPath,
    id: AssetId,
}

impl AssetHandle {
    pub fn asset_path(&self) -> &AssetPath {
        &self.path
    }
}

// Hash only hashes the asset id.
impl std::hash::Hash for AssetHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, Hash,
)]
pub struct AssetPath {
    // Only valid for binary and project assets.
    // In the form of (binary|project)::(dirs::)*file_name::extension
    pub asset_path: Option<String>,
    // Change maybe so we can support a giant asset file.
    path: PathBuf,
}

impl AssetPath {
    fn validate_path(path: &str) {
        let path_regex: Regex = Regex::new(r"^[a-zA-Z0-9_-]+(::[a-zA-Z0-9_-]+)*$").unwrap();
        assert!(
            path_regex.is_match(&path),
            "Path {} failed to pass path validation.",
            path
        );
    }

    pub fn new(path: PathBuf) -> Self {
        Self {
            asset_path: None,
            path,
        }
    }

    /// Searches in the editor/runtime required assets that are project independent.
    pub fn new_binary_dir(path: impl ToString) -> Self {
        let path = format!("{}", path.to_string());
        Self::validate_path(&path);
        Self {
            asset_path: Some(path.clone()),
            path: Self::into_file_path(&path, Path::new("./assets/")),
        }
    }

    /// Searches in the projects assets directory for the editor and runtime.
    pub fn new_project_dir(project_dir: PathBuf, path: String) -> Self {
        let path = format!("{}", path.clone());
        Self::validate_path(&path);
        Self {
            asset_path: Some(path.clone()),
            path: Self::into_file_path(&path, &project_dir.join("assets")),
        }
    }

    pub fn from_project_dir_path(project_dir: &Path, path: &Path) -> Self {
        let sub_path = path
            .strip_prefix(project_dir.join("assets"))
            .expect(&format!(
                "\"{}\" must be a prefix of path \"{}\".",
                project_dir.to_string_lossy(),
                path.to_string_lossy()
            ));

        let mut s = String::new();
        let last_i = sub_path.components().count() - 1;
        for (i, p) in sub_path.iter().enumerate() {
            let p = p.to_string_lossy().to_string();
            if p.contains(".") {
                let parts = p.split(".").collect::<Vec<_>>();
                s.push_str(parts[0]);
                s.push_str("::");
                s.push_str(parts[1]);
            } else {
                s.push_str(&p);
            }
            if i < last_i {
                s.push_str("::");
            }
        }
        Self::validate_path(&s);
        Self {
            asset_path: Some(s),
            path: path.to_owned(),
        }
    }

    pub fn new_user_dir(sub_path: impl ToString) -> Self {
        let sub_path = format!("{}", sub_path.to_string());
        let sub_path = Self::into_file_path(&sub_path.to_string(), Path::new("./rogue_user_data"));
        let path = match std::env::consts::OS {
            "linux" => std::env::var("HOME")
                .map(|home_dir| {
                    PathBuf::from_str(&home_dir)
                        .unwrap()
                        .join(".rogue-test-data")
                })
                .expect("Can't get home directory."),
            _ => unimplemented!("Unsupport OS."),
        };

        Self {
            // TODO: I don't trust myself with the home directory yet.
            //path: path.join(sub_path).to_str().unwrap().to_owned(),
            asset_path: None,
            path: sub_path,
        }
    }

    pub fn into_file_path(path: &str, prefix: &std::path::Path) -> PathBuf {
        assert!(path.len() >= 3);
        let parts = path.split("::").enumerate().collect::<Vec<_>>();
        let extension_index = parts.len() - 1;

        let mut path = prefix.to_owned();
        for (i, part) in &parts {
            if *i == extension_index {
                path.set_extension(part);
            } else {
                path = path.join(part);
            }
        }

        path
    }

    pub fn into_fetch_url(&self) -> String {
        "TODO!!!".to_owned()
    }

    pub fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub fn path_str(&self) -> &str {
        self.path.as_path().to_str().unwrap()
    }

    pub fn extension(&self) -> &str {
        self.path.extension().unwrap().to_str().unwrap()
    }
}
pub struct FileHandle(String);

impl FileHandle {
    fn from_path(path: &AssetPath) -> Self {
        Self(path.path.clone().to_str().unwrap().to_owned())
    }

    pub fn read_contents(&self) -> std::io::Result<String> {
        let path = self.0.clone();
        std::fs::read_to_string(std::path::Path::new(&path))
    }

    pub fn write_contents(&self, contents: String) -> std::io::Result<()> {
        let bytes = contents.as_bytes();

        let path = self.0.clone();
        let mut file = self.write_file()?;
        file.set_len(bytes.len() as u64);
        file.write_all(bytes)?;

        Ok(())
    }

    pub fn read_file(&self) -> std::io::Result<std::fs::File> {
        std::fs::File::open(&self.0)
    }

    pub fn write_file(&self) -> std::io::Result<std::fs::File> {
        let path = PathBuf::from_str(&self.0).unwrap();

        std::fs::create_dir_all(path.parent().unwrap()).expect(&format!(
            "Couldn't create parent dirs for file {:?}.",
            &path
        ));
        std::fs::File::create(&path)
    }

    fn calculate_hash(&self) -> u64 {
        let Ok(mut file) = self.read_file() else {
            return 0;
        };
        let last_modified = file.metadata().unwrap().modified().unwrap();

        let mut hasher = DefaultHasher::new();
        last_modified.hash(&mut hasher);

        hasher.finish()
    }
}

pub struct AssetFile {
    path: AssetPath,
    file_handle: FileHandle,
}

impl AssetFile {
    fn from_path(path: &AssetPath) -> Self {
        Self {
            path: path.clone(),
            file_handle: FileHandle::from_path(path),
        }
    }

    pub fn extension(&self) -> &str {
        self.path.extension()
    }

    pub fn read_contents(&self) -> std::io::Result<String> {
        self.file_handle.read_contents()
    }

    pub fn read_file(&self) -> std::io::Result<std::fs::File> {
        self.file_handle.read_file()
    }

    pub fn write_file(&self) -> std::fs::File {
        self.file_handle.write_file().unwrap()
    }

    pub fn write_contents(&self, contents: String) -> std::io::Result<()> {
        self.file_handle.write_contents(contents)
    }

    fn calculate_hash(&self) -> u64 {
        self.file_handle.calculate_hash()
    }
}
