use std::time::Duration;

use crate::window::time::Instant;

pub struct InputBuffer {
    timings: Vec<Option<Instant>>,
    did_input: bool,
    index: usize,
}

impl InputBuffer {
    pub fn new(buffer_size: usize) -> Self {
        Self {
            timings: vec![const { None }; buffer_size],
            did_input: false,
            index: 0,
        }
    }

    pub fn update(&mut self, did_input: bool) {
        self.did_input = did_input;
        if !did_input {
            return;
        }

        let now = Instant::now();
        self.timings[self.index] = Some(now);
        self.index = (self.index + 1) % self.timings.len();
    }

    pub fn did_double_input(&self, threshold: Duration) -> bool {
        if !self.did_input {
            return false;
        }

        let curr_index = (self.index + self.timings.len() - 1) % self.timings.len();
        let prev_index = (self.index + self.timings.len() - 2) % self.timings.len();
        if let (Some(curr), Some(prev)) = (self.timings[curr_index], self.timings[prev_index]) {
            return curr - prev < threshold;
        }
        return false;
    }
}
