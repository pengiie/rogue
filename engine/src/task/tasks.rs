use std::num::NonZero;

use rogue_macros::Resource;

use crate::util;

#[derive(Resource)]
pub struct Tasks {
    thread_pool: rayon::ThreadPool,
}

impl Tasks {
    pub fn new() -> Self {
        // Reserve one thread for the render thread and another for the OS.
        const RESERVED_THREAD_COUNT: usize = 2;
        let num_threads = std::thread::available_parallelism()
            .map(|x| NonZero::new(x.get().saturating_sub(RESERVED_THREAD_COUNT).max(1)).unwrap())
            .unwrap_or_else(|err| {
                log::error!(
                    "Failed to get the available parallelism for this platform: {}",
                    err
                );
                NonZero::new(1).unwrap()
            });

        Self {
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads.get())
                .build()
                .expect("Failed to build the task thread pool, oops."),
        }
    }

    pub fn spawn_background_process(&mut self, f: impl FnOnce() + Send + 'static) {
        self.thread_pool.spawn(f);
    }

    pub fn total_thread_count(&self) -> NonZero<usize> {
        NonZero::new(self.thread_pool.current_num_threads()).unwrap()
    }
}
