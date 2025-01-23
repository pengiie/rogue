use std::{
    iter::Enumerate,
    ops::{Deref, DerefMut},
    usize,
};

use log::debug;

union FreeListNode<T> {
    next_free: usize,
    data: std::mem::ManuallyDrop<T>,
}

// TODO: Make a new free list variant that stores free nodes with a separate
// HashSet so we can easily tell which nodes have been freed or not.
pub struct FreeList<T> {
    data: Vec<FreeListNode<T>>,
    free_head: usize,
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
            free_head: std::usize::MAX,
        }
    }

    pub fn push(&mut self, val: T) -> FreeListHandle<T> {
        if self.free_head == std::usize::MAX {
            self.data.push(FreeListNode {
                data: std::mem::ManuallyDrop::new(val),
            });
            return FreeListHandle::new(self.data.len() - 1);
        }

        let new_pos = self.free_head;
        let next_free = unsafe { self.data[self.free_head].next_free };
        self.free_head = next_free;

        self.data[new_pos] = FreeListNode {
            data: std::mem::ManuallyDrop::new(val),
        };
        FreeListHandle::new(new_pos)
    }

    pub fn remove(&mut self, handle: FreeListHandle<T>) -> T {
        assert!(handle.index < self.data.len());

        let val = unsafe { std::mem::ManuallyDrop::take(&mut self.data[handle.index].data) };

        // The free head is after the node we just removed, if add, remove, and add index 0,
        // self.free_head starts as a usize::MAX so next_free is set correctly to usize::MAX.
        if handle.index < self.free_head {
            self.data[handle.index].next_free = self.free_head;
            self.free_head = handle.index;
            return val;
        }

        let mut prev_left = self.free_head;
        let mut left = self.free_head;
        while left < handle.index {
            prev_left = self.free_head;
            left = unsafe { self.data[left].next_free };
        }

        self.data[prev_left].next_free = handle.index;
        self.data[handle.index].next_free = left;

        return val;
    }

    // TODO: Make this safe with an Option.
    pub fn get(&self, handle: FreeListHandle<T>) -> &T {
        unsafe { &self.data[handle.index].data }
    }

    pub fn get_mut(&mut self, handle: FreeListHandle<T>) -> &mut T {
        unsafe { &mut self.data[handle.index].data }
    }

    /// If we did push, this is the next free handle that we would be given, this handle is not
    /// valid to use.
    pub fn next_free_handle(&self) -> FreeListHandle<T> {
        return FreeListHandle::new(self.free_head.min(self.data.len()));
    }

    pub fn iter(&self) -> FreeListIterator<'_, T> {
        FreeListIterator {
            free_list: self,
            left: 0,
            right: self.free_head.min(self.data.len().saturating_sub(1)),
        }
    }

    pub fn iter_with_handle(&mut self) -> FreeListHandleIteratorMut<'_, T> {
        FreeListHandleIteratorMut {
            free_list: std::ptr::from_mut(self),
            left: 0,
            right: self.free_head.min(self.data.len().saturating_sub(1)),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> Drop for FreeList<T> {
    fn drop(&mut self) {
        let mut left = 0;
        let mut right = self.free_head.min(self.data.len());
        while left < self.data.len() && right <= self.data.len() {
            for i in left..right {
                unsafe { std::mem::ManuallyDrop::drop(&mut self.data[i].data) };
            }
            if right == self.data.len() {
                break;
            }

            left = right + 1;
            let next_free = unsafe { self.data[right].next_free };
            right = next_free.min(self.data.len());
        }
    }
}

// TODO: Clean up iterator code since it's very prone to breakage right now and is thrown together
// messily.
pub struct FreeListIterator<'a, T> {
    free_list: &'a FreeList<T>,
    left: usize,
    right: usize,
}

impl<'a, T> Iterator for FreeListIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let free_list = self.free_list;
        if free_list.data.is_empty() || self.left >= free_list.data.len() {
            return None;
        }

        if self.left == self.right {
            let mut last_free = self.right;
            let mut next_free = unsafe { free_list.data[self.right].next_free };
            while last_free + 1 == next_free {
                last_free = next_free;
                next_free = unsafe { free_list.data[next_free].next_free };
            }

            // Check if there are more items to iterate.
            self.left = last_free + 1;
            if self.left >= free_list.data.len() {
                return None;
            }

            self.right = next_free.min(free_list.data.len().saturating_sub(1));
        }

        let val = unsafe { free_list.data[self.left].data.deref() };
        self.left += 1;

        return Some(val);
    }
}

pub struct FreeListHandleIteratorMut<'a, T> {
    free_list: *mut FreeList<T>,
    left: usize,
    right: usize,
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

        if self.left == self.right {
            let mut last_free = self.right;
            let mut next_free = unsafe { free_list.data[self.right].next_free };
            while last_free + 1 == next_free {
                last_free = next_free;
                next_free = unsafe { free_list.data[next_free].next_free };
            }

            // Check if there are more items to iterate.
            self.left = last_free + 1;
            if self.left >= free_list.data.len() {
                return None;
            }

            self.right = next_free.min(free_list.data.len().saturating_sub(1));
        }

        let val = unsafe { free_list.data[self.left].data.deref_mut() };
        let handle = FreeListHandle::new(self.left);
        self.left += 1;

        return Some((handle, val));
    }
}
