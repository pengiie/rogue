use std::time::Duration;

use crate::engine::window::time::Instant;

pub trait Animatable {
    fn get_vals_mut(&mut self) -> Vec<&mut f32>;
}

impl Animatable for f32 {
    fn get_vals_mut(&mut self) -> Vec<&mut f32> {
        vec![self]
    }
}

impl Animatable for nalgebra::Vector3<f32> {
    fn get_vals_mut<'a>(&'a mut self) -> Vec<&'a mut f32> {
        let x_ptr = std::ptr::from_mut(&mut self.x);
        let y_ptr = std::ptr::from_mut(&mut self.y);
        let z_ptr = std::ptr::from_mut(&mut self.z);
        unsafe {
            vec![
                x_ptr.as_mut().unwrap(),
                y_ptr.as_mut().unwrap(),
                z_ptr.as_mut().unwrap(),
            ]
        }
    }
}

pub struct Animation<T: Animatable> {
    length: Duration,
    start_time: Option<Instant>,
    saturated: bool,
    start_values: Option<Vec<f32>>,
    diff_values: Option<Vec<f32>>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Animatable> Animation<T> {
    pub fn new(length: Duration) -> Self {
        Self {
            length,
            start_time: None,
            saturated: false,
            start_values: None,
            diff_values: None,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn start(&mut self, mut start: T, mut end: T, length: Duration) {
        self.start_time = Some(Instant::now());
        self.saturated = false;
        let start_values = start
            .get_vals_mut()
            .into_iter()
            .map(|x| *x)
            .collect::<Vec<_>>();
        self.diff_values = Some(
            end.get_vals_mut()
                .into_iter()
                .zip(&start_values)
                .map(|(x, y)| *x - y)
                .collect::<Vec<_>>(),
        );
        self.start_values = Some(start_values);
        self.length = length;
    }

    pub fn is_animating(&self) -> bool {
        self.saturated
    }

    /// Returns true the frame the animation is finished.
    pub fn update(&mut self, val: &mut T) -> bool {
        if self.saturated {
            return false;
        }
        let Some(start_time) = self.start_time else {
            return false;
        };
        let time_diff = Instant::now() - start_time;
        if time_diff > self.length {
            self.saturated = true;
        }

        let t = (time_diff.as_secs_f32() / self.length.as_secs_f32()).clamp(0.0, 1.0);
        // Cubic ease in-out.
        let t = if t < 0.5 {
            2.0 * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powf(3.0) / 2.0
        };
        for ((val, start), diff) in val
            .get_vals_mut()
            .into_iter()
            .zip(self.start_values.as_ref().unwrap())
            .zip(self.diff_values.as_ref().unwrap())
        {
            *val = *start + *diff * t;
        }
        return self.saturated;
    }
}
