use crate::asset::asset::{AssetHandle, AssetPath, AssetStatus, Assets, GameAssetPath};
use crate::common::freelist::{FreeList, FreeListHandle};
use crate::event::Events;
use crate::material::material::MaterialSerializable;
use crate::material::{MaterialAsset, MaterialTextureType};
use crate::resource::ResMut;
use bitflags::__private::serde::de::SeqAccess;
use bitflags::__private::serde::ser::SerializeSeq;
use bitflags::__private::serde::{Deserializer, Serializer};
use rogue_macros::Resource;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub type MaterialAssetId = FreeListHandle<MaterialAsset>;

pub enum MaterialBankEvent {
    AssetCreated(MaterialAssetId),
    AssetUpdated {
        material_id: MaterialAssetId,
        updated_texture_type: MaterialTextureType,
    },
}

pub type MaterialId = u32;
pub const NULL_MATERIAL_ID: MaterialId = 0xFFFF_FFFF;

#[derive(Resource)]
pub struct MaterialBank {
    pub materials: FreeList<MaterialAsset>,
    pub asset_path_map: HashMap<GameAssetPath, MaterialAssetId>,

    /// Table which models use to lookup their material id to find the material asset id. This
    /// indirection is done so a material can have a different identity, allowing for masking while
    /// still using another materials asset. This is nice for prototyping materials and more
    /// flexibility.
    pub id_to_asset_map: HashMap<MaterialId, MaterialAssetId>,
    pub id_to_name: HashMap<MaterialId, String>,
    pub id_counter: MaterialId,

    loading_materials: HashSet<MaterialId>,
    to_load_material_assets: HashMap<GameAssetPath, Vec<MaterialId>>,

    to_send_events: Vec<MaterialBankEvent>,
}

impl MaterialBank {
    pub fn new() -> Self {
        Self {
            materials: FreeList::new(),
            asset_path_map: HashMap::new(),

            id_to_asset_map: HashMap::new(),
            id_to_name: HashMap::new(),
            id_counter: 0,

            loading_materials: HashSet::new(),
            to_load_material_assets: HashMap::new(),

            to_send_events: Vec::new(),
        }
    }

    pub fn next_free_id(&self) -> MaterialAssetId {
        return self.materials.next_free_handle();
    }

    pub fn find_first_material_by_name(&self, name: &str) -> Option<MaterialId> {
        self.id_to_name.iter().find_map(|(id, material_name)| {
            if material_name == name {
                Some(*id)
            } else {
                None
            }
        })
    }

    pub fn loading_materials(&self) -> bool {
        !self.to_load_material_assets.is_empty() || !self.loading_materials.is_empty()
    }

    // pub fn request_material(&mut self, material_) {
    //     if self.asset_path_map.contains_key(&asset_path) {
    //         return;
    //     }

    //     self.to_load_materials.push((None, asset_path));
    // }

    pub fn contains_material(&self, material_id: &MaterialAssetId) -> bool {
        return self.materials.get(*material_id).is_some();
    }

    pub fn get_material(&self, material_id: MaterialAssetId) -> Option<&MaterialAsset> {
        return self.materials.get(material_id);
    }

    pub fn set_material_asset(&mut self, material_id: MaterialId, asset_path: GameAssetPath) {
        if let Some(existing_asset_id) = self.id_to_asset_map.get(&material_id) {
            self.id_to_asset_map.insert(material_id, *existing_asset_id);
            return;
        }

        if self.loading_materials.contains(&material_id) {
            log::warn!(
                "Material with id {:?} is already loading, maybe with another asset path?",
                material_id
            );
            return;
        }

        self.to_load_material_assets
            .entry(asset_path.clone())
            .or_default()
            .push(material_id);
        self.loading_materials.insert(material_id);
    }

    //pub fn push_material(&mut self, material: MaterialAsset) {
    //    let id = self.next_free_id();
    //    //self.register_material(id.index(), material);
    //}

    //pub fn register_material(&mut self, id: u32, material: MaterialAsset) {
    //    let material_id = FreeListHandle::new(id, 0);
    //    if !self.materials.is_free(material_id) {
    //        log::info!("TODO: Do we overwrite the material and reupdate or not idk here");
    //        return;
    //    }
    //    if let Some(asset_path) = &material.asset_path {
    //        let res = self.asset_path_map.insert(asset_path.clone(), material_id);
    //        if res.is_some() {
    //            return;
    //        }
    //        //assert!(
    //        //    res.is_none(),
    //        //    "Tried to register material with asset path {:?} but that already exists",
    //        //    asset_path
    //        //);
    //    }
    //    self.materials
    //        .insert_in_place(FreeListHandle::new(id, 0), material);
    //    self.name_map.insert(material_name, material_id);
    //    self.create_events.push(MaterialCreateEvent { material_id });
    //}

    pub fn update_material_texture(
        &mut self,
        material_id: MaterialAssetId,
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

        self.to_send_events.push(MaterialBankEvent::AssetUpdated {
            material_id,
            updated_texture_type: texture_type,
        });
    }

    pub fn take_next_material_id(&mut self) -> MaterialId {
        let id = self.id_counter;
        self.id_counter += 1;
        id
    }

    pub fn create_material(&mut self, material_name: String) {
        let material_id = self.take_next_material_id();
        self.id_to_name.insert(material_id, material_name);
    }

    pub fn update_material_loading(
        mut material_bank: ResMut<MaterialBank>,
        mut assets: ResMut<Assets>,
        mut events: ResMut<Events>,
    ) {
        let material_bank = &mut *material_bank;
        let Some(assets_dir) = assets.project_assets_dir() else {
            return;
        };

        for (asset_path, material_ids) in material_bank.to_load_material_assets.drain() {
            let material_asset = Assets::load_asset_sync::<MaterialAsset>(
                AssetPath::new_game_assets_dir(assets_dir.clone(), &asset_path.asset_path),
            );
            match material_asset {
                Ok(mut loaded_asset) => {
                    loaded_asset.asset_path = Some(asset_path.clone());
                    let asset_id = material_bank.materials.push(loaded_asset.clone());
                    material_bank
                        .asset_path_map
                        .insert(asset_path.clone(), asset_id);
                    for material_id in material_ids {
                        material_bank.id_to_asset_map.insert(material_id, asset_id);
                        material_bank.loading_materials.remove(&material_id);
                    }
                    events.push(MaterialBankEvent::AssetCreated(asset_id));
                }
                Err(err) => {
                    log::error!(
                        "Error loading material asset at path {:?} for material {:?}: {}",
                        asset_path,
                        material_ids
                            .iter()
                            .map(|id| material_bank.id_to_name.get(id))
                            .collect::<Vec<_>>(),
                        err
                    );
                    for material_id in material_ids {
                        material_bank.loading_materials.remove(&material_id);
                    }
                }
            }
        }
    }

    /// Indirection so we can run this headless for gpu stuff.
    pub fn update_events(mut material_bank: ResMut<MaterialBank>, mut events: ResMut<Events>) {
        for event in material_bank.to_send_events.drain(..) {
            events.push(event);
        }
    }
}

impl serde::ser::Serialize for MaterialBank {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("MaterialBank", 2)?;
        s.serialize_field("id_counter", &self.id_counter)?;
        s.serialize_field(
            "materials",
            &MaterialBankMaterialsSerializer {
                material_bank: self,
            },
        )?;
        s.end()
    }
}

pub struct MaterialBankMaterialsSerializer<'a> {
    pub material_bank: &'a MaterialBank,
}

impl serde::ser::Serialize for MaterialBankMaterialsSerializer<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let material_bank = self.material_bank;
        let mut seq = serializer.serialize_seq(Some(material_bank.materials.len()))?;
        for (material_id, material_asset_id) in material_bank.id_to_asset_map.iter() {
            let asset_path = material_bank
                .id_to_asset_map
                .get(material_id)
                .and_then(|asset_id| {
                    material_bank
                        .materials
                        .get(*asset_id)
                        .unwrap()
                        .asset_path
                        .clone()
                });
            let material_name = material_bank.id_to_name.get(material_id).unwrap().clone();

            seq.serialize_element(&MaterialSerializable {
                id: *material_id,
                name: material_name,
                asset_path: asset_path,
            })?;
        }
        seq.end()
    }
}

pub struct MaterialBankDeserializer<'a> {
    pub material_bank: &'a mut MaterialBank,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum MaterialBankField {
    IdCounter,
    Materials,
}

impl<'de> serde::de::DeserializeSeed<'de> for MaterialBankDeserializer<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &["id_counter, materials"];
        deserializer.deserialize_struct(
            "MaterialBank",
            FIELDS,
            MaterialBankDeserializer {
                material_bank: self.material_bank,
            },
        )
    }
}

impl<'de> serde::de::Visitor<'de> for MaterialBankDeserializer<'_> {
    type Value = ();

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<MaterialBankField>()? {
            match key {
                MaterialBankField::IdCounter => {
                    self.material_bank.id_counter = map.next_value()?;
                }
                MaterialBankField::Materials => {
                    map.next_value_seed(MaterialBankMaterialsDeserializer {
                        material_bank: self.material_bank,
                    })?;
                }
            }
        }
        Ok(())
    }

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("MaterialBank struct")
    }
}

pub struct MaterialBankMaterialsDeserializer<'a> {
    pub material_bank: &'a mut MaterialBank,
}

impl<'de> serde::de::Visitor<'de> for MaterialBankMaterialsDeserializer<'_> {
    type Value = ();

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        while let Some(MaterialSerializable {
            id,
            name,
            asset_path,
        }) = seq.next_element::<MaterialSerializable>()?
        {
            self.material_bank.id_to_name.insert(id, name.clone());
            if let Some(asset_path) = asset_path {
                self.material_bank.set_material_asset(id, asset_path);
            }
        }

        Ok(())
    }

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("Material array")
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for MaterialBankMaterialsDeserializer<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}
