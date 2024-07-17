use rogue_macros::generate_tuples;

use super::resource::{Res, ResMut, Resource, ResourceBank};

type SystemParamItem<'rb, T> = <T as SystemParam>::Item<'rb>;
pub trait SystemParam {
    type Item<'rb>: SystemParam;

    fn from_resource_bank(resource_bank: &ResourceBank) -> Self::Item<'_>;
}

impl<R> SystemParam for Res<'_, R>
where
    R: Resource,
{
    type Item<'rb> = Res<'rb, R>;

    fn from_resource_bank(resource_bank: &ResourceBank) -> Self::Item<'_> {
        resource_bank.get_resource::<R>()
    }
}

impl<R> SystemParam for ResMut<'_, R>
where
    R: Resource,
{
    type Item<'rb> = ResMut<'rb, R>;

    fn from_resource_bank(resource_bank: &ResourceBank) -> Self::Item<'_> {
        resource_bank.get_resource_mut::<R>()
    }
}

pub trait System<Marker> {
    fn run(&mut self, resource_bank: &ResourceBank);
}

macro_rules! impl_system {
    ($($param:ident),*) => {
        impl<F, $($param: SystemParam),*> System<fn($($param),*) -> ()> for F
        where
            F: FnMut($($param),*) + FnMut($(SystemParamItem<$param>),*),
            $($param: SystemParam),*
        {
            fn run(&mut self, _resource_bank: &ResourceBank) {
                self($(<$param as SystemParam>::from_resource_bank(_resource_bank)),*);
            }
        }
    };
}

generate_tuples!(impl_system, 16);
