use crate::common::aabb::AABB;

pub struct BVH {
    node: Option<BVHNode>,
}

impl BVH {
    pub fn new() -> Self {
        Self { node: None }
    }

    pub fn add_node(&mut self, aabb: AABB, data_ptr: u32) {
        let new_node = BVHNode {
            aabb,
            data_ptr,
            left: None,
            right: None,
        };
        if self.node.is_none() {
            self.node = Some(new_node);
            return;
        }

        todo!("Construct bvh");
    }

    pub fn remove_node(&mut self, aabb: &AABB) {}

    pub fn flatten_for_gpu(&self) -> Vec<u8> {
        let data = Vec::new();
        data
    }
}

pub type BVHNodeId = u64;
pub struct BVHNode {
    aabb: AABB,
    data_ptr: u32,
    left: Option<Box<BVHNode>>,
    right: Option<Box<BVHNode>>,
}

pub struct AccelerationData {
    aabb: AABB,
    data_ptr: u32,
}
