use std::collections::HashSet;

use nalgebra::Vector2;

pub struct Gamepad {
    gilrs: gilrs::Gilrs,

    left_axis: Vector2<f32>,
    right_axis: Vector2<f32>,

    pressed_buttons: HashSet<Button>,
    released_buttons: HashSet<Button>,
    down_buttons: HashSet<Button>,

    pub deadzone: f32,
}

impl Gamepad {
    pub fn new() -> Self {
        Self {
            gilrs: gilrs::GilrsBuilder::new()
                .build()
                .expect("Failed to initialize gamepad context."),
            left_axis: Vector2::new(0.0, 0.0),
            right_axis: Vector2::new(0.0, 0.0),
            pressed_buttons: HashSet::new(),
            released_buttons: HashSet::new(),
            down_buttons: HashSet::new(),
            deadzone: 0.1,
        }
    }

    pub fn collect_events(&mut self) {
        while let Some(event) = self.gilrs.next_event() {
            match event.event {
                gilrs::EventType::ButtonPressed(button, _) => {
                    self.pressed_buttons.insert(button.into());
                    self.down_buttons.insert(button.into());
                }
                gilrs::EventType::ButtonReleased(button, _) => {
                    self.released_buttons.insert(button.into());
                    self.down_buttons.remove(&button.into());
                }
                gilrs::EventType::AxisChanged(axis, value, _) => match axis {
                    gilrs::Axis::LeftStickX => {
                        self.left_axis.x = value;
                    }
                    gilrs::Axis::LeftStickY => {
                        self.left_axis.y = value;
                    }
                    gilrs::Axis::RightStickX => {
                        self.right_axis.x = value;
                    }
                    gilrs::Axis::RightStickY => {
                        self.right_axis.y = value;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    pub fn clear_inputs(&mut self) {
        self.pressed_buttons.clear();
        self.released_buttons.clear();
    }

    pub fn left_axis(&self) -> &Vector2<f32> {
        &self.left_axis
    }

    pub fn right_axis(&self) -> &Vector2<f32> {
        &self.right_axis
    }

    pub fn is_button_pressed(&self, button: Button) -> bool {
        self.pressed_buttons.contains(&button)
    }

    pub fn is_button_down(&self, button: Button) -> bool {
        self.down_buttons.contains(&button)
    }

    pub fn is_button_released(&self, button: Button) -> bool {
        self.released_buttons.contains(&button)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
/// Copied from gilrs::Button.
///
/// Gamepad's elements which state can be represented by value from 0.0 to 1.0.
///
/// ![Controller layout](https://gilrs-project.gitlab.io/gilrs/img/controller.svg)
pub enum Button {
    #[default]
    Unknown,
    // Action Pad
    South,
    East,
    North,
    West,
    C,
    Z,
    // Triggers
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    // Menu Pad
    Select,
    Start,
    Mode,
    // Sticks
    LeftThumb,
    RightThumb,
    // D-Pad
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl From<gilrs::Button> for Button {
    fn from(button: gilrs::Button) -> Self {
        match button {
            gilrs::Button::South => Button::South,
            gilrs::Button::East => Button::East,
            gilrs::Button::North => Button::North,
            gilrs::Button::West => Button::West,
            gilrs::Button::C => Button::C,
            gilrs::Button::Z => Button::Z,
            gilrs::Button::LeftTrigger => Button::LeftTrigger,
            gilrs::Button::LeftTrigger2 => Button::LeftTrigger2,
            gilrs::Button::RightTrigger => Button::RightTrigger,
            gilrs::Button::RightTrigger2 => Button::RightTrigger2,
            gilrs::Button::Select => Button::Select,
            gilrs::Button::Start => Button::Start,
            gilrs::Button::Mode => Button::Mode,
            gilrs::Button::LeftThumb => Button::LeftThumb,
            gilrs::Button::RightThumb => Button::RightThumb,
            gilrs::Button::DPadUp => Button::DPadUp,
            gilrs::Button::DPadDown => Button::DPadDown,
            gilrs::Button::DPadLeft => Button::DPadLeft,
            gilrs::Button::DPadRight => Button::DPadRight,
            gilrs::Button::Unknown => Button::Unknown,
        }
    }
}

impl From<Button> for gilrs::Button {
    fn from(button: Button) -> Self {
        match button {
            Button::Unknown => gilrs::Button::Unknown,
            Button::South => gilrs::Button::South,
            Button::East => gilrs::Button::East,
            Button::North => gilrs::Button::North,
            Button::West => gilrs::Button::West,
            Button::C => gilrs::Button::C,
            Button::Z => gilrs::Button::Z,
            Button::LeftTrigger => gilrs::Button::LeftTrigger,
            Button::LeftTrigger2 => gilrs::Button::LeftTrigger2,
            Button::RightTrigger => gilrs::Button::RightTrigger,
            Button::RightTrigger2 => gilrs::Button::RightTrigger2,
            Button::Select => gilrs::Button::Select,
            Button::Start => gilrs::Button::Start,
            Button::Mode => gilrs::Button::Mode,
            Button::LeftThumb => gilrs::Button::LeftThumb,
            Button::RightThumb => gilrs::Button::RightThumb,
            Button::DPadUp => gilrs::Button::DPadUp,
            Button::DPadDown => gilrs::Button::DPadDown,
            Button::DPadLeft => gilrs::Button::DPadLeft,
            Button::DPadRight => gilrs::Button::DPadRight,
        }
    }
}
