use std::{any::TypeId, cell::Cell, collections::HashMap, ptr::NonNull};

use crate::common::dyn_vec::TypeInfo;
use crate::entity::{archetype::ComponentArchetype, ecs_world::Entity, query::QueryItemRef};
use crate::physics::collider_registry::ColliderRegistry;
use crate::voxel::voxel_registry::VoxelModelRegistry;
use rogue_macros::generate_tuples;
use crate::animation::animation_property::{AnimationPropertyMethods, AnimationPropertyTypeInfo};

#[derive(Clone)]
pub struct GameComponentType {
    pub type_info: TypeInfo,
    pub component_name: String,
    pub is_constructible: bool,
    pub animaton_properties: Vec<AnimationPropertyTypeInfo>,
    pub construct_fn: GameComponentConstructFnPtr,
    pub deserialize_fn: GameComponentDeserializeFnPtr,
    pub methods_vtable_ptr: GameComponentMethodsVtablePtr,
}

impl GameComponentType {
    /// Safety: Data pointer must not be null, and should be allocated with layout of type info of
    /// this game component.
    pub unsafe fn as_dyn_mut<'a>(&self, data_ptr: *mut u8) -> &'a mut dyn GameComponentMethods {
        // Safety: The pointer should be valid and properly aligned, and the vtable pointer should be correct.
        unsafe {
            let vtable = self.methods_vtable_ptr as *const ();
            let fat_ptr = (data_ptr as *mut (), vtable);
            std::mem::transmute::<(*mut (), *const ()), &mut dyn GameComponentMethods>(fat_ptr)
        }
    }
}

pub struct GameComponentCloneContext<'a> {
    pub voxel_registry: &'a mut VoxelModelRegistry,
    pub collider_registry: &'a mut ColliderRegistry,
}

pub struct GameComponentSerializeContext<'a> {
    pub voxel_registry: &'a VoxelModelRegistry,
    pub collider_registry: &'a ColliderRegistry,
    pub entity_uuid_map: &'a HashMap<Entity, uuid::Uuid>,
}

pub struct GameComponentDeserializeContext<'a> {
    pub voxel_registry: &'a mut VoxelModelRegistry,
    pub collider_registry: &'a mut ColliderRegistry,
    /// Used for `EntityParent` in deserializing. By default is `uuid::Uuid::nil()`.
    pub entity_parent: uuid::Uuid,
}

pub struct GameComponentPropertiesUIContext<'a> {
    pub voxel_registry: &'a mut VoxelModelRegistry,
    pub collider_registry: &'a mut ColliderRegistry,
}

pub trait GameComponentMethods {
    fn clone_component(&self, ctx: &mut GameComponentCloneContext<'_>, dst_ptr: *mut u8);

    fn serialize_component(
        &self,
        ctx: &GameComponentSerializeContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()>;

    fn get_animation_property(&mut self, property: &str) -> &mut dyn AnimationPropertyMethods;
}

impl<T: GameComponent> GameComponentMethods for T {
    fn clone_component(&self, ctx: &mut GameComponentCloneContext<'_>, dst_ptr: *mut u8) {
        GameComponent::clone_component(self, ctx, dst_ptr);
    }

    fn serialize_component(
        &self,
        ctx: &GameComponentSerializeContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        GameComponent::serialize_component(self, ctx, ser)
    }

    fn get_animation_property(&mut self, property: &str) -> &mut dyn AnimationPropertyMethods {
        GameComponent::get_animation_property(self, property)
    }
}

pub type GameComponentDeserializeFnPtr = unsafe fn(
    /*ctx: */ &mut GameComponentDeserializeContext<'_>,
    /*de: */ &mut dyn erased_serde::Deserializer,
    /*dst_ptr: */ *mut u8,
) -> erased_serde::Result<()>;

pub type GameComponentConstructFnPtr = unsafe fn(/*dst_ptr: */ *mut u8);

pub type GameComponentMethodsVtablePtr = *const ();

/// Implements serialization and cloning.
pub trait GameComponent {
    const NAME: &str;

    fn is_constructible() -> bool {
        false
    }

    fn construct_component(dst_ptr: *mut u8) {
        if Self::is_constructible() {
            panic!(
                "Game component {} marked as constructible but GameComponent::construct was not implemented.",
                std::any::type_name::<Self>()
            );
        } else {
            panic!(
                "Call GameComponent::construct on a non-constructible game component {}.",
                std::any::type_name::<Self>()
            );
        }
    }

    fn animation_properties() -> Vec<AnimationPropertyTypeInfo> {
        Vec::new()
    }

    fn is_animatable() -> bool {
        !Self::animation_properties().is_empty()
    }

    fn get_animation_property(&mut self, property: &str) -> &mut dyn AnimationPropertyMethods {
        panic!(
            "GameComponent::animation_properties was either not written correctly, or this was called without checking if property {} exists first.",
            property
        );
    }

    fn clone_component(&self, ctx: &mut GameComponentCloneContext<'_>, dst_ptr: *mut u8);

    fn serialize_component(
        &self,
        ctx: &GameComponentSerializeContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()>;

    unsafe fn deserialize_component(
        ctx: &mut GameComponentDeserializeContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()>;
}

/// Used for spawning an entity with component, or inserting multiple components at a time.
pub trait Bundle {
    fn component_type_ids() -> Vec<TypeId>;
    fn component_type_infos() -> Vec<TypeInfo>;

    unsafe fn type_info(&self) -> Vec<(TypeInfo, *const u8)>;
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

            unsafe fn type_info(&self) -> Vec<(TypeInfo, *const u8)> {
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

    unsafe fn type_info(&self) -> Vec<(TypeInfo, *const u8)> {
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

pub struct RawComponentRef<'a> {
    component: NonNull<()>,
    borrow: &'a Cell<ComponentTypeBorrow>,
}

impl<'a> RawComponentRef<'a> {
    pub fn get_component_ptr(&self) -> *mut () {
        self.component.as_ptr()
    }

    pub fn create_ref(archetype: &'a ComponentArchetype, type_id: &TypeId, index: usize) -> Self {
        // Safety: I don't think this method is actually unsafe
        let data = unsafe { archetype.get_raw(type_id, index) }.as_ptr() as *mut ();
        let component = NonNull::new(data).unwrap();
        let borrow = archetype.borrow_type(type_id);
        Self { component, borrow }
    }
}

impl Drop for RawComponentRef<'_> {
    fn drop(&mut self) {
        let val = self.borrow.get();
        self.borrow.set(val.unborrow());
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
        let data = unsafe { archetype.get_raw(&TypeId::of::<T>(), index) }.as_ptr() as *mut T;
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
        let data = unsafe { archetype.get_raw(&TypeId::of::<T>(), index) }.as_ptr() as *mut T;
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
