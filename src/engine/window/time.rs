use std::time::{Duration, Instant};

use rogue_macros::Resource;

use crate::engine::resource::ResMut;

#[derive(Resource)]
pub struct Time {
    delta_time: Duration,
    last_time: Instant,
    frame_count: u32,

    start_time: Instant,
}

impl Time {
    pub fn new() -> Self {
        Self {
            delta_time: Duration::ZERO,
            last_time: Instant::now(),
            frame_count: 0,

            start_time: Instant::now(),
        }
    }

    pub fn update(mut time: ResMut<Time>) {
        let curr_time = Instant::now();
        time.delta_time = curr_time - time.last_time;
        time.last_time = curr_time;
        time.frame_count += 1;
    }

    pub fn delta_time(&self) -> Duration {
        self.delta_time
    }

    pub fn start_time(&self) -> Instant {
        self.start_time
    }

    pub fn fps(&self) -> u32 {
        f32::floor(1.0 / self.delta_time.as_secs_f32()) as u32
    }

    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }
}
