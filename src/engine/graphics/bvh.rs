use crate::common::aabb::AABB;

pub struct BVH {}

impl BVH {
    pub fn construct_bvh(acceleration_data: Vec<AccelerationData>) -> Self {
        Self {}
    }
}

pub struct BVHNode {
    aabb: AABB,
    left: u32,
    right: u32,
    data_ptr: u32,
}

pub struct AccelerationData {
    aabb: AABB,
    data_ptr: u32,
}
