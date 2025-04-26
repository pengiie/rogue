use std::collections::HashSet;

use nalgebra::Vector2;

pub struct Mouse {
    position: Vector2<f32>,
    delta: Vector2<f32>,
    pressed_buttons: HashSet<Button>,
    down_buttons: HashSet<Button>,
    released_buttons: HashSet<Button>,

    // Locked to center of screen and is invisible/confined.
    pub is_locked: bool,
    screen_center: Vector2<f32>,
}

impl Mouse {
    pub fn new() -> Self {
        Self {
            position: Vector2::new(0.0, 0.0),
            delta: Vector2::new(0.0, 0.0),
            pressed_buttons: HashSet::new(),
            down_buttons: HashSet::new(),
            released_buttons: HashSet::new(),

            is_locked: false,
            screen_center: Vector2::new(0.0, 0.0),
        }
    }

    pub fn clear_inputs(&mut self) {
        self.pressed_buttons.clear();
        self.released_buttons.clear();
        self.delta = Vector2::new(0.0, 0.0);
    }

    pub fn update_screen_center(&mut self, screen_size: Vector2<f32>) {
        self.screen_center = screen_size * 0.5;
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
                self.position = Vector2::new(x, y);
            }
            SubmitInput::Delta(x, y) => {
                self.delta.x += x;
                self.delta.y -= y;
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

    pub fn mouse_position(&self) -> Vector2<f32> {
        if self.is_locked {
            return self.screen_center;
        }
        self.position
    }

    pub fn mouse_delta(&self) -> Vector2<f32> {
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
