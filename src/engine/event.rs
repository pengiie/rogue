use std::{
    any::TypeId,
    array,
    borrow::BorrowMut,
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use rogue_macros::Resource;

use crate::common::dyn_vec::{DynVec, TypeInfo, TypeInfoCloneable};

use super::resource::ResMut;

pub type EventId = u32;

struct EventBank {
    // Alternates between 0 and 1.
    pub curr_frame_index: u32,
    // Monotically increasing id starting from 1.
    event_id_tracker: EventId,
    // One for every other frame.
    data: [(/*first_event_id_in_vec*/ EventId, DynVec); 2],
}

pub trait Event: Clone + 'static {}
impl<T: Clone + 'static> Event for T {}

impl EventBank {
    pub fn new<T: Event>(curr_frame_index: u32) -> Self {
        Self {
            curr_frame_index,
            event_id_tracker: 1,
            data: array::from_fn(|_| (0, DynVec::new(TypeInfoCloneable::new::<T>()))),
        }
    }

    pub fn push<T: Event>(&mut self, event: T) {
        let event_id = self.event_id_tracker;
        let (first_event_id, vec) = &mut self.data[self.curr_frame_index as usize];
        if vec.is_empty() {
            *first_event_id = event_id;
        }
        self.event_id_tracker += 1;
        vec.push(event);
    }

    pub fn clear(&mut self) {
        let (first_event_id, vec) = &mut self.data[self.curr_frame_index as usize];
        vec.clear();
    }
}

pub struct EventReader<T: Event> {
    last_event_id: EventId,
    // Marker since the event id is specific to the event type.
    marker: std::marker::PhantomData<T>,
}

impl<T: Event> EventReader<T> {
    /// Will read double events in the case that the producer runs before this reader in the game
    /// loop. Keep that in mind :p.
    pub fn new() -> Self {
        Self {
            last_event_id: 0,
            marker: std::marker::PhantomData,
        }
    }

    pub fn read<'a>(&'a mut self, events: &'a Events) -> EventReaderIter<T> {
        let event_bank = events.banks.get(&std::any::TypeId::of::<T>());
        let last_event_id = self.last_event_id;
        EventReaderIter {
            event_reader: self,
            event_bank,
            curr_event_id: last_event_id + 1,
            // Start with the opposite since it will have lower ids.
            curr_vec_index: (events.curr_frame_index + 1) % 2,
            marker: std::marker::PhantomData,
        }
    }
}

pub struct EventReaderIter<'a, 'b, T: Event> {
    event_reader: &'b mut EventReader<T>,
    event_bank: Option<&'a EventBank>,
    curr_event_id: EventId,
    curr_vec_index: u32,
    marker: std::marker::PhantomData<&'a T>,
}

impl<'a, 'b, T: Event> Iterator for EventReaderIter<'a, 'b, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(event_bank) = self.event_bank else {
            return None;
        };

        let (first_event_id, data) = &event_bank.data[self.curr_vec_index as usize];
        if self.curr_event_id < *first_event_id {
            self.curr_event_id = *first_event_id;
        }
        let index = self.curr_event_id - *first_event_id;
        if index >= data.len() as u32 {
            self.curr_vec_index = (self.curr_vec_index + 1) % 2;
            // If we come back to the same event bank we started with then we are done.
            if self.curr_vec_index != event_bank.curr_frame_index {
                return None;
            }
            return self.next();
        }
        self.event_reader.last_event_id = self.curr_event_id;
        self.curr_event_id += 1;
        return Some(data.get(index as usize));
    }
}

/// Stores abitrary events to make message passing between systems easier. These events are not
/// actually observer based, instead events are stored in buffer which can be queried and is also
/// cleared at the end of every frame. This prevents the dreaded `Arc<RwLock<_>>` that comes with
/// the observer pattern in rust and keeps event consumption more deterministic.
// TODO: Event tracking system so each event type has a monotic tracking id, consumers of events
// can use that id and track to their own id to see if they consumed the event already. Then we can
// persist events for two frames in the case that the consumer comes before the producer in the
// game loop.
#[derive(Resource)]
pub struct Events {
    banks: HashMap<TypeId, EventBank>,
    event_type_info: HashMap<TypeId, TypeInfo>,
    // Alternating index between 0 and 1.
    curr_frame_index: u32,
}

impl Events {
    pub fn new() -> Self {
        Self {
            banks: HashMap::new(),
            event_type_info: HashMap::new(),
            curr_frame_index: 0,
        }
    }

    pub fn frame_cleanup(mut events: ResMut<Events>) {
        let events: &mut Events = &mut events;
        // Update the frame index for all the event banks.
        events.curr_frame_index = (events.curr_frame_index + 1) % 2;
        for (event_type_id, bank) in events.banks.iter_mut() {
            bank.curr_frame_index = events.curr_frame_index;
            let (_, data) = &mut bank.data[events.curr_frame_index as usize];
            data.clear();
        }
    }

    pub fn push<T: Event>(&mut self, event: T) {
        let type_id = TypeId::of::<T>();
        let type_info = self
            .event_type_info
            .entry(type_id)
            .or_insert(TypeInfo::new::<T>())
            .clone();
        let event_bank = self
            .banks
            .entry(type_id)
            .or_insert(EventBank::new::<T>(self.curr_frame_index));
        event_bank.push(event);
    }
}
