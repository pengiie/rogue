use std::collections::HashSet;

use nalgebra::Vector2;

pub struct Gamepad {
    gilrs: gilrs::Gilrs,

    left_axis: Vector2<f32>,
    right_axis: Vector2<f32>,

    pressed_buttons: HashSet<gilrs::Button>,
    released_buttons: HashSet<gilrs::Button>,
    down_buttons: HashSet<gilrs::Button>,

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
                    self.pressed_buttons.insert(button);
                    self.down_buttons.insert(button);
                }
                gilrs::EventType::ButtonReleased(button, _) => {
                    self.released_buttons.insert(button);
                    self.down_buttons.remove(&button);
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

    pub fn is_button_pressed(&self, button: gilrs::Button) -> bool {
        self.pressed_buttons.contains(&button)
    }

    pub fn is_button_down(&self, button: gilrs::Button) -> bool {
        self.down_buttons.contains(&button)
    }

    pub fn is_button_released(&self, button: gilrs::Button) -> bool {
        self.released_buttons.contains(&button)
    }
}
