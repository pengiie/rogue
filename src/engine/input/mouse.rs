use std::collections::HashSet;

pub struct Mouse {
    position: (f32, f32),
    delta: (f32, f32),
    pressed_buttons: HashSet<Button>,
    down_buttons: HashSet<Button>,
    released_buttons: HashSet<Button>,
}

impl Mouse {
    pub fn new() -> Self {
        Self {
            position: (0.0, 0.0),
            delta: (0.0, 0.0),
            pressed_buttons: HashSet::new(),
            down_buttons: HashSet::new(),
            released_buttons: HashSet::new(),
        }
    }

    pub fn clear_inputs(&mut self) {
        self.pressed_buttons.clear();
        self.released_buttons.clear();
        self.delta = (0.0, 0.0);
    }

    pub fn submit_input(&mut self, input: SubmitInput) {
        match input {
            SubmitInput::Pressed(button) => {
                self.pressed_buttons.insert(button);
                self.down_buttons.insert(button);
            }
            SubmitInput::Released(button) => {
                self.released_buttons.insert(button);
                self.down_buttons.remove(&button);
            }
            SubmitInput::Position(x, y) => {
                self.position = (x, y);
            }
            SubmitInput::Delta(x, y) => {
                self.delta = (self.delta.0 + x, self.delta.1 + y);
            }
        }
    }

    pub fn is_mouse_button_pressed(&self, button: Button) -> bool {
        self.pressed_buttons.contains(&button)
    }

    pub fn is_mouse_button_down(&self, button: Button) -> bool {
        self.down_buttons.contains(&button)
    }

    pub fn is_mouse_button_released(&self, button: Button) -> bool {
        self.released_buttons.contains(&button)
    }

    pub fn mouse_position(&self) -> (f32, f32) {
        self.position
    }

    pub fn mouse_delta(&self) -> (f32, f32) {
        self.delta
    }
}

pub enum SubmitInput {
    Pressed(Button),
    Released(Button),
    Position(f32, f32),
    Delta(f32, f32),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    pub fn from_winit_button(button: &winit::event::MouseButton) -> Option<Self> {
        match button {
            winit::event::MouseButton::Left => Some(Self::Left),
            winit::event::MouseButton::Right => Some(Self::Right),
            winit::event::MouseButton::Middle => Some(Self::Middle),
            _ => None,
        }
    }
}
