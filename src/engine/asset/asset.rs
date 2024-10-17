use core::panic;
use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, VecDeque},
    future::{Future, IntoFuture},
    hash::{DefaultHasher, Hash, Hasher},
    io::Read,
    ops::Deref,
    pin::Pin,
    sync::{
        atomic::AtomicU64,
        mpsc::{channel, Receiver},
    },
    time::Duration,
};

use log::debug;
use regex::Regex;
use rogue_macros::Resource;
use wasm_bindgen::JsCast;

use crate::engine::resource::ResMut;

#[derive(Resource)]
pub struct Assets {
    // TODO: Create homogenous arrays based off of type info as the key so every asset isn't a
    // separate heap allocation.
    assets: HashMap<AssetId, AssetData>,
    queued_assets: VecDeque<QueuedAsset>,
    processing_assets: Vec<ProcessingAsset>,
    id_counter: AtomicU64,

    #[cfg(not(target_arch = "wasm32"))]
    thread_pool: rayon::ThreadPool,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            assets: HashMap::new(),
            queued_assets: VecDeque::new(),
            processing_assets: Vec::new(),
            id_counter: AtomicU64::new(0),

            #[cfg(not(target_arch = "wasm32"))]
            thread_pool: rayon::ThreadPoolBuilder::default()
                .num_threads(1)
                .build()
                .unwrap(),
        }
    }

    pub fn update_assets(mut assets: ResMut<Assets>) {
        // Move out the assets which have their futures ready. This mean their asset loader has
        // finished loading the asset.
        let mut processing_assets = Vec::new();
        std::mem::swap(&mut processing_assets, &mut assets.processing_assets);
        let (processed_assets, unprocessed_assets): (
            Vec<(ProcessingAsset, Option<ProcessingAssetResult>)>,
            Vec<(ProcessingAsset, Option<ProcessingAssetResult>)>,
        ) = processing_assets
            .into_iter()
            .map(|asset| {
                let res = asset.asset_recv.try_recv();
                (asset, res.ok())
            })
            .partition(|(_, recv_value)| recv_value.is_some());
        assets.processing_assets = unprocessed_assets
            .into_iter()
            .map(|(asset, _)| asset)
            .collect::<Vec<_>>();

        for (processing_asset, result) in processed_assets {
            let result = result.unwrap();
            match result {
                Ok(asset) => {
                    let id = processing_asset.id;
                    assets.assets.insert(
                        id,
                        AssetData {
                            data: asset,
                            is_touched: false,
                            last_hash: None,
                        },
                    );
                }
                Err(_) => todo!(),
            }
        }

        // Amount of asset that can be loaded at a time.
        const PROCESSING_THRESHOLD: u32 = 3;
        let take_count = PROCESSING_THRESHOLD - assets.processing_assets.len() as u32;
        for _ in 0..take_count {
            let Some(enqueued_asset) = assets.queued_assets.pop_front() else {
                break;
            };

            let (send, recv) = channel::<ProcessingAssetResult>();

            debug!("Enqueueing load asset!");
            let load_fut = async move {
                let asset_data = enqueued_asset.load_fut.await;

                send.send(asset_data);
                debug!("Loading asset!");
            };

            cfg_if::cfg_if! {
               if #[cfg(target_arch = "wasm32")] {
                   wasm_bindgen_futures::spawn_local(load_fut);
               } else {
                   assets.thread_pool.spawn(move || {
                       pollster::block_on(load_fut);
                   });
               }
            }

            assets.processing_assets.push(ProcessingAsset {
                id: enqueued_asset.id,
                asset_recv: recv,
            })
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn process_assets(&mut self) {}

    /// Single threaded environment since we are on wasm so we don't have access to an own thread
    /// pool. Instead we can check the status of promises from our web asset http requests.
    #[cfg(target_arch = "wasm32")]
    fn process_assets(&mut self) {}

    /// Enqueues the asset to the loading queue. Status on the asset can be queried using the
    /// returned `AssetHandle`.
    pub fn load_asset<T, N, C>(&mut self, path: AssetPath) -> AssetHandle
    where
        T: AssetLoader<N> + Send + 'static,
        N: AssetStorage + Send + 'static,
        C: AssetStorageCtor<N> + 'static,
    {
        let handle = AssetHandle {
            asset_type: std::any::TypeId::of::<T>(),
            storage_type: std::any::TypeId::of::<N>(),
            path: path.clone(),
            id: self.next_id(),
        };

        let load_fut = async move {
            let storage = C::from_path(&path);
            let contents = T::load(&storage).await;

            contents.map(|c| Box::new(c) as Box<dyn Any + Send>)
        };

        self.queued_assets.push_back(QueuedAsset {
            id: handle.id,
            load_fut: Box::pin(load_fut),
        });

        // TODO: Turn into async.
        //let asset_hash = storage.calculate_hash();
        //let asset_data = Box::new(T::load(storage));
        // let asset_info = AssetData {
        //     data: asset_data,
        //     is_touched: false,
        //     last_hash: None,
        // };
        // self.assets.insert(handle.id, asset_info);

        handle
    }

    pub fn update_asset<T, N, C>(&mut self, handle: &AssetHandle)
    where
        T: AssetLoader<N> + 'static,
        N: AssetStorage + 'static,
        C: AssetStorageCtor<N> + 'static,
    {
        // assert_eq!(
        //     handle.storage_type,
        //     std::any::TypeId::of::<AssetFile>(),
        //     "Can only update assets loaded from files."
        // );
        // assert_eq!(handle.storage_type, std::any::TypeId::of::<N>());
        // assert_eq!(handle.asset_type, std::any::TypeId::of::<T>());

        // let Some(asset_data) = self.assets.get_mut(&handle.id) else {
        //     panic!("This handle is invalid for some reason");
        // };

        // let storage = C::from_path(&handle.path);

        // let new_asset_hash = storage.calculate_hash();
        // let new_asset_data = Box::new(T::load(&storage));

        // asset_data.last_hash = new_asset_hash;
        // asset_data.data = new_asset_data;
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
        unimplemented!()
        // assert_eq!(handle.storage_type, std::any::TypeId::of::<AssetFile>());

        // let Some(asset_data) = self.assets.get(&handle.id) else {
        //     panic!("This handle is invalid for some reason");
        // };

        // let storage = AssetFile::from_path(&handle.path);

        // return storage.calculate_hash() != asset_data.last_hash;
    }

    pub fn next_id(&mut self) -> AssetId {
        self.id_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
}

struct QueuedAsset {
    id: AssetId,
    load_fut: Pin<Box<dyn ProcessingAssetFuture>>,
}

trait ProcessingAssetFuture: Future<Output = anyhow::Result<Box<dyn std::any::Any + Send>>> {}
impl<T> ProcessingAssetFuture for T where
    T: Future<Output = anyhow::Result<Box<dyn std::any::Any + Send>>>
{
}

type ProcessingAssetResult = anyhow::Result<Box<dyn std::any::Any + Send>>;
struct ProcessingAsset {
    id: AssetId,
    asset_recv: Receiver<ProcessingAssetResult>,
}

pub type AssetId = u64;

pub struct AssetData {
    data: Box<dyn std::any::Any>,
    // If the asset file has been touched in any way since the
    is_touched: bool,
    // Used to check if the asset has been modified since loading.
    last_hash: Option<u64>,
}

pub trait AssetStorageCtor<T: AssetStorage> {
    fn from_path(path: &AssetPath) -> T;
}

pub trait AssetStorage {
    fn calculate_hash(&self) -> u64;
}

#[cfg(not(target_arch = "wasm32"))]
pub trait AssetFuture<T>: Future<Output = anyhow::Result<T>> + Send
where
    T: Sized + std::any::Any,
{
}
#[cfg(not(target_arch = "wasm32"))]
impl<T, B> AssetFuture<T> for B
where
    T: Sized + std::any::Any,
    B: Future<Output = anyhow::Result<T>> + Send,
{
}

#[cfg(target_arch = "wasm32")]
pub trait AssetFuture<T>: Future<Output = anyhow::Result<T>>
where
    T: Sized + std::any::Any,
{
}
#[cfg(target_arch = "wasm32")]
impl<T, B> AssetFuture<T> for B
where
    T: Sized + std::any::Any,
    B: Future<Output = anyhow::Result<T>>,
{
}

pub trait AssetLoader<T: AssetStorage> {
    fn load(data: &T) -> impl AssetFuture<Self>
    where
        Self: Sized + std::any::Any;
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

    pub fn into_fetch_url(&self) -> String {
        "http://127.0.0.1:8080/".to_owned() + &self.into_file_path()[2..]
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        /// File handles on web are just urls to where the asset data can be loaded from.
        pub struct FileHandle(String);

        impl FileHandle {
            fn from_path(path: &AssetPath) -> Self {
                let url = path.into_fetch_url();

                Self(url)
            }

            pub fn read_contents(&self) -> impl std::future::Future<Output = String> {
                let url = self.0.clone();

                async move {
                    let mut request_opts = web_sys::RequestInit::new();
                    request_opts.method("GET");
                    request_opts.mode(web_sys::RequestMode::SameOrigin);
                    request_opts.signal(Some(&web_sys::AbortSignal::timeout_with_u32(2000)));

                    let mut request = web_sys::Request::new_with_str_and_init(&url, &request_opts).unwrap();

                    let fetch_promise = web_sys::window().unwrap().fetch_with_request(&request);
                    let result = wasm_bindgen_futures::JsFuture::from(fetch_promise).await;

                    let Ok(result) = result else {
                        panic!("Couldn't fetch url");
                    };
                    assert!(result.is_instance_of::<web_sys::Response>());
                    let response: web_sys::Response = result.dyn_into().unwrap();
                    debug!("{:?}", response);

                    if !response.ok() {
                        panic!("Couldn't get respone oopsie, responded with a {} status code", response.status());
                    }

                    let text = wasm_bindgen_futures::JsFuture::from(response.text().unwrap()).await.unwrap();

                    text.as_string().unwrap()
                }
            }

            pub fn calculate_hash(&self) -> u64 {
                0
            }
        }

    } else {
        pub struct FileHandle(String);

        impl FileHandle {
            fn from_path(path: &AssetPath) -> Self {

                Self(path.into_file_path())
            }

            pub fn read_contents(&self) -> impl std::future::Future<Output = String> {
                let path = self.0.clone();
                async move {
                    let mut file = std::fs::File::open(path).expect("couldnt open file");

                    let mut s = String::new();
                    file.read_to_string(&mut s);

                    s
                }
            }

            fn calculate_hash(&self) -> u64 {
                let mut file = std::fs::File::open(&self.0).expect("couldnt open file");
                let last_modified = file.metadata()
                    .unwrap()
                    .modified()
                    .unwrap();

                let mut hasher = DefaultHasher::new();
                last_modified.hash(&mut hasher);

                hasher.finish()
            }
        }
    }
}

pub struct AssetFile {
    path: AssetPath,
    file_handle: FileHandle,
}

impl AssetFile {
    pub fn read_contents(&self) -> impl Future<Output = String> {
        self.file_handle.read_contents()
    }
}

impl AssetStorageCtor<AssetFile> for AssetFile {
    fn from_path(path: &AssetPath) -> Self {
        Self {
            path: path.clone(),
            file_handle: FileHandle::from_path(path),
        }
    }
}

impl AssetStorage for AssetFile {
    fn calculate_hash(&self) -> u64 {
        self.file_handle.calculate_hash()
    }
}
