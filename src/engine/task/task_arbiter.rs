use std::num::NonZero;

use rogue_macros::Resource;

#[derive(Resource)]
pub struct TaskArbiter {
    thread_pool: rayon::ThreadPool,
}

impl TaskArbiter {
    pub fn new() -> Self {
        let num_threads = std::thread::available_parallelism().unwrap_or_else(|err| {
            log::error!(
                "Failed to get the available parallelism for this platform: {}",
                err
            );
            NonZero::new(2).unwrap()
        });

        Self {
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads.get())
                .build()
                .expect("Failed to build the task thread pool, oops."),
        }
    }
}
