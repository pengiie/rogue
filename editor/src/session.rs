use rogue_macros::Resource;

#[derive(Resource)]
pub struct Session {}

impl Session {
    pub fn new() -> Self {
        Self {}
    }
}
