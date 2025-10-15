use std::{
    collections::HashSet,
    iter::Enumerate,
    ops::{Deref, DerefMut},
    usize,
};

use log::debug;

struct FreeListNode<T> {
    generation: u32,
    data: std::mem::MaybeUninit<std::mem::ManuallyDrop<T>>,
}

impl<T> FreeListNode<T> {
    // TODO: fprobably assert that ho more that like 2bil elements go in this.
    pub const NULL_GENERATION_BIT: u32 = 1 << 31;

    pub fn new_null() -> Self {
        Self {
            generation: Self::NULL_GENERATION_BIT,
            data: std::mem::MaybeUninit::uninit(),
        }
    }

    pub fn is_null(&self) -> bool {
        return (self.generation & Self::NULL_GENERATION_BIT) > 0;
    }
}

impl<T: Clone> Clone for FreeListNode<T> {
    fn clone(&self) -> Self {
        Self {
            generation: self.generation,
            data: if self.is_null() {
                std::mem::MaybeUninit::uninit()
            } else {
                // Safety: We check if the generation is a null generation first.
                unsafe { std::mem::MaybeUninit::new(self.data.assume_init_ref().clone()) }
            },
        }
    }
}

pub struct FreeList<T> {
    data: Vec<FreeListNode<T>>,
    free: Vec<usize>,
}

impl<T: Clone> Clone for FreeList<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            free: self.free.clone(),
        }
    }
}

pub struct FreeListHandle<T> {
    index: u32,
    generation: u32,
    _marker: std::marker::PhantomData<T>,
}

impl<T> std::fmt::Debug for FreeListHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FreeListHandle")
            .field("index", &self.index())
            .field("generation", &self.index())
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T> std::hash::Hash for FreeListHandle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<T> Eq for FreeListHandle<T> {}

impl<T> PartialEq for FreeListHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Copy for FreeListHandle<T> {}

impl<T> Clone for FreeListHandle<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index,
            generation: self.generation,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> FreeListHandle<T> {
    pub fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub const DANGLING: FreeListHandle<T> = FreeListHandle::<T> {
        index: u32::MAX,
        generation: u32::MAX,
        _marker: std::marker::PhantomData,
    };

    pub fn is_null(&self) -> bool {
        *self == Self::DANGLING
    }

    pub fn as_untyped(self) -> FreeListHandle<()> {
        FreeListHandle::<()> {
            index: self.index,
            generation: self.generation,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_typed<R>(self) -> FreeListHandle<R> {
        FreeListHandle::<R> {
            index: self.index,
            generation: self.generation,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> FreeList<T> {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn push(&mut self, val: T) -> FreeListHandle<T> {
        if let Some(next_free) = self.free.pop() {
            // Safety, we set the generation on removal after the drop.
            let generation =
                self.data[next_free].generation & !FreeListNode::<T>::NULL_GENERATION_BIT;
            self.data[next_free].data =
                std::mem::MaybeUninit::new(std::mem::ManuallyDrop::new(val));
            return FreeListHandle::new(next_free as u32, generation);
        }

        self.data.push(FreeListNode {
            generation: 0,
            data: std::mem::MaybeUninit::new(std::mem::ManuallyDrop::new(val)),
        });
        return FreeListHandle::new(self.data.len() as u32 - 1, 0);
    }

    /// Custom constructed handle or previously serialized handle to insert into this freelist.
    pub fn insert_in_place(&mut self, handle: FreeListHandle<T>, val: T) {
        if handle.index as usize >= self.data.len() {
            self.data
                .resize_with(handle.index as usize + 1, || FreeListNode::new_null());
        }
        self.set(handle, val);
    }

    pub fn remove(&mut self, handle: FreeListHandle<T>) -> T {
        assert!((handle.index as usize) < self.data.len());
        self.free.push(handle.index as usize);
        let node = &mut self.data[handle.index as usize];
        assert!(!node.is_null());
        // Safety: We assert the node is not null.
        let res = unsafe { std::mem::ManuallyDrop::take(&mut node.data.assume_init_mut()) };
        node.generation = (node.generation + 1) & FreeListNode::<T>::NULL_GENERATION_BIT;
        return res;
    }

    pub fn is_free(&self, handle: FreeListHandle<T>) -> bool {
        return self.data[handle.index as usize].is_null();
    }

    pub fn get(&self, handle: FreeListHandle<T>) -> Option<&T> {
        if self.is_free(handle) {
            return None;
        }
        let r = unsafe { self.data[handle.index as usize].data.assume_init_ref() };
        return Some(r.deref());
    }

    pub fn get_mut(&mut self, handle: FreeListHandle<T>) -> Option<&mut T> {
        assert!((handle.index as usize) < self.data.len());
        if self.is_free(handle) {
            return None;
        }
        let mut r = unsafe { self.data[handle.index as usize].data.assume_init_mut() };
        return Some(r.deref_mut());
    }

    pub fn set(&mut self, handle: FreeListHandle<T>, val: T) {
        assert!((handle.index as usize) < self.data.len());
        assert!((handle.generation & FreeListNode::<T>::NULL_GENERATION_BIT) == 0);
        let mut res = &mut self.data[handle.index as usize];
        res.generation = handle.generation;
        res.data = std::mem::MaybeUninit::new(std::mem::ManuallyDrop::new(val));
    }

    /// If we did push, this is the next free handle that we would be given, this handle is not
    /// valid to use.
    pub fn next_free_handle(&self) -> FreeListHandle<T> {
        if let Some(free) = self.free.last() {
            return FreeListHandle::new(
                *free as u32,
                self.data[*free].generation & !FreeListNode::<T>::NULL_GENERATION_BIT,
            );
        }
        return FreeListHandle::new(self.data.len() as u32, 0);
    }

    pub fn iter(&self) -> FreeListIterator<'_, T> {
        FreeListIterator {
            free_list: self,
            left: 0,
        }
    }

    pub fn iter_with_handle(&self) -> FreeListIteratorHandle<'_, T> {
        FreeListIteratorHandle {
            free_list: self,
            left: 0,
        }
    }

    pub fn iter_with_handle_mut(&mut self) -> FreeListHandleIteratorMut<'_, T> {
        FreeListHandleIteratorMut {
            free_list: std::ptr::from_mut(self),
            left: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> Drop for FreeList<T> {
    fn drop(&mut self) {
        for node in &mut self.data {
            if !node.is_null() {
                // Safety: We check it is not null above.
                unsafe { std::mem::ManuallyDrop::drop(&mut node.data.assume_init_mut()) };
            }
        }
    }
}

// TODO: Clean up iterator code since it's very prone to breakage right now and is thrown together
// messily.
pub struct FreeListIterator<'a, T> {
    free_list: &'a FreeList<T>,
    left: usize,
}

impl<'a, T> Iterator for FreeListIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let free_list = self.free_list;
        if free_list.data.is_empty() || self.left >= free_list.data.len() {
            return None;
        }

        let node = &free_list.data[self.left];
        if node.is_null() {
            self.left += 1;
            return self.next();
        }

        // Safety:
        let val = unsafe { node.data.assume_init_ref() };
        self.left += 1;

        return Some(val.deref());
    }
}

pub struct FreeListIteratorHandle<'a, T> {
    free_list: &'a FreeList<T>,
    left: usize,
}

impl<'a, T> Iterator for FreeListIteratorHandle<'a, T> {
    type Item = (FreeListHandle<T>, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let free_list = self.free_list;
        if free_list.data.is_empty() || self.left >= free_list.data.len() {
            return None;
        }

        let node = &free_list.data[self.left];
        if node.is_null() {
            self.left += 1;
            return self.next();
        }

        // Safety:
        let val = unsafe { node.data.assume_init_ref() };
        let handle = FreeListHandle::new(self.left as u32, node.generation);
        self.left += 1;

        return Some((handle, val.deref()));
    }
}

pub struct FreeListHandleIteratorMut<'a, T> {
    free_list: *mut FreeList<T>,
    left: usize,
    _marker: std::marker::PhantomData<&'a mut FreeList<T>>,
}

impl<'a, T> Iterator for FreeListHandleIteratorMut<'a, T> {
    type Item = (FreeListHandle<T>, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        // Safety: Since this is a one directional iterator, we can safely borrow mutable for each
        // element.
        let free_list = unsafe { self.free_list.as_mut().unwrap() };

        if free_list.data.is_empty() || self.left >= free_list.data.len() {
            return None;
        }

        if free_list.free.iter().any(|x| *x == self.left) {
            self.left += 1;
            return self.next();
        }

        let node = &mut free_list.data[self.left];
        let val = unsafe { node.data.assume_init_mut() };
        let handle = FreeListHandle::new(self.left as u32, node.generation);
        self.left += 1;

        return Some((handle, val.deref_mut()));
    }
}
