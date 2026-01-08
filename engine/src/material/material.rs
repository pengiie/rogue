use std::collections::HashMap;

use rogue_macros::Resource;
use serde::ser::SerializeSeq;

use crate::common::freelist::{FreeList, FreeListHandle};
use crate::asset::asset::GameAssetPath;
use crate::event::Events;
use crate::resource::ResMut;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaterialTextureType {
    Color,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Material {
    pub name: String,
    pub color_texture: Option<GameAssetPath>,
}

impl Material {
    pub fn is_empty(&self) -> bool {
        return self.color_texture.is_none();
    }
}

#[derive(Hash, Clone, PartialEq, Eq)]
pub struct MaterialSamplerOptions {}

pub type MaterialId = FreeListHandle<Material>;

pub struct MaterialCreateEvent {
    pub material_id: MaterialId,
}

pub struct MaterialUpdateEvent {
    pub material_id: MaterialId,
    pub updated_texture_type: MaterialTextureType,
}

#[derive(Resource)]
pub struct MaterialBank {
    pub materials: FreeList<Material>,
    pub name_map: HashMap<String, MaterialId>,

    create_events: Vec<MaterialCreateEvent>,
    update_events: Vec<MaterialUpdateEvent>,
}

impl MaterialBank {
    pub fn new() -> Self {
        Self {
            materials: FreeList::new(),
            name_map: HashMap::new(),

            create_events: Vec::new(),
            update_events: Vec::new(),
        }
    }

    pub fn register_material(&mut self, material: Material) -> MaterialId {
        if let Some(_) = self.name_map.get(&material.name) {
            panic!(
                "Tried to register material with name {} but that already exits",
                material.name
            );
        }

        let material_name = material.name.clone();
        let material_id = self.materials.push(material);
        self.name_map.insert(material_name, material_id);
        self.create_events.push(MaterialCreateEvent { material_id });
        return material_id;
    }

    pub fn update_material_texture(
        &mut self,
        material_id: MaterialId,
        texture_type: MaterialTextureType,
        texture: Option<GameAssetPath>,
    ) {
        let material = self
            .materials
            .get_mut(material_id)
            .expect("Tried to update material that does not exist.");

        match texture_type {
            MaterialTextureType::Color => {
                material.color_texture = texture;
            }
        }

        self.update_events.push(MaterialUpdateEvent {
            material_id,
            updated_texture_type: texture_type,
        });
    }

    /// Indirection so we can run this headless for gpu stuff.
    pub fn update_events(mut material_bank: ResMut<MaterialBank>, mut events: ResMut<Events>) {
        for create_event in material_bank.create_events.drain(..) {
            events.push(create_event);
        }

        for update_event in material_bank.update_events.drain(..) {
            events.push(update_event);
        }
    }
}

impl serde::Serialize for MaterialBank {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = se.serialize_seq(Some(self.materials.len()))?;
        for material in self.materials.iter() {
            seq.serialize_element(material)?;
        }
        seq.end()
    }
}

impl<'de> serde::Deserialize<'de> for MaterialBank {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        de.deserialize_seq(MaterialBankVisitor)
    }
}

struct MaterialBankVisitor;
impl<'de> serde::de::Visitor<'de> for MaterialBankVisitor {
    type Value = MaterialBank;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("Material array")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut material_bank = MaterialBank::new();
        while let Some(material) = seq.next_element::<Material>()? {
            material_bank.register_material(material);
        }
        Ok(material_bank)
    }
}
