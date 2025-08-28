use std::{f32, time::Duration};

use nalgebra::Vector3;

use crate::consts;

use super::transform::Transform;

pub enum ForceType {
    /// Instantly applies the force.
    /// Newtons applied this frame.
    Impulse,
    /// Applies a force gradually.
    /// Newtons / Second
    Force,
}

#[derive(Clone)]
pub struct RigidBody {
    pub velocity: Vector3<f32>,
    pub acceleration: Vector3<f32>,

    // Set and cleared after physics update.
    pub force: Vector3<f32>,
    pub impulse_force: Vector3<f32>,

    mass: f32,
    inv_mass: f32,
    // Where 1 is fully elastic and 0 is non-elastic.
    restitution: f32,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl RigidBody {
    pub fn new(mass: f32) -> Self {
        Self {
            velocity: Vector3::zeros(),
            acceleration: Vector3::zeros(),
            force: Vector3::zeros(),
            impulse_force: Vector3::zeros(),

            mass,
            inv_mass: 1.0 / mass,
            restitution: 0.0,
        }
    }

    pub fn set_mass(&mut self, mass: f32) {
        self.mass = mass;
        self.inv_mass = 1.0 / mass;
    }

    pub fn mass(&self) -> f32 {
        self.mass
    }

    pub fn inv_mass(&self) -> f32 {
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
        }
    }

    pub fn update(&mut self, timestep: Duration, transform: &mut Transform) {
        let mut forces = self.impulse_force;
        forces += self.force * timestep.as_secs_f32();
        self.impulse_force = Vector3::zeros();
        self.force = Vector3::zeros();

        // Apply forces.
        // F = ma
        // a = F/m
        self.acceleration += forces * self.inv_mass;
        self.velocity = (self.velocity + self.acceleration * timestep.as_secs_f32()).map(|x| {
            x.clamp(
                -consts::physics::VELOCITY_MAX,
                consts::physics::VELOCITY_MAX,
            )
        });
        transform.position += self.velocity * timestep.as_secs_f32();
    }
}
