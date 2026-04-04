use std::{any::TypeId, collections::HashMap};

use uuid::Uuid;

use crate::common::dyn_vec::TypeInfo;
/// A standalone game entity with all of its component data, essentially a prefab.
/// Any references to colliders or voxel models are also specific to this entity instance.
pub struct WorldGameEntityAsset {
    pub name: String,
    pub uuid: Uuid,
    pub parent: Option<Uuid>,
    pub children: Vec<Uuid>,
    pub components: HashMap<TypeId, WorldGameComponentAsset>,
}

pub struct WorldGameComponentAsset {
    type_info: TypeInfo,
    data: *mut u8,
}

impl WorldGameComponentAsset {
    pub unsafe fn new(type_info: TypeInfo, data: *mut u8) -> Self {
        Self { type_info, data }
    }

    pub fn take_data(&mut self) -> *mut u8 {
        let ptr = self.data;
        assert!(!self.data.is_null(), "Data is already taken.");
        self.data = std::ptr::null_mut();
        return ptr;
    }

    pub fn type_info(&self) -> &TypeInfo {
        &self.type_info
    }
}

impl Drop for WorldGameComponentAsset {
    fn drop(&mut self) {
        if !self.data.is_null() {
            // Safety: We check for null, and when ownership of the component data is transferred,
            // we set the pointer to null.
            unsafe { std::alloc::dealloc(self.data, self.type_info.layout(1)) };
        }
    }
}

impl WorldGameEntityAsset {}
