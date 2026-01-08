use std::{
    any::TypeId,
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
};

use downcast::{downcast, Any};

pub trait Resource: Any {}
downcast!(dyn Resource);

pub(crate) type BoxedResource = Box<dyn Resource>;

pub type Res<'rb, R> = Ref<'rb, R>;
pub type ResMut<'rb, R> = RefMut<'rb, R>;

pub struct ResourceBank {
    resources: HashMap<TypeId, RefCell<BoxedResource>>,
}

impl ResourceBank {
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
        }
    }

    pub fn has_resource<R: Resource>(&self) -> bool {
        self.resources.contains_key(&TypeId::of::<R>())
    }

    pub fn get_resource<R: Resource>(&self) -> Res<R> {
        Ref::map(
            self.resources
                .get(&TypeId::of::<R>())
                .expect(&format!(
                    "Failed to get resource: {}",
                    std::any::type_name::<R>()
                ))
                .borrow(),
            |r| r.downcast_ref().unwrap(),
        )
    }

    pub fn get_resource_mut<R: Resource>(&self) -> ResMut<R>
    where
        R: Resource,
    {
        RefMut::map(
            self.resources
                .get(&TypeId::of::<R>())
                .expect(&format!(
                    "Failed to get resource: {}",
                    std::any::type_name::<R>()
                ))
                .borrow_mut(),
            |r| r.downcast_mut().unwrap(),
        )
    }

    pub fn insert<R: Resource>(&mut self, resource: R) {
        self.resources
            .insert(TypeId::of::<R>(), RefCell::new(Box::new(resource)));
    }
}
