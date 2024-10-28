use nalgebra::Vector3;

pub struct RigidBody {
    velocity: Vector3<f32>,
    acceleration: Vector3<f32>,

    mass: f32,
    inv_mass: f32,
}

impl RigidBody {
    pub fn new(mass: f32) -> Self {
        Self {
            velocity: Vector3::zeros(),
            acceleration: Vector3::zeros(),

            mass,
            inv_mass: 1.0 / mass,
        }
    }
}
