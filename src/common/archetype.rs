use rogue_macros::generate_tuples;
use std::{mem::offset_of, u64};

use crate::{
    common::{
        dyn_vec::{DynVec, DynVecCloneable, TypeInfoCloneable},
        freelist::FreeListHandle,
    },
    engine::entity::component::Bundle,
};

use super::dyn_vec::TypeInfo;

/// Essentially a type erased Free List Allocator with knowledge of (X, Y, Z)'s TypeIds and sizes.
/// Useful for storing parallel heterogenous (X, Y, Z) tuples but being able to iterate over just
/// a specific type, such as X. Also stored the global index
#[derive(Clone)]
pub struct Archetype {
    types: Vec<TypeInfoCloneable>,
    borrows: Vec<usize>,
    data: Vec<DynVecCloneable>,
    global_indices: Vec<FreeListHandle<()>>,
    size: u64,
    capacity: u64,
}

impl Archetype {
    pub const NULL_INDEX: u64 = u64::MAX;

    pub fn new(types: Vec<TypeInfoCloneable>) -> Self {
        let types_len = types.len();
        Self {
            types: types.clone(),
            borrows: vec![0; types_len],
            data: types.iter().map(|ty| DynVecCloneable::new(*ty)).collect(),
            global_indices: Vec::new(),
            size: 0,
            capacity: 0,
        }
    }

    fn get_type_data(&self, type_info: &TypeInfoCloneable) -> &DynVecCloneable {
        let i = self
            .types
            .iter()
            .enumerate()
            .find_map(|(i, x)| if x == type_info { Some(i) } else { None })
            .expect("Type info is not a part of this archetype");
        return &self.data[i];
    }

    fn get_type_data_mut(&mut self, type_info: &TypeInfoCloneable) -> &mut DynVecCloneable {
        let i = self
            .types
            .iter()
            .enumerate()
            .find_map(|(i, x)| if x == type_info { Some(i) } else { None })
            .expect("Type info is not a part of this archetype");
        return &mut self.data[i];
    }

    pub unsafe fn get_raw(&self, type_info: &TypeInfoCloneable, index: u64) -> *const u8 {
        let src_data = self.get_type_data(&type_info);
        return src_data.get_unchecked(index as usize).as_ptr();
    }

    unsafe fn insert_raw(
        &mut self,
        data: *const u8,
        type_info: TypeInfoCloneable,
        dst_byte_index: u64,
    ) {
    }

    fn allocate_entry(&mut self) -> u64 {
        let i = self.size;
        self.size += 1;
        return i;
    }

    pub fn remove(&mut self, index: u64) {
        assert_ne!(
            self.global_indices[index as usize],
            FreeListHandle::DANGLING
        );
        for (i, data) in self.data.iter_mut().enumerate() {
            let type_info = self.types[i];
            let ptr = data.get_mut_unchecked(index as usize).as_mut_ptr();
            log::info!("pre drop");
            unsafe { type_info.drop(ptr) };
            log::info!("post drop");
        }
        self.global_indices[index as usize] = FreeListHandle::DANGLING;
    }

    pub fn insert<T: ArchetypeStorage>(&mut self, global_id: FreeListHandle<()>, data: T) -> u64 {
        let index = self.allocate_entry();

        // Move `data` into our managed arrays.
        let data_info = data.type_info();
        for (i, (type_info, data_ptr)) in data_info.iter().enumerate() {
            let dst_byte_index = index * type_info.stride() as u64;
            assert_eq!(
                dst_byte_index % type_info.alignment() as u64,
                0,
                "dst_index is not properly aligned to what the source type should be."
            );

            let dst_data = &mut self.data[i];

            let val_bytes = unsafe { std::slice::from_raw_parts(*data_ptr, type_info.size()) };
            dst_data.push_unchecked(val_bytes);
        }
        std::mem::forget(data);

        self.global_indices.push(global_id);

        index
    }

    pub fn type_infos(&self) -> &[TypeInfoCloneable] {
        &self.types
    }

    pub fn iter(&self) -> ArchetypeIter<'_> {
        ArchetypeIter::new(self)
    }

    pub fn iter_mut(&mut self) -> ArchetypeIterMut<'_> {
        ArchetypeIterMut::new(self)
    }

    pub fn length(&self) -> usize {
        self.data[0].len() / self.types[0].size() as usize
    }
}

pub trait ArchetypeStorage {
    fn type_info_static() -> Vec<TypeInfo>;
    fn type_info(&self) -> Vec<(TypeInfo, *const u8)>;
}

macro_rules! impl_archetype_storage {
    ($($param:ident),+ , $($num:literal),*) => {
        impl<$($param: 'static),*> ArchetypeStorage for ($($param,)*) {
            fn type_info_static() -> Vec<TypeInfo> {
                vec![
                    $(TypeInfo::new::<$param>()),*
                ]
            }
            fn type_info(&self) -> Vec<(TypeInfo, *const u8)> {
                let p = std::slice::from_ref(self).as_ptr() as *const u8;
                vec![
                    $((
                        TypeInfo::new::<$param>(),
                        unsafe { p.offset(offset_of!(Self, $num) as isize) }
                    )),*
                ]
            }
        }
    }
}

generate_tuples!(impl_archetype_storage, 1, 8);

pub struct ArchetypeIter<'a> {
    archetype: &'a Archetype,
    i: usize,
}

impl<'a> ArchetypeIter<'a> {
    fn new(archetype: &'a Archetype) -> Self {
        Self { archetype, i: 0 }
    }
}

impl<'a> Iterator for ArchetypeIter<'a> {
    type Item = (FreeListHandle<()>, Vec<(TypeInfoCloneable, *const u8)>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.archetype.size as usize {
            return None;
        }

        let global_id = self.archetype.global_indices[self.i];
        if global_id == FreeListHandle::DANGLING {
            self.i += 1;
            return None;
        }
        let vec = self
            .archetype
            .types
            .iter()
            .map(|ty| {
                (ty.clone(), unsafe {
                    self.archetype.get_raw(ty, self.i as u64)
                })
            })
            .collect();

        self.i += 1;
        return Some((global_id, vec));
    }
}

pub struct ArchetypeIterMut<'a> {
    archetype: &'a mut Archetype,
    i: usize,
}

impl<'a> ArchetypeIterMut<'a> {
    fn new(archetype: &'a mut Archetype) -> Self {
        Self { archetype, i: 0 }
    }
}

impl<'a> Iterator for ArchetypeIterMut<'a> {
    type Item = (FreeListHandle<()>, Vec<(TypeInfoCloneable, *mut u8)>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.archetype.size as usize {
            return None;
        }

        let global_id = self.archetype.global_indices[self.i];
        if global_id == FreeListHandle::DANGLING {
            self.i += 1;
            return None;
        }
        let vec = self
            .archetype
            .types
            .iter()
            .map(|ty| {
                (ty.clone(), unsafe {
                    // Can cast to mut since we don't iterate over the same value twice in an
                    // iterator.
                    self.archetype.get_raw(ty, self.i as u64) as *mut u8
                })
            })
            .collect();

        self.i += 1;
        return Some((global_id, vec));
    }
}
