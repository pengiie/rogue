use std::collections::HashMap;

use super::keyboard::Key;

pub struct Keybinds {
    pub pressed_key_mappings: HashMap</*action=*/ String, Key>,
}

impl Keybinds {
    pub fn new() -> Self {
        Self {
            pressed_key_mappings: HashMap::new(),
        }
    }

    pub fn register_key(&mut self, action_name: impl ToString, key: Key) {
        self.pressed_key_mappings
            .insert(action_name.to_string(), key);
    }
}
