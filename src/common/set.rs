use std::collections::HashSet;

use log::debug;

pub trait AttributeSetImpl: Clone {
    type E: Clone + std::hash::Hash;
    fn aggregate_updates(&self, last: &Self) -> HashSet<Self::E>;
    fn aggregate_all_fields(&self) -> HashSet<Self::E>;
}

pub struct AttributeSet<T>
where
    T: AttributeSetImpl,
{
    data: Option<T>,
    updates: HashSet<T::E>,
}

impl<T> AttributeSet<T>
where
    T: AttributeSetImpl,
{
    pub fn new() -> Self {
        Self {
            data: None,
            updates: HashSet::new(),
        }
    }

    pub fn refresh_updates(&mut self, new_data: &T) {
        self.updates = if let Some(last_data) = self.data.as_ref() {
            new_data.aggregate_updates(last_data)
        } else {
            HashSet::new()
        };

        self.data = Some(new_data.clone());
    }

    pub fn updates(&self) -> &HashSet<T::E> {
        &self.updates
    }
}
