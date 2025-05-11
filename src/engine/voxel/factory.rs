use nalgebra::Vector3;

use crate::common::color::{Color, ColorSpaceSrgb};

use super::{
    attachment::{Attachment, PTMaterial},
    flat::VoxelModelFlat,
    voxel::VoxelModel,
};

pub struct VoxelModelFactory;

impl VoxelModelFactory {
    pub fn create_cuboid(side_length: Vector3<u32>, color: Color) -> VoxelModel<VoxelModelFlat> {
        let mut flat_model = VoxelModelFlat::new_empty(side_length);
        for (local_pos, mut voxel) in flat_model.xyz_iter_mut() {
            voxel.set_attachment(
                Attachment::PTMATERIAL,
                Some(PTMaterial::diffuse(color.clone()).encode()),
            );
        }

        return VoxelModel::new(flat_model);
    }
}
