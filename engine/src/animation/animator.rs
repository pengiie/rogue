use rogue_macros::game_component;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "Animator")]
#[serde(default)]
pub struct Animator {}

impl Animator {
    pub fn new() -> Self {
        Self {}
    }

    pub fn play_animtion(&mut self, animation_name: &str) {}
}

impl Default for Animator {
    fn default() -> Self {
        Self::new()
    }
}
