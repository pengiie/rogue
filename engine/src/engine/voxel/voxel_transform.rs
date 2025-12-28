use nalgebra::{UnitQuaternion, Vector3};

use crate::consts;
use crate::common::geometry::aabb::AABB;
use crate::common::geometry::obb::OBB;

pub enum VoxelModelRotationAnchor {
    Zero,
    Center,
}

pub struct VoxelModelTransform {
    pub position: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
    pub rotation_anchor: VoxelModelRotationAnchor,
    pub scale: f32,
}

impl VoxelModelTransform {
    pub fn with_position(position: Vector3<f32>) -> Self {
        Self {
            position,
            rotation: UnitQuaternion::identity(),
            rotation_anchor: VoxelModelRotationAnchor::Center,
            scale: 1.0,
        }
    }

    pub fn with_position_rotation(position: Vector3<f32>, rotation: UnitQuaternion<f32>) -> Self {
        Self {
            position,
            rotation,
            rotation_anchor: VoxelModelRotationAnchor::Center,
            scale: 1.0,
        }
    }

    pub fn as_obb(&self, model_dimensions: Vector3<u32>) -> OBB {
        let min = self.position;
        let max = min
            + model_dimensions.map(|x| x as f32) * consts::voxel::VOXEL_METER_LENGTH * self.scale;

        let rotation_anchor = match self.rotation_anchor {
            VoxelModelRotationAnchor::Zero => min,
            VoxelModelRotationAnchor::Center => (min + max) * 0.5,
        };

        OBB::new(
            AABB::new_two_point(min, max),
            self.rotation,
            rotation_anchor,
        )
    }
}
