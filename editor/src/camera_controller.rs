use nalgebra::{UnitQuaternion, Vector3};
use rogue_engine::{
    input::{Input, keyboard::Key, mouse},
    physics::transform::Transform,
    window::{time::Time, window::Window},
};

use crate::editor_project_settings::EditorProjectSettingsData;

pub enum EditorCameraControllerType {
    PanOrbit,
    Fps,
}

pub struct EditorCameraController {
    pub rotation_anchor: Vector3<f32>,
    pub euler: Vector3<f32>,
    pub distance: f32,

    pub controller_type: EditorCameraControllerType,
}

impl EditorCameraController {
    const SENS: f32 = 0.001;

    pub fn new() -> Self {
        Self {
            rotation_anchor: Vector3::zeros(),
            euler: Vector3::zeros(),
            distance: 10.0,

            controller_type: EditorCameraControllerType::PanOrbit,
        }
    }

    pub fn from_project_settings(settings: &EditorProjectSettingsData) -> Self {
        Self {
            rotation_anchor: settings.editor_camera_anchor,
            euler: settings.editor_camera_rotation,
            distance: settings.editor_camera_distance,
            controller_type: EditorCameraControllerType::PanOrbit,
        }
    }

    pub fn focus_on_position(&mut self, world_position: Vector3<f32>) {
        match self.controller_type {
            EditorCameraControllerType::PanOrbit => {
                self.rotation_anchor = world_position;
            }
            EditorCameraControllerType::Fps => {}
        }
    }

    pub fn update(
        &mut self,
        transform: &mut Transform,
        input: &Input,
        time: &Time,
        window: &mut Window,
    ) {
        if input.is_key_pressed(Key::Escape) {
            match self.controller_type {
                EditorCameraControllerType::PanOrbit => {
                    self.controller_type = EditorCameraControllerType::Fps;
                    window.set_cursor_lock(true);
                }
                EditorCameraControllerType::Fps => {
                    self.controller_type = EditorCameraControllerType::PanOrbit;
                    self.rotation_anchor = transform.position
                        + transform.rotation.transform_vector(&Vector3::new(
                            0.0,
                            0.0,
                            -self.distance,
                        ));
                    window.set_cursor_lock(false);
                    window.set_cursor_position(window.inner_size_vec2().cast::<i32>() / 2);
                }
            }
        }

        match self.controller_type {
            EditorCameraControllerType::PanOrbit => self.update_pan_orbit(transform, input),
            EditorCameraControllerType::Fps => self.update_fps(transform, input, time),
        }
    }

    fn update_fps(&mut self, transform: &mut Transform, input: &Input, time: &Time) {
        let mouse_delta = input.mouse_delta() * Self::SENS;
        self.euler.x = (self.euler.x + mouse_delta.y)
            .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        self.euler.y -= mouse_delta.x;

        transform.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.euler.y)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.euler.x);

        // Get yaw for translation from rotation
        let movement_axes = input.movement_axes();
        let y_rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.euler.y);
        let mut translation =
            y_rotation.transform_vector(&Vector3::new(movement_axes.x, 0.0, -movement_axes.y));
        if input.is_key_down(Key::Space) {
            translation.y = 1.0;
        } else if input.is_key_down(Key::LShift) {
            translation.y -= 1.0;
        }

        let mut movement_speed = 30.0;
        if input.is_key_down(Key::LControl) {
            movement_speed = 500.0;
        }
        transform.position += translation * movement_speed * time.delta_time().as_secs_f32();
    }

    fn update_pan_orbit(&mut self, transform: &mut Transform, input: &Input) {
        if input.is_mouse_button_down(mouse::Button::Middle) {
            let delta = -input.mouse_delta() * Self::SENS * self.distance.max(1.0);
            let up = transform.rotation.transform_vector(&Vector3::y());
            let right = transform.rotation.transform_vector(&Vector3::x());
            self.rotation_anchor += delta.x * right + delta.y * up;
        }

        if input.is_mouse_button_down(mouse::Button::Right) {
            let delta = input.mouse_delta() * Self::SENS * 0.8;
            self.euler.x = (self.euler.x + delta.y)
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            self.euler.y -= delta.x;
        }

        let mut scroll_delta = input.mouse().scroll_delta() * 0.05;
        self.distance = (self.distance * (1.0 + scroll_delta)).clamp(0.01, 250.0);

        let rot = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.euler.y)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.euler.x);
        let pos = self.rotation_anchor + self.distance * (rot.transform_vector(&Vector3::z()));
        transform.position = pos;
        transform.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.euler.y)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.euler.x);
    }
}
