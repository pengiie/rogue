pub mod player;

pub enum GameEntityType {
    Player,
}

pub struct GameEntity {
    pub uuid: uuid::Uuid,
    pub name: String,
    pub entity_type: GameEntityType,
}

impl GameEntity {
    pub fn new(entity_type: GameEntityType) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            name: "test".to_owned(),
            entity_type,
        }
    }

    pub fn set_name(mut self, name: impl ToString) -> Self {
        self.name = name.to_string();
        self
    }
}
