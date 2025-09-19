use std::{f32, time::Duration};

use ash::vk::DisplayPlaneAlphaFlagsKHR;
use nalgebra::{UnitQuaternion, Vector3};

use crate::consts;

use super::transform::Transform;

pub enum ForceType {
    /// Instantly applies the force.
    /// Newtons applied this frame.
    Impulse,
    /// Applies a force gradually.
    /// Newtons / Second
    Force,
    /// Directly adds to the velocity in m/s, same as Impulse
    /// but doesn't take mass into account.
    Acceleration,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum RigidBodyType {
    // Doesn't move, has infinite mass.
    Static,
    // Can be affected by forces like gravity and collisions.
    Dynamic,
}

impl Default for RigidBodyType {
    fn default() -> Self {
        Self::Dynamic
    }
}

pub struct RigidBodyCreateInfo {
    pub rigid_body_type: RigidBodyType,
    pub mass: f32,
    // Where 1 is fully elastic and 0 is non-elastic.
    pub restitution: f32,
}

#[derive(Clone)]
pub struct RigidBody {
    pub velocity: Vector3<f32>,
    // Axis angle with length representing ccw-speed in radians per second.
    pub angular_velocity: Vector3<f32>,

    // Set and cleared after physics update.
    pub force: Vector3<f32>,
    pub impulse_force: Vector3<f32>,

    pub rigid_body_type: RigidBodyType,
    mass: f32,
    inv_mass: f32,
    // Where 1 is fully elastic and 0 is non-elastic.
    pub restitution: f32,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self::new(RigidBodyCreateInfo {
            rigid_body_type: RigidBodyType::Dynamic,
            mass: 1.0,
            restitution: 0.5,
        })
    }
}

impl RigidBody {
    pub fn new(create_info: RigidBodyCreateInfo) -> Self {
        Self {
            velocity: Vector3::zeros(),
            angular_velocity: Vector3::zeros(),
            force: Vector3::zeros(),
            impulse_force: Vector3::zeros(),

            rigid_body_type: create_info.rigid_body_type,

            mass: create_info.mass,
            inv_mass: 1.0 / create_info.mass,
            restitution: create_info.restitution,
        }
    }

    pub fn velocity(&self) -> Vector3<f32> {
        if self.rigid_body_type == RigidBodyType::Static {
            return Vector3::zeros();
        }
        return self.velocity;
    }

    pub fn set_mass(&mut self, mass: f32) {
        self.mass = mass;
        self.inv_mass = 1.0 / mass;
    }

    pub fn mass(&self) -> f32 {
        if self.rigid_body_type == RigidBodyType::Static {
            return 1.0;
        }
        self.mass
    }

    pub fn inv_mass(&self) -> f32 {
        if self.rigid_body_type == RigidBodyType::Static {
            return 0.0;
        }
        self.inv_mass
    }

    pub fn apply_force(&mut self, force_type: ForceType, force: Vector3<f32>) {
        match force_type {
            ForceType::Impulse => {
                self.impulse_force += force;
            }
            ForceType::Force => {
                self.force += force;
            }
            ForceType::Acceleration => {
                self.velocity += force;
            }
        }
    }

    pub fn update(&mut self, timestep: Duration, transform: &mut Transform) {
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
        transform.position += self.velocity * timestep.as_secs_f32();
        let delta_angular_velocity = self.angular_velocity * timestep.as_secs_f32();
        if delta_angular_velocity.norm_squared() != 0.0 {
            transform.rotation *= UnitQuaternion::from_axis_angle(
                &nalgebra::Unit::new_normalize(delta_angular_velocity),
                delta_angular_velocity.norm(),
            );
        }
    }
}
