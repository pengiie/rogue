use rogue_macros::generate_tuples;

use crate::resource::{Res, ResMut, Resource, ResourceBank};

type SystemParamItem<'rb, T> = <T as SystemParam>::Item<'rb>;
pub trait SystemParam {
    type Item<'rb>: SystemParam;

    fn from_resource_bank(resource_bank: &ResourceBank) -> Self::Item<'_>;
}

impl SystemParam for &'_ ResourceBank {
    type Item<'rb> = &'rb ResourceBank;

    fn from_resource_bank(resource_bank: &ResourceBank) -> Self::Item<'_> {
        resource_bank
    }
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

/// Stores the system since we cannot simply due dyn System due to the
/// Marker bound. This stores the system impl via ptr and also stores
/// a fn ptr to a polymorphic function which knows how to call that
/// specific System trait with its bounds.
pub struct SystemErased {
    system_run_fn: fn(*const (), &ResourceBank) -> (),
    system_ptr: *const (),
}

impl SystemErased {
    pub fn new<F: System<Marker> + 'static, Marker>(system_fn: F) -> Self {
        fn run_erased<F: System<Marker>, Marker>(
            system_ptr: *const (),
            resource_bank: &ResourceBank,
        ) {
            // Safety: By the generic types this will be the right function ptr
            // type so we can safety cast to this closure.
            let system = unsafe { &*(system_ptr as *const F) };
            system.run(resource_bank);
        }

        let system_ptr = std::ptr::from_ref(&system_fn);
        Self {
            system_run_fn: run_erased::<F, Marker>,
            system_ptr: system_ptr as *const (),
        }
    }

    pub fn run(&self, resource_bank: &ResourceBank) {
        (self.system_run_fn)(self.system_ptr, resource_bank);
    }
}

pub trait System<Marker> {
    fn run(&self, resource_bank: &ResourceBank);
}

macro_rules! impl_system {
    ($($param:ident), *$(,)? $($num:literal),*) => {
        impl<F, $($param: SystemParam),*> System<($($param),*)> for F
        where
            F: Fn($($param),*) + Fn($(SystemParamItem<$param>),*),
            $($param: SystemParam),*
        {
            fn run(&self, _resource_bank: &ResourceBank) {
                self($(<$param as SystemParam>::from_resource_bank(_resource_bank)),*);
            }
        }
    };
}

//macro_rules! impl_system_param_tuple {
//    ($($param:ident), *$(,)? $($num:literal),*) => {
//        impl<$($param: SystemParam),*> SystemParam for ($($param),*)
//        where
//            $($param: SystemParam),*
//        {
//            type Item<'rb> = Self;
//
//            fn from_resource_bank(resource_bank: &ResourceBank) -> Self::Item<'_> {
//                (
//                    $(
//                        <$param as SystemParam>::from_resource_bank(resource_bank)
//                    ),*
//                )
//            }
//        }
//    };
//}
//generate_tuples!(impl_system_param_tuple, 2, 16);

generate_tuples!(impl_system, 0, 16);
