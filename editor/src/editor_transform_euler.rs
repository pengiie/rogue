use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use rogue_engine::{
    entity::{GameEntity, ecs_world::ECSWorld},
    physics::transform::Transform,
    resource::ResMut,
};

#[derive(Clone)]
pub struct EditorTransformEuler {
    /// (pitch, yaw, roll) in radians.
    euler: Vector3<f32>,
    last_quat_value: UnitQuaternion<f32>,
}

impl EditorTransformEuler {
    pub fn euler(&self) -> Vector3<f32> {
        self.euler
    }

    /// Returns the quaternion that the transform should be set to.
    pub fn set_euler(&mut self, new_euler: Vector3<f32>) -> UnitQuaternion<f32> {
        self.euler = new_euler;
        self.last_quat_value =
            UnitQuaternion::from_euler_angles(self.euler.x, self.euler.y, self.euler.z);
        self.last_quat_value
    }

    pub fn rotate(&mut self, euler: &Vector3<f32>) -> UnitQuaternion<f32> {
        self.euler += euler;
        self.last_quat_value =
            UnitQuaternion::from_euler_angles(self.euler.x, self.euler.y, self.euler.z);
        self.last_quat_value
    }

    pub fn get_quaternion(&self) -> UnitQuaternion<f32> {
        self.last_quat_value
    }

    fn from_transform(transform: &Transform) -> Self {
        let (roll, pitch, yaw) = transform.rotation.euler_angles();
        Self {
            euler: Vector3::new(pitch, yaw, roll),
            last_quat_value: transform.rotation,
        }
    }

    fn try_update_from_transform(&mut self, transform: &Transform) {
        if transform.rotation != self.last_quat_value {
            let (mut roll, mut pitch, mut yaw) = transform.rotation.euler_angles();
            if roll == -0.0 {
                roll = 0.0;
            }
            if pitch == -0.0 {
                pitch = 0.0;
            }
            if yaw == -0.0 {
                yaw = 0.0;
            }
            self.euler = Vector3::new(pitch, yaw, roll);
            self.last_quat_value = transform.rotation;
        }
    }

    pub fn update_euler_representation(mut ecs_world: ResMut<ECSWorld>) {
        let mut to_insert = Vec::new();
        for (entity, transform) in ecs_world
            .query::<&Transform>()
            .with::<(GameEntity,)>()
            .into_iter()
        {
            if !ecs_world.contains::<EditorTransformEuler>(entity) {
                to_insert.push((entity, EditorTransformEuler::from_transform(transform)));
            }

            if let Ok(mut editor_transform_euler) =
                ecs_world.get::<&mut EditorTransformEuler>(entity)
            {
                editor_transform_euler.try_update_from_transform(transform);
            }
        }

        for (entity, editor_transform_euler) in to_insert {
            ecs_world.insert_one(entity, editor_transform_euler);
        }
    }
}
