use std::{ops::Sub, time::Duration};

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

#[derive(Clone, Debug, Copy)]
pub struct Instant(std::time::Duration);

impl Instant {
    pub fn now() -> Self {
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                Instant(
                    std::time::Duration::from_secs_f64(
                        web_sys::window()
                            .unwrap()
                            .performance()
                            .expect("Can't get Performance api to calculate frame times.")
                            .now() / 1000.0))
            } else {
                Instant(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap())
            }
        }
    }

    pub fn elapsed(&self) -> Duration {
        Self::now() - *self
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        self.0 - rhs.0
    }
}
