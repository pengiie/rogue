use std::{f32, time::Duration};

use nalgebra::{Matrix3, UnitQuaternion, Vector3};
use rogue_macros::game_component;

use super::transform::Transform;
use crate::consts;

pub enum ForceType {
    /// Instantly applies the force.
    /// Newtons applied this frame.
    Impulse,
    /// Applies a force gradually.
    /// Newtons / Second
    Force,
    /// Directly adds to the velocity in m/s, same as Impulse
    /// but doesn't take mass into account.
    VelocityChange,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum RigidBodyType {
    // Doesn't move, has infinite mass.
    Static,
    // Can be affected by forces like gravity and collisions.
    Dynamic,
    Kinematic,
    /// The rigid body can change the position and rotation of the transform
    /// and this rigid body's velocities will be calculated the next physics step.
    KinematicPositionBased,
}

impl Default for RigidBodyType {
    fn default() -> Self {
        Self::Dynamic
    }
}

#[derive(
    Clone,
    Copy,
    Eq,
    PartialEq,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    strum_macros::Display,
    strum_macros::VariantArray,
)]
pub enum RigidBodyPositionInterpolation {
    None,
    Interpolate,
}

/// Serializable type for the RigidBody component.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RigidBodyCreateInfo {
    pub rigid_body_type: RigidBodyType,
    pub mass: f32,
    // Where 1 is fully elastic and 0 is non-elastic.
    pub restitution: f32,
    // Friction coefficient 0.0-1.0.
    pub friction: f32,
    pub locked_rotational_axes: Vector3<bool>,
    pub interpolation: RigidBodyPositionInterpolation,
}

impl Default for RigidBodyCreateInfo {
    fn default() -> Self {
        Self {
            rigid_body_type: RigidBodyType::Dynamic,
            mass: 1.0,
            restitution: 0.5,
            friction: 0.7,
            locked_rotational_axes: Vector3::new(false, false, false),
            interpolation: RigidBodyPositionInterpolation::None,
        }
    }
}

impl From<RigidBodyCreateInfo> for RigidBody {
    fn from(info: RigidBodyCreateInfo) -> Self {
        RigidBody::new(info)
    }
}

impl Into<RigidBodyCreateInfo> for RigidBody {
    fn into(self) -> RigidBodyCreateInfo {
        self.create_info()
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "RigidBody")]
#[serde(from = "RigidBodyCreateInfo")]
#[serde(into = "RigidBodyCreateInfo")]
pub struct RigidBody {
    /// Used for deriving velocities from transform changes.
    last_position: Vector3<f32>,
    last_rotation: UnitQuaternion<f32>,
    /// Authoritative position/rotation for the transform the rigid body is attached to.
    position: Vector3<f32>,
    rotation: UnitQuaternion<f32>,
    needs_transform_init: bool,
    pub interpolation: RigidBodyPositionInterpolation,

    pub velocity: Vector3<f32>,
    // Axis angle with length representing ccw-speed in radians per second.
    pub angular_velocity: Vector3<f32>,

    // Set and cleared after physics update.
    pub force: Vector3<f32>,
    pub impulse_force: Vector3<f32>,

    pub locked_rotational_axes: Vector3<bool>,
    pub rigid_body_type: RigidBodyType,
    mass: f32,

    inv_mass: f32,
    // Massless inverse interia tensor;
    inv_inertia: Matrix3<f32>,
    inv_inertia_world: Matrix3<f32>,

    // Where 1 is fully elastic and 0 is non-elastic.
    pub restitution: f32,
    pub friction: f32,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self::new(RigidBodyCreateInfo::default())
    }
}

impl RigidBody {
    pub fn new(create_info: RigidBodyCreateInfo) -> Self {
        Self {
            last_position: Vector3::zeros(),
            last_rotation: UnitQuaternion::identity(),
            position: Vector3::zeros(),
            rotation: UnitQuaternion::identity(),
            needs_transform_init: true,
            interpolation: create_info.interpolation,

            velocity: Vector3::zeros(),
            angular_velocity: Vector3::zeros(),
            force: Vector3::zeros(),
            impulse_force: Vector3::zeros(),

            rigid_body_type: create_info.rigid_body_type,
            locked_rotational_axes: create_info.locked_rotational_axes,

            mass: create_info.mass,
            inv_mass: 1.0 / create_info.mass,
            inv_inertia: Matrix3::identity() * (1.0 / ((1.0 / 6.0) * (2.0 + 2.0f32).powi(2))),
            inv_inertia_world: Matrix3::identity(),
            restitution: create_info.restitution,
            friction: create_info.friction,
        }
    }

    pub fn new_static() -> Self {
        Self::new(RigidBodyCreateInfo {
            rigid_body_type: RigidBodyType::Static,
            mass: 0.0,
            restitution: 0.5,
            friction: 0.7,
            locked_rotational_axes: Vector3::new(false, false, false),
            interpolation: RigidBodyPositionInterpolation::None,
        })
    }

    pub fn sync_transform(&mut self, transform: &Transform) {
        self.position = transform.position;
        self.rotation = transform.rotation;
        self.last_position = transform.position;
        self.last_rotation = transform.rotation;
    }

    pub fn try_init_transform(&mut self, transform: &Transform) {
        if !self.needs_transform_init {
            return;
        }
        self.sync_transform(transform);
        self.needs_transform_init = false;
    }

    /// Applies the rigid body's position and rotation to the transform depending on interpolation.
    pub fn apply_to_transform(&self, transform: &mut Transform, t: f32) {
        match self.interpolation {
            RigidBodyPositionInterpolation::None => {
                transform.position = self.position;
                transform.rotation = self.rotation;
            }
            RigidBodyPositionInterpolation::Interpolate => {
                transform.position = self.last_position.lerp(&self.position, t);
                transform.rotation = self.last_rotation.nlerp(&self.rotation, t);
            }
        }
    }

    pub fn position(&self) -> Vector3<f32> {
        self.position
    }

    pub fn kinetic_energy(&self) -> f32 {
        0.5 * self.mass() * self.velocity.component_mul(&self.velocity()).norm()
    }

    pub fn create_info(&self) -> RigidBodyCreateInfo {
        RigidBodyCreateInfo {
            rigid_body_type: self.rigid_body_type,
            mass: self.mass,
            restitution: self.restitution,
            friction: self.friction,
            locked_rotational_axes: self.locked_rotational_axes,
            interpolation: self.interpolation,
        }
    }

    pub fn velocity(&self) -> Vector3<f32> {
        if self.rigid_body_type == RigidBodyType::Static {
            return Vector3::zeros();
        }
        return self.velocity;
    }

    /// The linear velocity at `local_point` caused by the angular velocity.
    pub fn angular_linear_velocity(&self, local_point: Vector3<f32>) -> Vector3<f32> {
        if self.rigid_body_type == RigidBodyType::Static {
            return Vector3::zeros();
        }
        return self.angular_velocity.cross(&local_point);
    }

    pub fn set_mass(&mut self, mass: f32) {
        self.mass = mass;
        self.inv_mass = 1.0 / mass;
    }

    pub fn mass(&self) -> f32 {
        if self.is_static() {
            return 1.0;
        }
        self.mass
    }

    /// Static in the sense that its position and rotation cannot move.
    pub fn is_static(&self) -> bool {
        self.rigid_body_type == RigidBodyType::Static
            || self.rigid_body_type == RigidBodyType::KinematicPositionBased
    }

    pub fn inv_mass(&self) -> f32 {
        if self.is_static() {
            return 0.0;
        }
        self.inv_mass
    }

    /// Return the world-space inertia tensor.
    pub fn inv_inertia(&self) -> Matrix3<f32> {
        if self.is_static() {
            return Matrix3::zeros();
        }
        // TODO: Calculate based on colliders.
        return self.inv_inertia_world;
    }

    pub fn apply_impulse_at_point(
        &mut self,
        impulse_force: Vector3<f32>,
        world_space_offset: Vector3<f32>,
    ) {
        if self.is_static() {
            return;
        }

        // impulse = mass * delta_velocity
        self.velocity += impulse_force * self.inv_mass();
        self.set_angular_velocity(
            self.angular_velocity + self.inv_inertia() * world_space_offset.cross(&impulse_force),
        );
    }

    pub fn set_angular_velocity(&mut self, angular_velocity: Vector3<f32>) {
        if self.is_static() {
            return;
        }

        self.angular_velocity = angular_velocity.zip_map(
            &self.locked_rotational_axes,
            |v, locked| if locked { 0.0 } else { v },
        );
    }

    pub fn set_position(&mut self, position: Vector3<f32>) {
        if self.is_static() {
            return;
        }
        self.position = position;
    }

    pub fn apply_force(&mut self, force_type: ForceType, force: Vector3<f32>) {
        match force_type {
            ForceType::Impulse => {
                self.impulse_force += force;
            }
            ForceType::Force => {
                self.force += force;
            }
            ForceType::VelocityChange => {
                self.impulse_force += force * self.inv_mass;
            }
        }
    }

    pub fn integrate_velocities(&mut self, timestep: Duration) {
        self.position += self.velocity * timestep.as_secs_f32();

        // Apply angular velocity.
        let delta_angular_velocity = self.angular_velocity * timestep.as_secs_f32();
        if delta_angular_velocity.norm_squared() != 0.0 {
            // Order of quaternion multiplication matters here.
            self.rotation =
                UnitQuaternion::from_scaled_axis(delta_angular_velocity) * self.rotation;
        }
    }

    /// Calculate velocity from any forces applied.
    pub fn integrate_forces(&mut self, timestep: Duration) {
        let forces = self.impulse_force + self.force * timestep.as_secs_f32();
        self.impulse_force = Vector3::zeros();
        self.force = Vector3::zeros();

        // Apply forces.
        // F = ma
        // a = F/m
        let acceleration = forces * self.inv_mass;
        self.velocity = (self.velocity + acceleration).map(|x| {
            x.clamp(
                -consts::physics::VELOCITY_MAX,
                consts::physics::VELOCITY_MAX,
            )
        });
    }

    pub fn recalculate_world_inertia_tensor(&mut self) {
        let rot_matrix = self.rotation.to_rotation_matrix();
        self.inv_inertia_world = rot_matrix * self.inv_inertia * rot_matrix.matrix().transpose();
    }

    // Calculate velocity from the change in position over the timestep.
    pub fn derive_forces(&mut self, timestep: Duration) {
        let delta_position = self.position - self.last_position;
        self.velocity = delta_position / timestep.as_secs_f32();

        let delta_rotation = self.rotation * self.last_rotation.inverse();
        let axis_angle = delta_rotation.scaled_axis();
        self.angular_velocity = axis_angle / timestep.as_secs_f32();
    }

    /// Sets the last position and rotation to the current ones.
    pub fn update_last_position_rotation(&mut self) {
        self.last_position = self.position;
        self.last_rotation = self.rotation;
    }
}
