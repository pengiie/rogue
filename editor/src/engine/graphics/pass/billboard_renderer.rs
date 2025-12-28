use nalgebra::{Vector2, Vector3};

use crate::engine::asset::asset::AssetHandle;

/// Billboard as in the content in rendered in 3d and either
/// always facing the player or fixed a fixed rotation.
pub struct BillboardRenderer {
    images: Vec<ImageBillboard>,
}

pub struct ImageBillboard {
    pub image: AssetHandle,
    pub size: Vector2<f32>,
    /// Center of the billboard.
    pub position: Vector3<f32>,
    pub normal: BillboardNormal,
}

pub enum BillboardNormal {
    /// The billboard normal will always be rendered to look at the player.
    AlwaysVisible,
    Normal(Vector3<f32>),
}

impl BillboardRenderer {
    pub fn new() -> Self {
        Self { images: Vec::new() }
    }
}
