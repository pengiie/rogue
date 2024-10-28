use nalgebra::{Rotation3, UnitQuaternion, Vector3};

use crate::common::{aabb::AABB, obb::OBB};

use super::{voxel::VoxelModelImpl, voxel_constants};

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

    pub fn as_obb(&self, voxel_model: &dyn VoxelModelImpl) -> OBB {
        let min = self.position;
        let max = min
            + voxel_model.length().map(|x| x as f32)
                * voxel_constants::VOXEL_WORLD_UNIT_LENGTH
                * self.scale;

        let rotation_anchor = match self.rotation_anchor {
            VoxelModelRotationAnchor::Zero => min,
            VoxelModelRotationAnchor::Center => (min + max) * 0.5,
        };

        OBB::new(AABB::new(min, max), self.rotation, rotation_anchor)
    }
}
