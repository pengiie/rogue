use rogue_macros::generate_tuples;
use std::{mem::offset_of, u64};

use super::dyn_vec::TypeInfo;

/// Essentially a type erased Free List Allocator with knowledge of (X, Y, Z)'s TypeIds and sizes.
/// Useful for storing contiguous heterogenous (X, Y, Z) tuples but being able to iterate over just
/// a specific type, such as X.
pub struct Archetype {
    types: Vec<TypeInfo>,
    // TODO: Replace with DynVec
    data: Vec<Vec<u8>>,
    global_indices: Vec<u64>,
    size: u64,
    capacity: u64,
}

impl Archetype {
    pub const NULL_INDEX: u64 = u64::MAX;

    pub fn new(types: Vec<TypeInfo>) -> Self {
        let types_len = types.len();
        Self {
            types,
            data: (0..types_len).map(|_| Vec::new()).collect(),
            global_indices: Vec::new(),
            size: 0,
            capacity: 0,
        }
    }

    fn get_type_data(&self, type_info: &TypeInfo) -> &Vec<u8> {
        let i = self
            .types
            .iter()
            .enumerate()
            .find_map(|(i, x)| if x == type_info { Some(i) } else { None })
            .expect("Type info is not a part of this archetype");
        return &self.data[i];
    }

    fn get_type_data_mut(&mut self, type_info: &TypeInfo) -> &mut Vec<u8> {
        let i = self
            .types
            .iter()
            .enumerate()
            .find_map(|(i, x)| if x == type_info { Some(i) } else { None })
            .expect("Type info is not a part of this archetype");
        return &mut self.data[i];
    }

    pub unsafe fn get_raw(&self, type_info: &TypeInfo, index: u64) -> *const u8 {
        let src_data = self.get_type_data(&type_info);
        src_data
            .as_slice()
            .as_ptr()
            .offset((type_info.size() as u64 * index) as isize)
    }

    unsafe fn insert_raw(&mut self, data: *const u8, type_info: TypeInfo, dst_byte_index: u64) {
        assert_eq!(
            dst_byte_index % type_info.alignment() as u64,
            0,
            "dst_index is not properly aligned to what the source type should be."
        );

        let dst_data = self.get_type_data_mut(&type_info);

        unsafe {
            let dst_ptr = dst_data
                .as_mut_slice()
                .as_mut_ptr()
                .offset(dst_byte_index as isize);
            dst_ptr.copy_from_nonoverlapping(data, type_info.size() as usize);
        }
    }

    fn resize(&mut self, additional: u64) {
        for (type_info, type_data) in self.types.iter().zip(self.data.iter_mut()) {
            type_data.resize(
                type_data.len() + (type_info.size() as u64 * additional) as usize,
                0,
            );
        }
        self.global_indices.resize(
            self.global_indices.len() + additional as usize,
            Self::NULL_INDEX,
        );
        self.capacity += additional;
    }

    fn allocate_entry(&mut self) -> u64 {
        self.resize(1);
        let i = self.size;
        self.size += 1;
        return i;
    }

    pub fn insert<T: ArchetypeStorage>(&mut self, global_id: u64, data: T) -> u64 {
        let index = self.allocate_entry();

        // Move `data` into our managed arrays.
        let data_info = data.type_info();
        for (ty, data_ptr) in data_info {
            unsafe {
                self.insert_raw(data_ptr, ty, index * ty.size() as u64);
            }
        }
        std::mem::forget(data);

        self.global_indices[index as usize] = global_id;

        index
    }

    pub fn type_infos(&self) -> &[TypeInfo] {
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
    type Item = (u64, Vec<(TypeInfo, *const u8)>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.archetype.size as usize {
            return None;
        }

        let global_id = self.archetype.global_indices[self.i];
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
    type Item = (u64, Vec<(TypeInfo, *mut u8)>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.archetype.size as usize {
            return None;
        }

        let global_id = self.archetype.global_indices[self.i];
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
