pub mod player;

pub struct GameEntity {
    pub uuid: uuid::Uuid,
    pub name: String,
}

impl GameEntity {
    pub fn new(name: impl ToString) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            name: name.to_string(),
        }
    }
}
