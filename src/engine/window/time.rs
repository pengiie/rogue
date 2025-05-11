use std::{
    ops::{Add, Sub},
    time::Duration,
};

use log::debug;
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

#[derive(Clone, Debug, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

    pub fn epoch() -> Self {
        Instant(std::time::Duration::from_millis(0))
    }

    pub fn elapsed(&self) -> Duration {
        Self::now() - *self
    }
}

impl From<std::time::SystemTime> for Instant {
    fn from(sys_time: std::time::SystemTime) -> Self {
        Self(sys_time.duration_since(std::time::UNIX_EPOCH).unwrap())
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, rhs: Duration) -> Self::Output {
        Instant(self.0 + rhs)
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        self.0.saturating_sub(rhs.0)
    }
}

pub struct Timer {
    time_dur: Duration,
    last_instant: Instant,
}

impl Timer {
    pub fn new(duration: Duration) -> Self {
        Self {
            time_dur: duration,
            last_instant: Instant::now(),
        }
    }

    pub fn try_complete(&mut self) -> bool {
        if self.is_complete() {
            self.reset();
            return true;
        }

        return false;
    }

    pub fn is_complete(&self) -> bool {
        self.last_instant + self.time_dur <= Instant::now()
    }

    /// Fast fowards this timer so it is considered complete.
    pub fn fast_forward(&mut self) {
        self.last_instant = Instant::epoch();
    }

    pub fn reset(&mut self) {
        self.last_instant = Instant::now();
    }
}

pub struct Stopwatch {
    name: String,
    creation_instant: Instant,
}

impl Stopwatch {
    pub fn new(name: impl ToString) -> Self {
        Self {
            name: name.to_string(),
            creation_instant: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.creation_instant.elapsed()
    }
}

impl Drop for Stopwatch {
    fn drop(&mut self) {
        log::info!("Stopwatch {}: Dropped in {:?}.", self.name, self.elapsed());
    }
}
