use crate::engine::graphics::backend::{Image, ResourceId};

pub struct Material {
    name: String,
}

pub enum MaterialType {
    Texture(MaterialTexture),
}

pub struct MaterialTexture {
    texture: ResourceId<Image>,
}

pub struct MaterialBank {
    materials: Vec<Material>,
}

impl MaterialBank {}
