use rogue_macros::Resource;

use crate::engine::resource::ResMut;

use winit::event::DeviceEvent as WinitDeviceEvent;
use winit::event::DeviceId as WinitDeviceId;

use super::{
    keyboard::{self, Keyboard},
    mouse::{self, Mouse},
};

#[derive(Resource)]
pub struct Input {
    keyboard: Keyboard,
    mouse: Mouse,
}

impl Input {
    pub fn new() -> Self {
        Self {
            keyboard: Keyboard::new(),
            mouse: Mouse::new(),
        }
    }

    pub fn clear_inputs(mut input: ResMut<Input>) {
        input.keyboard.clear_inputs();
        input.mouse.clear_inputs();
    }

    // General Input
    pub fn horizontal_axis(&self) -> f32 {
        let mut axis = 0.0;
        if self.keyboard.is_key_down(keyboard::Key::A) {
            axis -= 1.0;
        }
        if self.keyboard.is_key_down(keyboard::Key::D) {
            axis += 1.0;
        }
        axis
    }

    pub fn vertical_axis(&self) -> f32 {
        let mut axis = 0.0;
        if self.keyboard.is_key_down(keyboard::Key::S) {
            axis -= 1.0;
        }
        if self.keyboard.is_key_down(keyboard::Key::W) {
            axis += 1.0;
        }
        axis
    }

    // Keyboard functions
    pub fn is_key_pressed(&self, key: keyboard::Key) -> bool {
        self.keyboard.is_key_pressed(key)
    }

    pub fn is_key_pressed_with_modifiers(
        &self,
        key: keyboard::Key,
        modifiers: &[keyboard::Modifier],
    ) -> bool {
        self.keyboard.is_key_pressed_with_modifiers(key, modifiers)
    }

    pub fn is_key_down_with_modifiers(
        &self,
        key: keyboard::Key,
        modifiers: &[keyboard::Modifier],
    ) -> bool {
        self.keyboard.is_key_down_with_modifiers(key, modifiers)
    }

    pub fn is_key_released_with_modifiers(
        &self,
        key: keyboard::Key,
        modifiers: &[keyboard::Modifier],
    ) -> bool {
        self.keyboard.is_key_released_with_modifiers(key, modifiers)
    }

    pub fn is_key_down(&self, key: keyboard::Key) -> bool {
        self.keyboard.is_key_down(key)
    }

    /// Returns true if the key is being viewed as held by the OS.
    /// Mainly used for text input.
    pub fn is_key_repeat(&self, key: keyboard::Key) -> bool {
        self.keyboard.is_key_repeat(key)
    }

    pub fn is_key_released(&self, key: keyboard::Key) -> bool {
        self.keyboard.is_key_released(key)
    }

    // Mouse functions
    pub fn is_mouse_button_pressed(&self, button: mouse::Button) -> bool {
        self.mouse.is_mouse_button_pressed(button)
    }

    pub fn is_mouse_button_down(&self, button: mouse::Button) -> bool {
        self.mouse.is_mouse_button_down(button)
    }

    pub fn is_mouse_button_released(&self, button: mouse::Button) -> bool {
        self.mouse.is_mouse_button_released(button)
    }

    pub fn mouse_position(&self) -> (f32, f32) {
        self.mouse.mouse_position()
    }

    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse.mouse_delta()
    }

    pub fn keyboard(&self) -> &Keyboard {
        &self.keyboard
    }

    pub fn keyboard_mut(&mut self) -> &mut Keyboard {
        &mut self.keyboard
    }

    pub fn mouse(&self) -> &Mouse {
        &self.mouse
    }

    pub fn mouse_mut(&mut self) -> &mut Mouse {
        &mut self.mouse
    }

    pub fn handle_winit_device_event(&mut self, device_id: WinitDeviceId, event: WinitDeviceEvent) {
        match event {
            WinitDeviceEvent::Key(key_event) => {
                if let winit::keyboard::PhysicalKey::Code(winit_key_code) = key_event.physical_key {
                    if let Some(key) = keyboard::Key::from_winit_key_code(winit_key_code) {
                        println!("device key: {:?}", key);
                        match key_event.state {
                            winit::event::ElementState::Pressed => {
                                self.keyboard
                                    .submit_input(keyboard::SubmitInput::Pressed(key));
                            }
                            winit::event::ElementState::Released => {
                                self.keyboard
                                    .submit_input(keyboard::SubmitInput::Released(key));
                            }
                        }
                    }
                }
            }
            WinitDeviceEvent::MouseMotion { delta } => {
                self.mouse
                    .submit_input(mouse::SubmitInput::Delta(delta.0 as f32, delta.1 as f32));
            }
            _ => {}
        }
    }

    pub fn handle_winit_window_event(&mut self, event: winit::event::WindowEvent) {
        if let winit::event::WindowEvent::KeyboardInput {
            device_id,
            event,
            is_synthetic,
        } = &event
        {
            if !is_synthetic {
                if let winit::keyboard::PhysicalKey::Code(winit_key_code) = event.physical_key {
                    if let Some(key) = keyboard::Key::from_winit_key_code(winit_key_code) {
                        println!("window key: {:?}", key);
                        match event.state {
                            winit::event::ElementState::Pressed => {
                                self.keyboard
                                    .submit_input(keyboard::SubmitInput::Pressed(key));
                            }
                            winit::event::ElementState::Released => {
                                self.keyboard
                                    .submit_input(keyboard::SubmitInput::Released(key));
                            }
                        }
                    }
                }
            }
        }
        if let winit::event::WindowEvent::MouseInput {
            device_id,
            button,
            state,
            ..
        } = &event
        {
            if let Some(button) = mouse::Button::from_winit_button(button) {
                match state {
                    winit::event::ElementState::Pressed => {
                        self.mouse.submit_input(mouse::SubmitInput::Pressed(button));
                    }
                    winit::event::ElementState::Released => {
                        self.mouse
                            .submit_input(mouse::SubmitInput::Released(button));
                    }
                }
            }
        }
    }
}
