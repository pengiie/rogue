use std::time::Duration;

use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{
    resource::{Res, ResMut},
    window::time::Time,
};

#[derive(Resource)]
pub struct Sky {
    pub time_of_day_secs: f32,
    pub do_day_night_cycle: bool,
}

impl Sky {
    pub const SECS_PER_DAY: f32 = 24.0 * 60.0 * 60.0;
    pub const REAL_MINUTES_PER_DAY: f32 = 1.0;
    pub const REAL_SECONDS_PER_DAY: f32 = Self::REAL_MINUTES_PER_DAY * 60.0;
    pub const TIME_SCALE: f32 = Self::SECS_PER_DAY / Self::REAL_SECONDS_PER_DAY;

    pub fn new() -> Self {
        Self {
            time_of_day_secs: 60.0 * 60.0 * 12.0,
            do_day_night_cycle: false,
        }
    }

    pub fn update_time(mut sky: ResMut<Sky>, time: Res<Time>) {
        if sky.do_day_night_cycle {
            sky.time_of_day_secs += time.delta_time().as_secs_f32() * Self::TIME_SCALE;
        }
        sky.time_of_day_secs %= Self::SECS_PER_DAY;
    }

    fn sun_angle(&self) -> f32 {
        // Night-time is half of daytime cause daytime is nice :)
        // t-value of when night-time starts.
        let night_time_factor = 0.75;
        let t = self.time_of_day_secs / Self::SECS_PER_DAY;
        if t < night_time_factor {
            (t / night_time_factor) * std::f32::consts::PI
        } else {
            // Map [night_time_factor, 1] to [pi, 2 * pi]
            let night_t = (t - night_time_factor) / (1.0 - night_time_factor);
            night_t * std::f32::consts::PI + std::f32::consts::PI
        }
    }

    pub fn sun_dir(&self) -> Vector3<f32> {
        let angle = self.sun_angle();
        Vector3::new(angle.cos(), angle.sin(), 0.0)
    }
}
