use std::{
    any::TypeId,
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
};

use downcast::{Any, downcast};

use crate::system::System;

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

    pub fn try_get_resource<R: Resource>(&self) -> Option<Res<R>> {
        let ref_cell = self.resources.get(&TypeId::of::<R>())?;
        Some(Ref::map(ref_cell.borrow(), |r| r.downcast_ref().unwrap()))
    }

    pub fn try_get_resource_mut<R: Resource>(&self) -> Option<ResMut<R>> {
        let ref_cell = self.resources.get(&TypeId::of::<R>())?;
        Some(RefMut::map(ref_cell.borrow_mut(), |r| {
            r.downcast_mut().unwrap()
        }))
    }

    pub fn get_resource<R: Resource>(&self) -> Res<R>
    where
        R: Resource,
    {
        self.try_get_resource::<R>()
            .unwrap_or_else(|| panic!("Resource of type {} not found", std::any::type_name::<R>()))
    }

    pub fn get_resource_mut<R: Resource>(&self) -> ResMut<R>
    where
        R: Resource,
    {
        self.try_get_resource_mut::<R>()
            .unwrap_or_else(|| panic!("Resource of type {} not found", std::any::type_name::<R>()))
    }

    pub fn insert<R: Resource>(&mut self, resource: R) {
        self.resources
            .insert(TypeId::of::<R>(), RefCell::new(Box::new(resource)));
    }

    pub fn run_system<Marker>(&self, mut system: impl System<Marker>) {
        system.run(self);
    }
}
