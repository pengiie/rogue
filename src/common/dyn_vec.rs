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

    pub fn push<T: 'static>(&mut self, val: T) {
        let type_info = TypeInfo::new::<T>();
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
            let dst_ptr = self.data.byte_add(self.type_info.size * self.size);
            bytes
                .as_ptr()
                .copy_to_nonoverlapping(dst_ptr.as_ptr(), self.type_info.size)
        };
        self.size += 1;
    }

    pub fn get<T: 'static>(&self, index: usize) -> &T {
        let type_info = TypeInfo::new::<T>();
        assert_eq!(self.type_info, type_info);
        assert!(self.size > index, "Index is out of bounds.");

        let bytes = self.get_unchecked(index).as_ptr() as *const T;
        unsafe { bytes.as_ref().unwrap() }
    }

    pub fn get_unchecked(&self, index: usize) -> &[u8] {
        todo!()
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
            if new_ptr == std::ptr::null_mut() {
                curr_ptr
            } else {
                // Copy and deallocate old contents since the allocator did realloc our data.
                let new_ptr = unsafe { NonNull::new_unchecked(new_ptr) };
                unsafe { curr_ptr.copy_to_nonoverlapping(new_ptr, new_layout_size) };
                unsafe { std::alloc::dealloc(curr_ptr.as_ptr(), old_layout) };

                new_ptr
            }
        };

        self.data = new_data_ptr;
        self.capacity = new_capacity;
    }

    pub fn size(&self) -> usize {
        self.size
    }
}

impl Clone for DynVec {
    fn clone(&self) -> Self {
        Self {
            type_info: self.type_info.clone(),
            data: todo!(),
            size: self.size.clone(),
            capacity: self.capacity.clone(),
        }
    }
}

impl Drop for DynVec {
    fn drop(&mut self) {
        for i in 0..self.size {
            let curr_ptr = unsafe { self.data.byte_add(self.type_info.size * i) };
            unsafe { (self.type_info.drop_fn)(curr_ptr.as_ptr()) };
        }

        if self.capacity != 0 {
            let layout = std::alloc::Layout::from_size_align(
                self.type_info.size() * self.capacity,
                self.type_info.alignment(),
            )
            .unwrap();
            unsafe { std::alloc::dealloc(self.data.as_ptr(), layout) };
        }
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct TypeInfo {
    type_id: std::any::TypeId,
    drop_fn: unsafe fn(*mut u8),
    size: usize,
    alignment: usize,
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
        self.size().next_multiple_of(self.alignment)
    }
}
