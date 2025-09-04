use std::{
    collections::HashSet,
    iter::Enumerate,
    ops::{Deref, DerefMut},
    usize,
};

use log::debug;

struct FreeListNode<T> {
    data: std::mem::ManuallyDrop<T>,
}

impl<T: Clone> Clone for FreeListNode<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
        }
    }
}

// TODO: Make a new free list variant that stores free nodes with a separate
// HashSet so we can easily tell which nodes have been freed or not.
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
    index: usize,
    _marker: std::marker::PhantomData<T>,
}

impl<T> std::fmt::Debug for FreeListHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FreeListHandle")
            .field("index", &self.index())
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
        self.index == other.index
    }
}

impl<T> Copy for FreeListHandle<T> {}

impl<T> Clone for FreeListHandle<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> FreeListHandle<T> {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn index(&self) -> usize {
        self.index
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
            self.data[next_free] = FreeListNode {
                data: std::mem::ManuallyDrop::new(val),
            };
            return FreeListHandle::new(next_free);
        }

        self.data.push(FreeListNode {
            data: std::mem::ManuallyDrop::new(val),
        });
        return FreeListHandle::new(self.data.len() - 1);
    }

    pub fn remove(&mut self, handle: FreeListHandle<T>) -> T {
        assert!(handle.index < self.data.len());
        self.free.push(handle.index);
        return unsafe { std::mem::ManuallyDrop::take(&mut self.data[handle.index].data) };
    }

    pub fn is_free(&self, handle: FreeListHandle<T>) -> bool {
        return self.free.iter().any(|x| *x == handle.index);
    }

    pub fn get(&self, handle: FreeListHandle<T>) -> Option<&T> {
        if self.is_free(handle) {
            return None;
        }
        return Some(unsafe { &self.data[handle.index].data });
    }

    pub fn get_mut(&mut self, handle: FreeListHandle<T>) -> Option<&mut T> {
        assert!(handle.index < self.data.len());
        if self.is_free(handle) {
            return None;
        }
        return Some(unsafe { &mut self.data[handle.index].data });
    }

    /// If we did push, this is the next free handle that we would be given, this handle is not
    /// valid to use.
    pub fn next_free_handle(&self) -> FreeListHandle<T> {
        return FreeListHandle::new(*self.free.last().unwrap_or(&self.data.len()));
    }

    pub fn iter(&self) -> FreeListIterator<'_, T> {
        FreeListIterator {
            free_list: self,
            left: 0,
        }
    }

    pub fn iter_with_handle(&mut self) -> FreeListHandleIteratorMut<'_, T> {
        FreeListHandleIteratorMut {
            free_list: std::ptr::from_mut(self),
            left: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> Drop for FreeList<T> {
    fn drop(&mut self) {
        for i in 0..self.data.len() {
            if !self.free.iter().any(|x| *x == i) {
                unsafe { std::mem::ManuallyDrop::drop(&mut self.data[i].data) };
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

        if free_list.free.iter().any(|x| *x == self.left) {
            self.left += 1;
            return self.next();
        }

        let val = unsafe { free_list.data[self.left].data.deref() };
        self.left += 1;

        return Some(val);
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

        let val = unsafe { free_list.data[self.left].data.deref_mut() };
        let handle = FreeListHandle::new(self.left);
        self.left += 1;

        return Some((handle, val));
    }
}
