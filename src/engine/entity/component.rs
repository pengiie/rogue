use std::{any::TypeId, cell::Cell, collections::HashMap, ptr::NonNull};

use rogue_macros::generate_tuples;

use crate::{
    common::dyn_vec::TypeInfo,
    engine::{
        entity::{archetype::ComponentArchetype, query::QueryItemRef},
        physics::physics_world::PhysicsWorld,
        resource::ResourceBank,
        voxel::voxel_world::VoxelWorld,
    },
};

pub struct GameComponentContext<'a> {
    pub voxel_world: &'a mut VoxelWorld,
    pub physics_world: &'a mut PhysicsWorld,
}

/// Implements serialization and cloning.
pub trait GameComponent {
    fn clone_component(&self, ctx: &mut GameComponentContext<'_>, dst_ptr: *mut u8);

    fn serialize_component(
        &self,
        ctx: GameComponentContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()>;

    fn deserialize_component(
        &self,
        ctx: GameComponentContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()>;
}

/// Used for spawning an entity with component, or inserting multiple components at a time.
pub trait Bundle {
    fn component_type_ids() -> Vec<TypeId>;
    fn component_type_infos() -> Vec<TypeInfo>;

    fn type_info(&self) -> Vec<(TypeInfo, *const u8)>;
}

macro_rules! impl_bundle {
    ($($param:ident),+ , $($num:literal),*) => {
        impl<$($param: 'static),*> Bundle for ($($param,)*) {
            fn component_type_ids() -> Vec<TypeId> {
                vec![
                    $(std::any::TypeId::of::<$param>()),*
                ]
            }

            fn component_type_infos() -> Vec<TypeInfo> {
                vec![
                    $(TypeInfo::new::<$param>()),*
                ]
            }

            fn type_info(&self) -> Vec<(TypeInfo, *const u8)> {
                let p = std::slice::from_ref(self).as_ptr() as *const u8;
                vec![
                    $((
                        TypeInfo::new::<$param>(),
                        unsafe { p.offset(std::mem::offset_of!(Self, $num) as isize) }
                    )),*
                ]
            }
        }
    }
}

generate_tuples!(impl_bundle, 1, 16);

impl Bundle for () {
    fn component_type_ids() -> Vec<TypeId> {
        vec![]
    }

    fn component_type_infos() -> Vec<TypeInfo> {
        vec![]
    }

    fn type_info(&self) -> Vec<(TypeInfo, *const u8)> {
        vec![]
    }
}

pub struct ComponentBorrowMap {
    borrows: HashMap<TypeId, Cell<ComponentTypeBorrow>>,
}

impl ComponentBorrowMap {
    pub fn new() -> Self {
        Self {
            borrows: HashMap::new(),
        }
    }

    pub fn borrow_type(&self, type_id: &TypeId) -> &Cell<ComponentTypeBorrow> {
        let borrow = self.borrows.get(type_id).unwrap();
        let borrow_val = borrow.get();
        assert!(
            borrow_val.is_readable(),
            "Component type is already borrowed mutably!",
        );
        borrow.set(ComponentTypeBorrow(borrow_val.0 + 1));
        return borrow;
    }

    pub fn borrow_type_mut(&self, type_id: &TypeId) -> &Cell<ComponentTypeBorrow> {
        let borrow = self.borrows.get(type_id).unwrap();
        assert!(
            borrow.get().is_writeabe(),
            "Component type is already borrowed!",
        );
        borrow.set(ComponentTypeBorrow::WRITE_LOCKED);
        return borrow;
    }

    pub fn ensure_type_exists(&mut self, type_id: &TypeId) {
        if !self.borrows.contains_key(type_id) {
            self.borrows
                .insert(*type_id, Cell::new(ComponentTypeBorrow::FREE));
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ComponentTypeBorrow(u32);

impl ComponentTypeBorrow {
    pub const WRITE_BIT: u32 = 1 << 31;
    pub const FREE: Self = ComponentTypeBorrow(0);
    pub const WRITE_LOCKED: Self = ComponentTypeBorrow(Self::WRITE_BIT);

    pub fn is_writeabe(&self) -> bool {
        self.0 == 0
    }

    pub fn inc(&self) -> Self {
        return Self(self.0 + 1);
    }

    // Decrements the read count if it is a read borrow, or frees entirely if a write borrow.
    pub fn unborrow(&self) -> Self {
        if self.0 == Self::WRITE_BIT {
            return Self::FREE;
        }
        return Self(self.0.saturating_sub(1));
    }

    pub fn is_readable(&self) -> bool {
        (self.0 & Self::WRITE_BIT) == 0
    }
}

pub struct ComponentRef<'a, T> {
    // From Rust std::Cell::Ref:
    // NB: we use a pointer instead of `&'b T` to avoid `noalias` violations, because a
    // `Ref` argument doesn't hold immutability for its whole scope, only until it drops.
    // `NonNull` is also covariant over `T`, just like we would have with `&T`.
    //
    // Essentially since borrow checker doesn't check, a &T could be `noalias` while already
    // have been dropped, which is UB.
    component: NonNull<T>,
    borrow: &'a Cell<ComponentTypeBorrow>,
}

impl<'a, T: 'static> QueryItemRef<'a> for ComponentRef<'a, T> {
    fn create_ref(archetype: &'a ComponentArchetype, index: usize) -> Self {
        // Safety: I don't think this method is actually unsafe
        let data = unsafe { archetype.get_raw(&TypeInfo::new::<T>(), index) }.as_ptr() as *mut T;
        let component = NonNull::new(data).unwrap();
        let borrow = archetype.borrow_type(&std::any::TypeId::of::<T>());
        ComponentRef { component, borrow }
    }
}

impl<T> Drop for ComponentRef<'_, T> {
    fn drop(&mut self) {
        let val = self.borrow.get();
        self.borrow.set(val.unborrow());
    }
}

impl<T> std::ops::Deref for ComponentRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: Since this component is dynamically borrow and called within the lifetime of
        // this `ComponentRef`, the pointer should be safe to access.
        unsafe { self.component.as_ref() }
    }
}

pub struct ComponentRefMut<'a, T> {
    // From Rust std::Cell::Ref:
    // NB: we use a pointer instead of `&'b T` to avoid `noalias` violations, because a
    // `Ref` argument doesn't hold immutability for its whole scope, only until it drops.
    // `NonNull` is also covariant over `T`, just like we would have with `&T`.
    //
    // Essentially since borrow checker doesn't check, a &T could be `noalias` while already
    // have been dropped, which is UB.
    component: NonNull<T>,
    borrow: &'a Cell<ComponentTypeBorrow>,
}

impl<T> std::ops::Deref for ComponentRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: Since this component is dynamically borrow and called within the lifetime of
        // this `ComponentRef`, the pointer should be safe to access.
        unsafe { self.component.as_ref() }
    }
}

impl<T> std::ops::DerefMut for ComponentRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: Since this component is dynamically borrow and called within the lifetime of
        // this `ComponentRef`, the pointer should be safe to access.
        unsafe { self.component.as_mut() }
    }
}

impl<'a, T: 'static> QueryItemRef<'a> for ComponentRefMut<'a, T> {
    fn create_ref(archetype: &'a ComponentArchetype, index: usize) -> Self {
        // Safety: I don't think this method is actually unsafe
        let data = unsafe { archetype.get_raw(&TypeInfo::new::<T>(), index) }.as_ptr() as *mut T;
        let component = NonNull::new(data).unwrap();
        let borrow = archetype.borrow_type_mut(&std::any::TypeId::of::<T>());
        ComponentRefMut { component, borrow }
    }
}

impl<T> Drop for ComponentRefMut<'_, T> {
    fn drop(&mut self) {
        let val = self.borrow.get();
        self.borrow.set(val.unborrow());
    }
}
