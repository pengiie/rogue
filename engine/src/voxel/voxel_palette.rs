use crate::{
    common::color::{Color, ColorSpaceSrgb},
    voxel::voxel::VoxelMaterialData,
};

pub struct VoxelModelPalette {
    data: Vec<EncodedVoxelPaletteEntry>,
}

impl VoxelModelPalette {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }
}

struct EncodedVoxelPaletteEntry {
    pub color: [u8; 4],
}
