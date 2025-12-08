use std::{f32, time::Duration};

use ash::vk::DisplayPlaneAlphaFlagsKHR;
use nalgebra::{Matrix3, UnitQuaternion, Vector3};
use rogue_macros::game_component;

use super::transform::Transform;
use crate::engine::entity::component::GameComponentSerializeContext;
use crate::{consts, engine::entity::component::GameComponent};

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
}

impl Default for RigidBodyType {
    fn default() -> Self {
        Self::Dynamic
    }
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
}

impl Default for RigidBodyCreateInfo {
    fn default() -> Self {
        Self {
            rigid_body_type: RigidBodyType::Dynamic,
            mass: 1.0,
            restitution: 0.5,
            friction: 0.7,
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
    pub velocity: Vector3<f32>,
    // Axis angle with length representing ccw-speed in radians per second.
    pub angular_velocity: Vector3<f32>,

    // Set and cleared after physics update.
    pub force: Vector3<f32>,
    pub impulse_force: Vector3<f32>,

    pub rigid_body_type: RigidBodyType,
    mass: f32,

    inv_mass: f32,
    // Massless inverse interia tensor;
    inv_inertia: Matrix3<f32>,

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
            velocity: Vector3::zeros(),
            angular_velocity: Vector3::zeros(),
            force: Vector3::zeros(),
            impulse_force: Vector3::zeros(),

            rigid_body_type: create_info.rigid_body_type,

            mass: create_info.mass,
            inv_mass: 1.0 / create_info.mass,
            inv_inertia: Matrix3::identity() * (1.0 / ((1.0 / 6.0) * 4.0)),
            restitution: create_info.restitution,
            friction: create_info.friction,
        }
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
        if self.rigid_body_type == RigidBodyType::Static {
            return 1.0;
        }
        self.mass
    }

    pub fn is_static(&self) -> bool {
        self.rigid_body_type == RigidBodyType::Static
    }

    pub fn inv_mass(&self) -> f32 {
        if self.rigid_body_type == RigidBodyType::Static {
            return 0.0;
        }
        self.inv_mass
    }

    pub fn inv_inertia(&self) -> Matrix3<f32> {
        if self.rigid_body_type == RigidBodyType::Static {
            return Matrix3::zeros();
        }
        // TODO: Calculate based on colliders.
        return self.inv_inertia;
    }

    pub fn apply_impulse_at_point(
        &mut self,
        impulse_force: Vector3<f32>,
        local_point: Vector3<f32>,
    ) {
        if self.rigid_body_type == RigidBodyType::Static {
            return;
        }

        // impulse = mass * delta_velocity
        self.velocity += impulse_force * self.inv_mass();
        self.angular_velocity += self.inv_inertia() * local_point.cross(&impulse_force);
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
