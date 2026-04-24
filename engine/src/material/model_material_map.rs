use crate::common::freelist::FreeListHandle;
use crate::material::material_bank::MaterialId;
use crate::{asset::asset::GameAssetPath, common::freelist::FreeList};

#[derive(Clone)]
pub struct ModelMaterial {
    pub material_id: MaterialId,
    pub model_material_id: u32,
}

#[derive(Clone)]
pub struct ModelMaterialMap {
    pub model_materials: Vec<ModelMaterial>,
}

impl ModelMaterialMap {
    pub fn new() -> Self {
        Self {
            model_materials: Vec::new(),
        }
    }

    pub fn ensure_global_material_exists(&mut self, material_id: &MaterialId) {
        if self
            .model_materials
            .iter()
            .any(|m| &m.material_id == material_id)
        {
            return;
        }
        self.push(material_id.clone());
    }

    pub fn get_global_material(&self, model_material_id: u32) -> Option<&MaterialId> {
        self.model_materials
            .get(model_material_id as usize)
            .map(|m| &m.material_id)
    }

    pub fn get_model_material(&self, material_id: &MaterialId) -> Option<&ModelMaterial> {
        self.model_materials
            .iter()
            .find(|m| &m.material_id == material_id)
    }

    pub fn push(&mut self, material_id: MaterialId) {
        let next_id = self.model_materials.len();
        self.model_materials.push(ModelMaterial {
            material_id,
            model_material_id: next_id as u32,
        });
    }
}
