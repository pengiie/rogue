use std::collections::HashMap;

use super::keyboard::Key;

pub struct Keybinds {
    pressed_key_mappings: HashMap<Key, String>,
}

impl Keybinds {
    pub fn new() -> Self {
        Self {
            pressed_key_mappings: HashMap::new(),
        }
    }

    pub fn set_key_pressed(&mut self, key: Key, action: impl ToString) {
        self.pressed_key_mappings.insert(key, action.to_string());
    }
}
