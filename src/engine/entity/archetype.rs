use std::{
    any::TypeId,
    cell::{Cell, Ref, RefCell},
    num::NonZeroU32,
    ptr::NonNull,
};

use rogue_macros::generate_tuples;

use crate::{
    common::dyn_vec::{DynVec, DynVecCloneable, TypeInfo},
    engine::entity::{
        component::{Bundle, ComponentTypeBorrow},
        ecs_world::Entity,
        query::QueryItemRef,
    },
};

/// Essentially a type erased Free List Allocator with knowledge of (X, Y, Z)'s TypeIds and sizes.
/// Useful for storing parallel heterogenous (X, Y, Z) tuples but being able to iterate over just
/// a specific type, such as X. Also stored the global index
///
/// Types are always sorted by TypeId, so index lookups are easier.
pub struct ComponentArchetype {
    pub types: Vec<TypeInfo>,
    pub borrows: Vec<Cell<ComponentTypeBorrow>>,
    data: Vec<DynVec>,
    global_indices: Vec<Entity>,
    size: usize,
    // temporary counte for ids, will ikmplement free hashset later for removing so we reuse.
    temp: usize,
    capacity: usize,
}

impl ComponentArchetype {
    pub fn new(mut types: Vec<TypeInfo>) -> Self {
        types.sort();
        let types_len = types.len();
        Self {
            types: types.clone(),
            borrows: vec![Cell::new(ComponentTypeBorrow::FREE); types_len],
            data: types
                .iter()
                .map(|ty| DynVec::new(ty.clone()))
                .collect::<Vec<_>>(),
            global_indices: Vec::new(),
            size: 0,
            temp: 0,
            capacity: 0,
        }
    }

    pub fn get_entity(&self, index: usize) -> Entity {
        let entity = self.global_indices[index];
        assert!(!entity.is_null());
        return entity;
    }

    fn get_type_index(&self, type_id: &TypeId) -> usize {
        self.types
            .iter()
            .enumerate()
            .find_map(|(i, x)| if &x.type_id == type_id { Some(i) } else { None })
            .expect("Type info is not a part of this archetype")
    }

    pub fn has_type<T: 'static>(&self) -> bool {
        self.has_type_id(std::any::TypeId::of::<T>())
    }

    pub fn has_type_id(&self, type_id: std::any::TypeId) -> bool {
        self.types.iter().find(|x| x.type_id == type_id).is_some()
    }

    fn get_type_data(&self, type_info: &TypeInfo) -> &DynVec {
        let i = self.get_type_index(&type_info.type_id);
        return &self.data[i];
    }

    fn get_type_data_mut(&mut self, type_info: &TypeInfo) -> &mut DynVec {
        let i = self.get_type_index(&type_info.type_id);
        return &mut self.data[i];
    }

    pub fn get<T: 'static>(&self, type_info: &TypeInfo, index: usize) -> &T {
        self.get_type_data(type_info).get(index)
    }

    pub unsafe fn get_raw(&self, type_info: &TypeInfo, index: usize) -> &[u8] {
        self.get_type_data(type_info).get_bytes(index)
    }

    /// For unsafety please see `DynVec::get_mut_unchecked(usize)`.
    pub unsafe fn get_mut_unchecked<T: 'static>(
        &self,
        type_info: &TypeInfo,
        index: usize,
    ) -> NonNull<T> {
        return self.get_type_data(type_info).get_mut_unchecked(index);
    }

    pub fn get_mut<T: 'static>(&mut self, type_info: &TypeInfo, index: usize) -> &mut T {
        self.get_type_data_mut(type_info).get_mut(index)
    }

    // This is unsafe if the type ordering of `src_data` doesn't match the internal archetype type
    // ordering, which is being sorted by the TypeInfo.
    pub unsafe fn insert_raw(&mut self, entity_id: Entity, src_data: Vec<*mut u8>) -> usize {
        assert_eq!(
            src_data.len(),
            self.types.len(),
            "Inserted entity data and archetype types must be the same length."
        );

        let index = self.allocate_entry();
        let new_allocation = index >= self.size;

        // Move `data` into our managed arrays.
        for (i, data_ptr) in src_data.iter().enumerate() {
            let mut dst_data = &mut self.data[i];
            if index >= dst_data.len() {
                // Safety: data_ptr is a valid ptr to a value offset into T.
                unsafe { dst_data.push_unchecked(*data_ptr) };
            } else {
                unsafe { dst_data.write_unchecked(index, *data_ptr) };
            }
        }

        if new_allocation {
            self.global_indices.push(entity_id);
        } else {
            self.global_indices[index as usize] = entity_id;
        }
        self.size += 1;

        index
    }

    pub fn borrow_type(&self, type_id: &TypeId) -> &Cell<ComponentTypeBorrow> {
        let index = self.get_type_index(type_id);
        let borrow = &self.borrows[index];
        let borrow_val = borrow.get();
        assert!(
            borrow_val.is_readable(),
            "Component type is already borrowed mutably!",
        );
        borrow.set(borrow_val.inc());
        return borrow;
    }

    pub fn borrow_type_mut(&self, type_id: &TypeId) -> &Cell<ComponentTypeBorrow> {
        let index = self.get_type_index(type_id);
        let borrow = &self.borrows[index];
        assert!(
            borrow.get().is_writeabe(),
            "Component type is already borrowed!",
        );
        borrow.set(ComponentTypeBorrow::WRITE_LOCKED);
        return borrow;
    }

    pub fn remove(&mut self, index: usize) {
        assert_ne!(self.global_indices[index as usize], Entity::DANGLING);
        for (i, data) in self.data.iter_mut().enumerate() {
            let type_info = self.types[i];
            let ptr = data.get_mut_bytes(index).as_mut_ptr();
            // Safety: We overwrite the DynVec drop function so this is not dropped twice, and
            // track the status of this index via `global_indices`.
            unsafe { type_info.drop(ptr) };
        }
        self.global_indices[index as usize] = Entity::DANGLING;
        self.size -= 1;
    }

    /// Returns a vec of ptrs to the data with the indices lining up with this archetypes type infos.
    /// This leaves the slot's entity dangling but doesn't drop the data so it can be copied.
    pub fn take_raw(&mut self, index: usize) -> Vec<*mut u8> {
        assert_ne!(self.global_indices[index as usize], Entity::DANGLING);
        let ptrs = self
            .data
            .iter_mut()
            .map(|data| data.get_mut_bytes(index).as_mut_ptr())
            .collect::<Vec<_>>();
        self.global_indices[index as usize] = Entity::DANGLING;
        self.size -= 1;
        return ptrs;
    }

    // Allocates a local index for the next entry.
    fn allocate_entry(&mut self) -> usize {
        let i = self.temp;
        self.temp += 1;
        return i;
    }

    pub fn insert(&mut self, entity_id: Entity, data: Vec<(TypeInfo, *const u8)>) -> usize {
        let index = self.allocate_entry();
        let new_allocation = index >= self.size;

        // Move `data` into our managed arrays.
        let mut data_info = data;
        data_info.sort_by(|(type_info_a, _), (type_info_b, _)| type_info_a.cmp(type_info_b));
        for (i, (type_info, data_ptr)) in data_info.iter().enumerate() {
            let mut dst_data = &mut self.data[i];
            assert_eq!(type_info.type_id, self.types[i].type_id);
            if index >= dst_data.len() {
                // Safety: data_ptr is a valid ptr to a value offset into T.
                unsafe { dst_data.push_unchecked(*data_ptr) };
            } else {
                unsafe { dst_data.write_unchecked(index, *data_ptr) };
            }
        }

        if new_allocation {
            self.global_indices.push(entity_id);
        } else {
            self.global_indices[index as usize] = entity_id;
        }
        self.size += 1;

        index
    }

    pub fn type_infos(&self) -> &[TypeInfo] {
        &self.types
    }

    pub fn len(&self) -> usize {
        self.size
    }
}

impl Drop for ComponentArchetype {
    fn drop(&mut self) {
        for data in &mut self.data {
            // Safety: We drop the items that still need to be dropped below.
            unsafe { data.forget_data() };
        }

        for i in 0..self.size {
            if self.global_indices[i].is_null() {
                continue;
            }

            for data in &mut self.data {
                let type_info = data.type_info().clone();
                let curr_ptr = unsafe { data.as_mut_ptr().byte_add(type_info.stride() * i) };
                unsafe { type_info.drop(curr_ptr) };
            }
        }
    }
}

//pub struct ArchetypeIter<'a> {
//    archetype: &'a Archetype,
//    i: usize,
//}
//
//impl<'a> ArchetypeIter<'a> {
//    fn new(archetype: &'a Archetype) -> Self {
//        Self { archetype, i: 0 }
//    }
//}
//
//impl<'a> Iterator for ArchetypeIter<'a> {
//    type Item = (FreeListHandle<()>, Vec<(TypeInfo, *const u8)>);
//
//    fn next(&mut self) -> Option<Self::Item> {
//        if self.i == self.archetype.size as usize {
//            return None;
//        }
//
//        let global_id = self.archetype.global_indices[self.i];
//        if global_id == FreeListHandle::DANGLING {
//            self.i += 1;
//            return None;
//        }
//        let vec = self
//            .archetype
//            .types
//            .iter()
//            .map(|ty| {
//                (ty.clone(), unsafe {
//                    self.archetype.get_raw(ty, self.i as u64)
//                })
//            })
//            .collect();
//
//        self.i += 1;
//        return Some((global_id, vec));
//    }
//}
//
//pub struct ArchetypeIterMut<'a> {
//    archetype: &'a mut Archetype,
//    i: usize,
//}
//
//impl<'a> ArchetypeIterMut<'a> {
//    fn new(archetype: &'a mut Archetype) -> Self {
//        Self { archetype, i: 0 }
//    }
//}
//
//impl<'a> Iterator for ArchetypeIterMut<'a> {
//    type Item = (FreeListHandle<()>, Vec<(TypeInfo, *mut u8)>);
//
//    fn next(&mut self) -> Option<Self::Item> {
//        if self.i == self.archetype.size as usize {
//            return None;
//        }
//
//        let global_id = self.archetype.global_indices[self.i];
//        if global_id == FreeListHandle::DANGLING {
//            self.i += 1;
//            return None;
//        }
//        let vec = self
//            .archetype
//            .types
//            .iter()
//            .map(|ty| {
//                (ty.clone(), unsafe {
//                    // Can cast to mut since we don't iterate over the same value twice in an
//                    // iterator.
//                    self.archetype.get_raw(ty, self.i as u64) as *mut u8
//                })
//            })
//            .collect();
//
//        self.i += 1;
//        return Some((global_id, vec));
//    }
//}
//
////
//#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
//pub struct ComponentTypeInfo {
//    type_id: std::any::TypeId,
//    drop_fn: unsafe fn(*mut u8),
//    size: usize,
//    alignment: usize,
//}
//
//impl ComponentTypeInfo {
//    pub fn new<T: 'static>() -> Self {
//        unsafe fn drop_fn<T>(ptr: *mut u8) {
//            std::ptr::drop_in_place(ptr as *mut T);
//        }
//
//        Self {
//            type_id: std::any::TypeId::of::<T>(),
//            drop_fn: drop_fn::<T>,
//            size: std::mem::size_of::<T>(),
//            alignment: std::mem::align_of::<T>(),
//        }
//    }
//
//    pub fn type_id(&self) -> std::any::TypeId {
//        self.type_id
//    }
//
//    pub fn size(&self) -> usize {
//        self.size
//    }
//
//    pub unsafe fn drop(&self, data: *mut u8) {
//        (self.drop_fn)(data);
//    }
//
//    pub fn alignment(&self) -> usize {
//        self.alignment
//    }
//
//    pub fn stride(&self) -> usize {
//        self.size().next_multiple_of(self.alignment)
//    }
//}
