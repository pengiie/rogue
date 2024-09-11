use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use nalgebra::Vector3;
use rogue_macros::Resource;

use super::{esvo::VoxelModelESVO, flat::VoxelModelFlat};

pub struct VoxelRange {
    pub data: VoxelModelFlat,
    position: Vector3<u32>,
    length: Vector3<u32>,
}

impl VoxelRange {
    pub fn new(data: VoxelModelFlat, position: Vector3<u32>, length: Vector3<u32>) -> Self {
        Self {
            data,
            position,
            length,
        }
    }

    pub fn position(&self) -> Vector3<u32> {
        self.position
    }

    pub fn length(&self) -> Vector3<u32> {
        self.length
    }

    pub fn new_filled(position: Vector3<u32>, length: Vector3<u32>, color: u32) -> Self {
        let mut attributes = HashMap::new();
        attributes.insert(Attributes::COMPRESSED, color);
        let data = VoxelModelFlat::new_filled(attributes, length);

        Self::new(data, position, length)
    }
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct Attributes: u32 {
        const NONE = 0;
        const ALBEDO = 1;
        const NORMAL = 2;

        // Color 8,8,8,8, Normal 10,10,10
        const COMPRESSED = 4;
    }
}

pub trait VoxelModelImpl {
    fn set_voxel_range(&mut self, range: VoxelRange);
    fn get_node_data(&self) -> &[u8];
    fn get_attachment_lookup_data(&self) -> &[u8];
    fn get_attachments_data(&self) -> HashMap<Attributes, &[u8]>;
}

pub enum VoxelModelSchema {
    ESVO,
}

pub struct VoxelModel {
    // TODO: Make the models store in memory pools so we can get contiguous cache access, only
    // important if we end up with a lot of models such as for breakables or something.
    model: Box<dyn VoxelModelImpl>,
}

impl VoxelModel {
    pub fn new(schema: VoxelModelSchema) -> Self {
        Self {
            model: Self::initialize_voxel_model(schema),
        }
    }

    fn initialize_voxel_model(schema: VoxelModelSchema) -> Box<dyn VoxelModelImpl> {
        Box::new(match schema {
            VoxelModelSchema::ESVO => VoxelModelESVO::new(),
        })
    }
}

impl Deref for VoxelModel {
    type Target = dyn VoxelModelImpl;

    fn deref(&self) -> &Self::Target {
        self.model.deref()
    }
}

impl DerefMut for VoxelModel {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.model.deref_mut()
    }
}

#[derive(Resource)]
pub struct VoxelWorld {
    esvo: VoxelModel,
}

impl VoxelWorld {
    pub fn new() -> Self {
        let mut esvo = VoxelModel::new(VoxelModelSchema::ESVO);
        esvo.set_voxel_range(VoxelRange::new_filled(
            Vector3::new(0, 0, 0),
            Vector3::new(10, 10, 10),
            0xFFFF00FF,
        ));
        Self { esvo }
    }

    pub fn world_node_data(&self) -> &[u8] {
        self.esvo.get_node_data()
    }

    pub fn world_attachment_lookup_data(&self) -> &[u8] {
        self.esvo.get_attachment_lookup_data()
    }

    pub fn world_attachments_data(&self) -> HashMap<Attributes, &[u8]> {
        self.esvo.get_attachments_data()
    }
}
