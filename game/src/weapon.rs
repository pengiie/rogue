use rogue_macros::game_component;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "Weapon")]
pub struct WeaponComponent {}

impl Default for WeaponComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl WeaponComponent {
    pub fn new() -> Self {
        Self {}
    }
}
