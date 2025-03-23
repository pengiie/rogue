use log::debug;
use nalgebra::Vector3;

use super::aabb::AABB;

#[derive(Clone, Debug)]
pub struct Ray {
    pub origin: Vector3<f32>,
    pub dir: Vector3<f32>,
    pub inv_dir: Vector3<f32>,
}

impl Ray {
    pub fn new(origin: Vector3<f32>, dir: Vector3<f32>) -> Self {
        Self {
            origin,
            dir,
            inv_dir: dir.map(|x| 1.0 / x),
        }
    }

    pub fn begin_dda(&self, aabb: &AABB, bounds: Vector3<u32>) -> RayDDA {
        RayDDA::new(self, aabb, bounds)
    }

    pub fn advance(&mut self, t: f32) {
        self.origin = self.origin + self.dir * t;
    }

    pub fn intersect_point(&self, point: Vector3<f32>) -> Vector3<f32> {
        return self.inv_dir.component_mul(&(point - self.origin));
    }

    /// Returns the t-value to advance to the AABB, only in the positive direction.
    pub fn intersect_aabb(&self, aabb: &AABB) -> Option<f32> {
        let t0 = self.intersect_point(aabb.min);
        let t1 = self.intersect_point(aabb.max);
        let t_min = t0.zip_map(&t1, |x, y| x.min(y));
        let t_max = t0.zip_map(&t1, |x, y| x.max(y));

        let t_enter = t_min.max().max(0.0);
        let t_exit = t_max.min();

        return (t_exit > t_enter).then_some(t_enter);
    }
}

pub struct RayDDA {
    curr_grid: Vector3<i32>,
    unit_grid: Vector3<i32>,
    curr_t: Vector3<f32>,
    unit_t: Vector3<f32>,
    bounds: Vector3<u32>,
}

impl RayDDA {
    pub fn new(ray: &Ray, aabb: &AABB, bounds: Vector3<u32>) -> Self {
        // assert!(
        //     ray.origin.x - aabb.min.x < 0.001
        //         && ray.origin.y - aabb.min.y < 0.001
        //         && ray.origin.z - aabb.min.z < 0.001
        //         && ray.origin.x - aabb.max.x < 0.001
        //         && ray.origin.y - aabb.max.y < 0.001
        //         && ray.origin.z - aabb.max.z < 0.001,
        //     "To DDA with a ray, the ray must be advanced to the aabb"
        // );
        let local_pos = ray.origin - aabb.min;
        let norm_pos = local_pos.zip_map(&aabb.side_length(), |x, y| (x / y).clamp(0.0, 0.9999));
        // Our scaled position from [0, bounds).
        let dda_pos = norm_pos.component_mul(&bounds.cast::<f32>());
        let curr_grid = dda_pos.map(|x| x.floor() as i32);
        let unit_grid = ray.dir.map(|x| x.signum() as i32);
        let next_point = curr_grid.cast::<f32>() + (unit_grid.cast::<f32>() * 0.5).add_scalar(0.5);
        let curr_t = ray.inv_dir.component_mul(&(next_point - dda_pos)).map(|x| {
            if x.is_infinite() {
                1000000.00
            } else {
                x
            }
        });
        let unit_t = ray
            .inv_dir
            .map(|x| if x.is_infinite() { 0.0 } else { x.abs() });
        debug!(
            "init with curr_t {:?}, unit_t {:?}, dda_pos {:?}",
            curr_t, unit_t, dda_pos
        );

        Self {
            curr_grid,
            unit_grid,
            curr_t,
            unit_t,
            bounds,
        }
    }

    pub fn in_bounds(&self) -> bool {
        return self.curr_grid.x >= 0
            && self.curr_grid.y >= 0
            && self.curr_grid.z >= 0
            && self.curr_grid.x < self.bounds.x as i32
            && self.curr_grid.y < self.bounds.y as i32
            && self.curr_grid.z < self.bounds.z as i32;
    }

    pub fn curr_grid_pos(&self) -> Vector3<i32> {
        self.curr_grid
    }

    pub fn step(&mut self) {
        let min_t = self.curr_t.min();
        let mask = self.curr_t.map(|x| if x == min_t { 1 } else { 0 });
        self.curr_grid += mask.component_mul(&self.unit_grid);
        self.curr_t += mask.cast::<f32>().component_mul(&self.unit_t);
    }
}
