use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use hecs::Bundle;
use nalgebra::Vector3;
use rogue_macros::Resource;

use crate::{common::aabb::AABB, engine::physics::transform::Transform};

use super::{allocator::VoxelAllocator, esvo::VoxelModelESVO, flat::VoxelModelFlat};

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

        // Max 8 attachments
    }
}

pub trait VoxelModelImpl: Send + Sync {
    fn set_voxel_range(&mut self, range: VoxelRange);
    fn schema(&self) -> VoxelModelSchema;
    fn length(&self) -> Vector3<u32>;
}

#[derive(Clone, Copy)]
pub enum VoxelModelSchema {
    ESVO = 1,
}

#[derive(Bundle)]
pub struct RenderableVoxelModel {
    pub voxel_model: VoxelModel,
    pub transform: Transform,
}

pub struct VoxelModel {
    // TODO: Make the models store in memory pools so we can get contiguous cache access, only
    // important if we end up with a lot of models such as for breakables or something.
    model: Box<dyn VoxelModelImpl>,
    schema: VoxelModelSchema,
}

impl VoxelModel {
    pub fn new(schema: VoxelModelSchema) -> Self {
        Self {
            model: Self::initialize_voxel_model(schema.clone()),
            schema,
        }
    }

    pub fn from_impl(model: Box<dyn VoxelModelImpl>) -> Self {
        let schema = model.deref().schema();
        Self { model, schema }
    }

    pub fn length(&self) -> Vector3<u32> {
        self.model.length()
    }

    fn initialize_voxel_model(schema: VoxelModelSchema) -> Box<dyn VoxelModelImpl> {
        Box::new(match schema {
            VoxelModelSchema::ESVO => VoxelModelESVO::new(32),
        })
    }

    pub fn schema(&self) -> VoxelModelSchema {
        self.schema
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
