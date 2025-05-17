use nalgebra::Vector2;
use rogue_macros::Resource;

use crate::consts;
use crate::engine::resource::Res;
use crate::engine::resource::ResMut;
use crate::engine::window::window::Window;

use winit::event::DeviceEvent as WinitDeviceEvent;
use winit::event::DeviceId as WinitDeviceId;

use super::gamepad::Gamepad;
use super::mapper::Keybinds;
use super::{
    keyboard::{self, Keyboard},
    mouse::{self, Mouse},
};

#[derive(Resource)]
pub struct Input {
    keyboard: Keyboard,
    mouse: Mouse,
    gamepad: Gamepad,
    keybinds: Keybinds,
}

impl Input {
    pub fn new() -> Self {
        let mut keybinds = Keybinds::new();
        keybinds.register_key(
            consts::actions::EDITOR_TOGGLE,
            consts::actions::keybind::EDITOR_TOGGLE_DEBUG,
        );
        Self {
            keyboard: Keyboard::new(),
            mouse: Mouse::new(),
            gamepad: Gamepad::new(),
            keybinds,
        }
    }

    pub fn clear_inputs(mut input: ResMut<Input>, window: Res<Window>) {
        input.keyboard.clear_inputs();
        input.mouse.clear_inputs();
        input.gamepad.clear_inputs();
    }

    pub fn collect_gamepad_events(mut input: ResMut<Input>) {
        input.gamepad.collect_events();
    }

    // General Input
    pub fn movement_axes(&self) -> Vector2<f32> {
        if self.gamepad.left_axis().x.abs() >= self.gamepad.deadzone
            || self.gamepad.left_axis().y.abs() >= self.gamepad.deadzone
        {
            return *self.gamepad.left_axis();
        }

        let mut axes = Vector2::new(0.0, 0.0);
        if self.keyboard.is_key_down(keyboard::Key::A) {
            axes.x -= 1.0;
        }
        if self.keyboard.is_key_down(keyboard::Key::D) {
            axes.x += 1.0;
        }
        if self.keyboard.is_key_down(keyboard::Key::S) {
            axes.y -= 1.0;
        }
        if self.keyboard.is_key_down(keyboard::Key::W) {
            axes.y += 1.0;
        }

        return axes;
    }

    pub fn did_action(&self, action: &str) -> bool {
        let key = *self
            .keybinds
            .pressed_key_mappings
            .get(action)
            .expect("Action does not exist.");
        return self.is_key_pressed(key);
    }

    pub fn is_controller_camera(&self) -> bool {
        return self.gamepad.right_axis().x.abs() >= self.gamepad.deadzone
            || self.gamepad.right_axis().y.abs() >= self.gamepad.deadzone;
    }

    pub fn camera_axes(&self) -> Vector2<f32> {
        if self.is_controller_camera() {
            return *self.gamepad.right_axis();
        }

        return self.mouse.mouse_delta();
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

    pub fn mouse_position(&self) -> Vector2<f32> {
        self.mouse.mouse_position()
    }

    pub fn mouse_delta(&self) -> Vector2<f32> {
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
                    .submit_input(mouse::SubmitInput::PosDelta(delta.0 as f32, delta.1 as f32));
            }
            WinitDeviceEvent::MouseWheel { delta } => match delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => {
                    self.mouse.submit_input(mouse::SubmitInput::ScrollDelta(y));
                }
                winit::event::MouseScrollDelta::PixelDelta(physical_position) => {
                    log::warn!("pixel based scrolling not supported yet.")
                }
            },
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

        if let winit::event::WindowEvent::MouseWheel {
            device_id,
            delta,
            phase,
        } = &event
        {
            match delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => {
                    // Use the device one instead.
                    //self.mouse.submit_input(mouse::SubmitInput::ScrollDelta(*y));
                }
                winit::event::MouseScrollDelta::PixelDelta(physical_position) => {
                    log::warn!("pixel based scrolling not supported yet.")
                }
            }
        }

        if let winit::event::WindowEvent::CursorMoved { position, .. } = &event {
            self.mouse.submit_input(mouse::SubmitInput::Position(
                position.x as f32,
                position.y as f32,
            ));
        }
    }
}
