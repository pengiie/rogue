use std::{
    any::TypeId,
    borrow::BorrowMut,
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use rogue_macros::Resource;

use crate::common::archetype::TypeInfo;

use super::resource::ResMut;

#[derive(Resource)]
pub struct Events {
    events: HashMap<TypeId, Vec<u8>>,
    event_type_info: HashMap<TypeId, TypeInfo>,
}

impl Events {
    pub fn new() -> Self {
        Self {
            events: HashMap::new(),
            event_type_info: HashMap::new(),
        }
    }

    pub fn push<T: 'static>(&mut self, event: T) {
        let type_id = TypeId::of::<T>();
        let type_info = self
            .event_type_info
            .entry(type_id)
            .or_insert(TypeInfo::new::<T>())
            .clone();
        let event_data = self.events.entry(type_id).or_insert(Vec::new());
        // TODO: Worry about alignment, this will surely come back to bite me later.
        // TODO: Copy drop function because we are going to memory leak every frame.
        let i = event_data.len();
        event_data.resize(event_data.len() + type_info.size() as usize, 0);

        // Safety: The offset of i bytes works because we resize event_data above so the copy is
        // also safe.
        unsafe {
            let event_data_ptr = event_data.as_mut_slice().as_mut_ptr().offset(i as isize);
            event_data_ptr.copy_from(
                std::ptr::from_ref(&event) as *const u8,
                type_info.size() as usize,
            );
        }
    }

    /// Iterates of all events of type T submitted since the beginning of this frame.
    pub fn iter<T: 'static>(&self) -> EventIter<'_, T> {
        EventIter::<T>::new(self)
    }

    /// Clears all cached events from the previous frame.
    pub fn clear_events(mut events: ResMut<Events>) {
        let type_info_map = events.event_type_info.clone();
        for (type_info, event_data) in events
            .events
            .iter_mut()
            .map(|(type_id, event_data)| (type_info_map.get(type_id).unwrap(), event_data))
        {
            // TODO: Call the drop function for this type_info on each entry here, since right now
            // we are leaking memory :p.
            event_data.clear();
        }
    }
}

pub struct EventIter<'a, T> {
    event_data: Option<&'a [u8]>,
    type_info: TypeInfo,
    index: usize,
    _marker: std::marker::PhantomData<&'a T>,
}

impl<'a, T> EventIter<'a, T> {
    pub fn new(events: &'a Events) -> Self
    where
        T: 'static,
    {
        let type_info = TypeInfo::new::<T>();
        let event_data = events.events.get(&TypeId::of::<T>()).map(|d| d.as_slice());

        Self {
            event_data,
            type_info,
            index: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, T> Iterator for EventIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(event_data) = self.event_data else {
            // No events of this type were issued this frame.
            return None;
        };

        let byte_index = self.index * self.type_info.size() as usize;
        if byte_index >= event_data.len() {
            // We have reached the end of the event buffer.
            return None;
        }

        // Safety: We ensure byte_index..(byte_index + size_of::<T>()) is within the range of
        // event_data. We can safely cast to T since TODO: all T's in event_data are aligned and
        // valid byte data for T.
        assert!(byte_index + self.type_info.size() as usize <= event_data.len());
        let ptr = unsafe { event_data.as_ptr().offset(byte_index as isize) } as *const T;
        let t_ref = unsafe { ptr.as_ref().unwrap() };

        self.index += 1;

        Some(t_ref)
    }
}
