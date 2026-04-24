use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    asset::asset::GameAssetPath,
    world::terrain::{
        chunk_pos::ChunkPos,
        region::{WorldChunkData, WorldRegion},
        region_pos::RegionPos,
    },
};

pub struct RegionMapDisk {
    regions_dir: GameAssetPath,
    loader_alive: Arc<AtomicBool>,

    to_load_regions_send: std::sync::mpsc::Sender<RegionPos>,
    loaded_regions_recv: std::sync::mpsc::Receiver<WorldRegion>,

    to_load_chunks_send: std::sync::mpsc::Sender<ChunkPos>,
    loaded_chunks_recv: std::sync::mpsc::Receiver<WorldChunkData>,
}

struct LoaderThreadContext {
    alive: Arc<AtomicBool>,
    to_load_regions_recv: std::sync::mpsc::Receiver<RegionPos>,
    loaded_regions_send: std::sync::mpsc::Sender<WorldRegion>,

    to_load_chunks_recv: std::sync::mpsc::Receiver<ChunkPos>,
    loaded_chunks_send: std::sync::mpsc::Sender<WorldChunkData>,
}

impl RegionMapDisk {
    pub fn new(regions_dir: GameAssetPath) -> Self {
        let loader_alive = Arc::new(AtomicBool::new(true));
        let (to_load_regions_send, to_load_regions_recv) = std::sync::mpsc::channel();
        let (loaded_regions_send, loaded_regions_recv) = std::sync::mpsc::channel();
        let (to_load_chunks_send, to_load_chunks_recv) = std::sync::mpsc::channel();
        let (loaded_chunks_send, loaded_chunks_recv) = std::sync::mpsc::channel();
        Self::spawn_loader_thread(LoaderThreadContext {
            alive: loader_alive.clone(),
            to_load_regions_recv,
            loaded_regions_send,

            to_load_chunks_recv,
            loaded_chunks_send,
        });
        Self {
            regions_dir,
            loader_alive,

            to_load_regions_send,
            loaded_regions_recv,

            to_load_chunks_send,
            loaded_chunks_recv,
        }
    }

    pub fn spawn_loader_thread(ctx: LoaderThreadContext) {
        std::thread::spawn(move || while ctx.alive.load(Ordering::Relaxed) {});
    }

    pub fn regions_dir(&self) -> &GameAssetPath {
        &self.regions_dir
    }

    pub fn load_region(region_pos: RegionPos) -> WorldRegion {
        todo!()
    }
}

impl Drop for RegionMapDisk {
    fn drop(&mut self) {
        self.loader_alive.store(false, Ordering::Relaxed);
    }
}
