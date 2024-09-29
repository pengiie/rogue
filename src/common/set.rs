pub trait AttributeSetImpl: Clone {
    type E: Clone;
    fn aggregate_updates(&self, last: &Self) -> Vec<Self::E>;
    fn aggregate_all_fields(&self) -> Vec<Self::E>;
}

pub struct AttributeSet<T>
where
    T: AttributeSetImpl,
{
    data: Option<T>,
    last_data: Option<T>,
    updates: Vec<T::E>,
}

impl<T> AttributeSet<T>
where
    T: AttributeSetImpl,
{
    pub fn new() -> Self {
        Self {
            data: None,
            last_data: None,
            updates: Vec::new(),
        }
    }

    pub fn refresh_updates(&mut self, new_data: &T) {
        self.updates = if let Some(last_data) = self.last_data.as_ref() {
            new_data.aggregate_updates(last_data)
        } else {
            new_data.aggregate_all_fields()
        };

        self.last_data = self.data.clone();
        self.data = Some(new_data.clone());
    }

    pub fn updates(&self) -> &Vec<T::E> {
        &self.updates
    }
}
