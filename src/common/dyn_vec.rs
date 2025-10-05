use std::{any::TypeId, ptr::NonNull};

pub struct DynVec {
    type_info: TypeInfo,
    data: NonNull<u8>,
    size: usize,
    capacity: usize,
}

impl DynVec {
    pub fn new(type_info: TypeInfo) -> Self {
        Self {
            type_info,
            data: NonNull::dangling(),
            size: 0,
            capacity: 0,
        }
    }

    /// Used to ensure the drop function is not called for any values contained within. Unsafe
    /// since this responsibility is now delegated to the wrapper of this DynVec.
    pub unsafe fn forget_data(&mut self) {
        self.size = 0;
    }

    pub fn push<T: 'static>(&mut self, val: T) {
        let type_info = TypeInfo::new::<T>();
        assert_eq!(self.type_info, type_info);

        // Safety: val must be a valid object
        unsafe { self.push_unchecked(std::ptr::from_ref(&val) as *const u8) };
        std::mem::forget(val);
    }

    pub unsafe fn push_unchecked(&mut self, bytes: *const u8) {
        if self.size == self.capacity {
            self.grow(1);
        }

        unsafe {
            let dst_ptr = self.data.byte_add(self.type_info.stride() * self.size);
            bytes.copy_to_nonoverlapping(dst_ptr.as_ptr(), self.type_info.size)
        };
        self.size += 1;
    }

    pub unsafe fn write_unchecked(&mut self, index: usize, bytes: *const u8) {
        assert!(index < self.capacity);
        assert!(index < self.size);

        unsafe {
            let dst_ptr = self.data.byte_add(self.type_info.stride() * index);
            bytes.copy_to_nonoverlapping(dst_ptr.as_ptr(), self.type_info.size)
        };
    }

    pub fn get<T: 'static>(&self, index: usize) -> &T {
        let type_info = TypeInfo::new::<T>();
        assert_eq!(self.type_info, type_info);
        assert!(self.size > index, "Index is out of bounds.");

        let bytes = self.get_bytes(index).as_ptr() as *const T;
        unsafe { bytes.as_ref().unwrap() }
    }

    pub fn get_mut<T: 'static>(&mut self, index: usize) -> &mut T {
        let type_info = TypeInfo::new::<T>();
        assert_eq!(self.type_info, type_info);
        assert!(self.size > index, "Index is out of bounds.");

        let bytes = self.get_mut_bytes(index).as_ptr() as *mut T;
        unsafe { bytes.as_mut().unwrap() }
    }

    /// Unsafe since this provides interior mutability (can modify elements of self without a
    /// mutable reference to self). Meaning this requires external borrow checking.
    pub unsafe fn get_mut_unchecked<T: 'static>(&self, index: usize) -> NonNull<T> {
        let type_info = TypeInfo::new::<T>();
        assert_eq!(self.type_info, type_info);
        assert!(self.size > index, "Index is out of bounds.");

        assert!(index < self.size);
        let stride = self.type_info.stride();
        // Safety: We check index is in bounds and offset by a stride of the type alignment.
        return unsafe { self.data.offset((stride * index) as isize).cast::<T>() };
    }

    pub fn get_bytes(&self, index: usize) -> &[u8] {
        assert!(index < self.size);
        let stride = self.type_info.stride();
        let start = unsafe { self.data.offset((stride * index) as isize) };
        return unsafe { std::slice::from_raw_parts(start.as_ptr(), stride) };
    }

    pub fn get_mut_bytes(&mut self, index: usize) -> &mut [u8] {
        assert!(index < self.size);
        let stride = self.type_info.stride();
        let start = unsafe { self.data.offset((stride * index) as isize) };
        return unsafe { std::slice::from_raw_parts_mut(start.as_ptr(), stride) };
    }

    pub fn type_info(&self) -> &TypeInfo {
        &self.type_info
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_ptr()
    }

    pub fn is_empty(&self) -> bool {
        return self.size == 0;
    }

    fn grow(&mut self, grow_amount: usize) {
        let new_capacity = self.capacity + grow_amount;

        let new_layout_size = self.type_info.stride() * new_capacity;
        let new_layout =
            std::alloc::Layout::from_size_align(new_layout_size, self.type_info.alignment())
                .unwrap();

        let new_data_ptr = if self.capacity == 0 {
            unsafe { NonNull::new_unchecked(std::alloc::alloc(new_layout)) }
        } else {
            let curr_ptr = self.data;
            let old_layout = std::alloc::Layout::from_size_align(
                self.type_info.stride() * self.capacity,
                self.type_info.alignment(),
            )
            .unwrap();

            let new_ptr =
                unsafe { std::alloc::realloc(curr_ptr.as_ptr(), old_layout, new_layout_size) };
            if let Some(new_ptr) = NonNull::new(new_ptr) {
                new_ptr
            } else {
                // Copy and deallocate old contents since the allocator did not reallocate our data.
                let new_ptr = unsafe { NonNull::new_unchecked(std::alloc::alloc(new_layout)) };
                unsafe { curr_ptr.copy_to_nonoverlapping(new_ptr, new_layout_size) };
                unsafe { std::alloc::dealloc(curr_ptr.as_ptr(), old_layout) };
                new_ptr
            }
        };

        self.data = new_data_ptr;
        self.capacity = new_capacity;
    }

    pub fn clear(&mut self) {
        self.size = 0;
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn iter<T>(&self) -> DynVecIter<T> {
        DynVecIter::<T> {
            vec: self,
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn iter_mut<T>(&mut self) -> DynVecIterMut<T> {
        DynVecIterMut::<T> {
            vec: unsafe { std::ptr::from_mut(self) },
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl Drop for DynVec {
    fn drop(&mut self) {
        assert!(self.size <= self.capacity);
        for i in 0..self.size {
            let curr_ptr = unsafe { self.data.byte_add(self.type_info.stride() * i) };
            unsafe { (self.type_info.drop_fn)(curr_ptr.as_ptr()) };
        }

        if self.capacity != 0 {
            assert!(!self.data.as_ptr().is_null());
            let layout = std::alloc::Layout::from_size_align(
                self.type_info.stride() * self.capacity,
                self.type_info.alignment(),
            )
            .unwrap();
            unsafe { std::alloc::dealloc(self.data.as_ptr(), layout) };
        }
    }
}

// Safety: idk
unsafe impl Send for DynVec {}

pub struct DynVecCloneable {
    type_info: TypeInfoCloneable,
    data: NonNull<u8>,
    size: usize,
    capacity: usize,
}

impl DynVecCloneable {
    pub fn new(type_info: TypeInfoCloneable) -> Self {
        Self {
            type_info,
            data: NonNull::dangling(),
            size: 0,
            capacity: 0,
        }
    }

    pub fn push<T: Clone + 'static>(&mut self, val: T) {
        let type_info = TypeInfoCloneable::new::<T>();
        assert_eq!(self.type_info, type_info);

        let val_bytes = unsafe {
            std::slice::from_raw_parts(
                std::slice::from_ref(&val).as_ptr() as *const u8,
                type_info.size(),
            )
        };
        self.push_unchecked(val_bytes);
        std::mem::forget(val);
    }

    pub fn push_unchecked(&mut self, bytes: &[u8]) {
        if self.size == self.capacity {
            self.grow(1);
        }

        unsafe {
            let dst_ptr = self.data.byte_add(self.type_info.stride() * self.size);
            bytes
                .as_ptr()
                .copy_to_nonoverlapping(dst_ptr.as_ptr(), self.type_info.size)
        };
        self.size += 1;
    }

    pub fn get<T: Clone + 'static>(&self, index: usize) -> &T {
        let type_info = TypeInfoCloneable::new::<T>();
        assert_eq!(self.type_info, type_info);
        assert!(self.size > index, "Index is out of bounds.");

        let bytes = self.get_unchecked(index).as_ptr() as *const T;
        unsafe { bytes.as_ref().unwrap() }
    }

    pub fn get_mut<T: Clone + 'static>(&mut self, index: usize) -> &mut T {
        let type_info = TypeInfoCloneable::new::<T>();
        assert_eq!(self.type_info, type_info);
        assert!(self.size > index, "Index is out of bounds.");

        let bytes = self.get_mut_unchecked(index).as_ptr() as *mut T;
        unsafe { bytes.as_mut().unwrap() }
    }

    pub fn get_unchecked(&self, index: usize) -> &[u8] {
        assert!(index < self.size);
        let stride = self.type_info.stride();
        let start = unsafe { self.data.offset((stride * index) as isize) };
        return unsafe { std::slice::from_raw_parts(start.as_ptr(), stride) };
    }

    pub fn get_mut_unchecked(&mut self, index: usize) -> &mut [u8] {
        assert!(index < self.size);
        let stride = self.type_info.stride();
        let start = unsafe { self.data.offset((stride * index) as isize) };
        return unsafe { std::slice::from_raw_parts_mut(start.as_ptr(), stride) };
    }

    pub fn is_empty(&self) -> bool {
        return self.size == 0;
    }

    fn grow(&mut self, grow_amount: usize) {
        let new_capacity = self.capacity + grow_amount;

        let new_layout_size = self.type_info.size() * new_capacity;
        let new_layout =
            std::alloc::Layout::from_size_align(new_layout_size, self.type_info.alignment())
                .unwrap();

        let new_data_ptr = if self.capacity == 0 {
            unsafe { NonNull::new_unchecked(std::alloc::alloc(new_layout)) }
        } else {
            let curr_ptr = self.data;
            let old_layout = std::alloc::Layout::from_size_align(
                self.type_info.size() * self.capacity,
                self.type_info.alignment(),
            )
            .unwrap();

            let new_ptr =
                unsafe { std::alloc::realloc(curr_ptr.as_ptr(), old_layout, new_layout_size) };
            if let Some(new_ptr) = NonNull::new(new_ptr) {
                new_ptr
            } else {
                // Copy and deallocate old contents since the allocator did not reallocate our data.
                let new_ptr = unsafe { NonNull::new_unchecked(std::alloc::alloc(new_layout)) };
                unsafe { curr_ptr.copy_to_nonoverlapping(new_ptr, new_layout_size) };
                unsafe { std::alloc::dealloc(curr_ptr.as_ptr(), old_layout) };
                new_ptr
            }
        };

        self.data = new_data_ptr;
        self.capacity = new_capacity;
    }

    pub fn clear(&mut self) {
        self.size = 0;
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn iter<T>(&self) -> DynVecIterCloneable<T> {
        DynVecIterCloneable::<T> {
            vec: self,
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn iter_mut<T>(&mut self) -> DynVecIterMutCloneable<T> {
        DynVecIterMutCloneable::<T> {
            vec: unsafe { std::ptr::from_mut(self) },
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl Clone for DynVecCloneable {
    fn clone(&self) -> Self {
        let layout_size = self.type_info.size() * self.capacity;
        let layout =
            std::alloc::Layout::from_size_align(layout_size, self.type_info.alignment()).unwrap();
        let new_data = unsafe { NonNull::new_unchecked(std::alloc::alloc(layout)) };
        for i in 0..self.size {
            let mut new_box = unsafe {
                (self.type_info.clone_fn)(
                    self.data
                        .offset((self.type_info.stride() * i) as isize)
                        .as_ptr(),
                )
            };

            unsafe {
                new_data
                    .byte_offset((i * self.type_info.stride()) as isize)
                    .copy_from_nonoverlapping(
                        std::ptr::NonNull::new(Box::into_raw(new_box)).unwrap(),
                        self.type_info.size(),
                    )
            };
        }

        Self {
            type_info: self.type_info.clone(),
            data: new_data,
            size: self.size,
            capacity: self.capacity,
        }
    }
}

impl Drop for DynVecCloneable {
    fn drop(&mut self) {
        assert!(self.size <= self.capacity);
        for i in 0..self.size {
            let curr_ptr = unsafe { self.data.byte_add(self.type_info.size * i) };
            unsafe { (self.type_info.drop_fn)(curr_ptr.as_ptr()) };
        }

        if self.capacity != 0 {
            assert!(!self.data.as_ptr().is_null());
            let layout = std::alloc::Layout::from_size_align(
                self.type_info.size() * self.capacity,
                self.type_info.alignment(),
            )
            .unwrap();
            unsafe { std::alloc::dealloc(self.data.as_ptr(), layout) };
        }
    }
}

// Safety: idk
unsafe impl Send for DynVecCloneable {}

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct TypeInfoCloneable {
    type_id: std::any::TypeId,
    drop_fn: unsafe fn(*mut u8),
    clone_fn: unsafe fn(*const u8) -> Box<u8>,
    size: usize,
    alignment: usize,
}

impl TypeInfoCloneable {
    pub fn new<T: Clone + 'static>() -> Self {
        unsafe fn drop_fn<T>(ptr: *mut u8) {
            std::ptr::drop_in_place(ptr as *mut T);
        }

        unsafe fn clone_fn<T: Clone>(ptr: *const u8) -> Box<u8> {
            let cloned_box = Box::new((ptr as *const T).as_ref().unwrap().clone());
            return Box::from_raw(Box::into_raw(cloned_box) as *mut u8);
        }

        Self {
            type_id: std::any::TypeId::of::<T>(),
            drop_fn: drop_fn::<T>,
            clone_fn: clone_fn::<T>,
            size: std::mem::size_of::<T>(),
            alignment: std::mem::align_of::<T>(),
        }
    }

    pub fn type_id(&self) -> std::any::TypeId {
        self.type_id
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub unsafe fn drop(&self, data: *mut u8) {
        (self.drop_fn)(data);
    }

    pub fn alignment(&self) -> usize {
        self.alignment
    }

    pub fn stride(&self) -> usize {
        self.size().next_multiple_of(self.alignment)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TypeInfo {
    pub type_id: std::any::TypeId,
    drop_fn: unsafe fn(*mut u8),
    size: usize,
    alignment: usize,
}

impl PartialEq for TypeInfo {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id
    }
}

impl std::hash::Hash for TypeInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.type_id.hash(state);
    }
}

impl Eq for TypeInfo {}

impl PartialOrd for TypeInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.type_id.partial_cmp(&other.type_id)
    }
}

impl Ord for TypeInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.type_id.cmp(&other.type_id)
    }
}

impl TypeInfo {
    pub fn new<T: 'static>() -> Self {
        unsafe fn drop_fn<T>(ptr: *mut u8) {
            std::ptr::drop_in_place(ptr as *mut T);
        }

        Self {
            type_id: std::any::TypeId::of::<T>(),
            drop_fn: drop_fn::<T>,
            size: std::mem::size_of::<T>(),
            alignment: std::mem::align_of::<T>(),
        }
    }

    pub fn type_id(&self) -> std::any::TypeId {
        self.type_id
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub unsafe fn drop(&self, data: *mut u8) {
        (self.drop_fn)(data);
    }

    pub fn alignment(&self) -> usize {
        self.alignment
    }

    pub fn stride(&self) -> usize {
        let rem = self.alignment - (self.size % self.alignment);
        return self.size + rem;
    }
}

pub struct DynVecIter<'a, T> {
    vec: &'a DynVec,
    i: usize,
    _marker: std::marker::PhantomData<&'a T>,
}

impl<'a, T> DynVecIter<'a, T> {
    fn new(vec: &'a DynVec) -> Self {
        Self {
            vec,
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, T: Clone + 'static> Iterator for DynVecIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.vec.size as usize {
            return None;
        }

        let next = self.vec.get::<T>(self.i);
        self.i += 1;
        return Some(next);
    }
}

pub struct DynVecIterMut<'a, T> {
    vec: *mut DynVec,
    i: usize,
    _marker: std::marker::PhantomData<(&'a mut DynVec, &'a mut T)>,
}

impl<'a, T> DynVecIterMut<'a, T> {
    fn new(vec: &'a mut DynVec) -> Self {
        Self {
            vec,
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, T: Clone + 'static> Iterator for DynVecIterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        let vec: &'a mut DynVec = unsafe { self.vec.as_mut() }.unwrap();
        if self.i == vec.size as usize {
            return None;
        }

        let next = vec.get_mut::<T>(self.i);
        self.i += 1;
        return Some(next);
    }
}

pub struct DynVecIterCloneable<'a, T> {
    vec: &'a DynVecCloneable,
    i: usize,
    _marker: std::marker::PhantomData<&'a T>,
}

impl<'a, T> DynVecIterCloneable<'a, T> {
    fn new(vec: &'a DynVecCloneable) -> Self {
        Self {
            vec,
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, T: Clone + 'static> Iterator for DynVecIterCloneable<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.vec.size as usize {
            return None;
        }

        let next = self.vec.get::<T>(self.i);
        self.i += 1;
        return Some(next);
    }
}

pub struct DynVecIterMutCloneable<'a, T> {
    vec: *mut DynVecCloneable,
    i: usize,
    _marker: std::marker::PhantomData<(&'a mut DynVecCloneable, &'a mut T)>,
}

impl<'a, T> DynVecIterMutCloneable<'a, T> {
    fn new(vec: &'a mut DynVecCloneable) -> Self {
        Self {
            vec,
            i: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, T: Clone + 'static> Iterator for DynVecIterMutCloneable<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        let vec: &'a mut DynVecCloneable = unsafe { self.vec.as_mut() }.unwrap();
        if self.i == vec.size as usize {
            return None;
        }

        let next = vec.get_mut::<T>(self.i);
        self.i += 1;
        return Some(next);
    }
}
