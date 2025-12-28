use core::panic;
use std::{mem::MaybeUninit, usize};

use log::debug;

pub struct RingQueue<T> {
    left: usize,
    right: usize,
    size: usize,
    init_size: usize,
    buffer: Box<[MaybeUninit<T>]>,
}

impl<T> RingQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "Ring Queue capacity must be atleast 1.");
        Self {
            left: 0,
            right: 0,
            size: 0,
            init_size: 0,
            // TODO: Use Box::new_uninit_slice once I can debug in rust 1.83.0.
            buffer: (0..capacity)
                .map(|_| MaybeUninit::uninit())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    pub fn push(&mut self, item: T) {
        if self.is_full() {
            panic!("Can't push when full");
        }
        let mut slot = &mut self.buffer[self.right];
        slot.write(item);
        //     S
        //     R
        //   L
        // 0 1 0 0
        self.right = self.right.wrapping_add(1) % self.capacity();
        self.size += 1;
    }

    pub fn try_pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        let maybe_uninit_data = &self.buffer[self.left];
        // Safety: We increment the left pointer so we know not to drop this
        let init_data = unsafe { maybe_uninit_data.assume_init_read() };
        self.left = self.left.wrapping_add(1) % self.capacity();
        self.size -= 1;
        return Some(init_data);
    }

    pub fn size(&self) -> usize {
        return self.size;
    }

    pub fn is_empty(&self) -> bool {
        return self.size == 0;
    }

    pub fn is_full(&self) -> bool {
        return self.size == self.capacity();
    }

    pub fn peek(&self) -> &T {
        if self.is_empty() {
            panic!("Can't peek an empty buffer");
        }

        // Safety: Checked if the buffer is empty above.
        unsafe { self.buffer[self.left].assume_init_ref() }
    }

    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }
}
