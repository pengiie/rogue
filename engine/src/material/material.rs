use std::collections::HashMap;
use std::path::Path;

use rogue_macros::Resource;
use serde::ser::{SerializeSeq, SerializeStruct};

use crate::asset::asset::{AssetHandle, AssetPath, AssetStatus, Assets, GameAssetPath};
use crate::common::freelist::{FreeList, FreeListHandle};
use crate::event::Events;
use crate::impl_asset_load_save_serde;
use crate::resource::ResMut;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaterialTextureType {
    Color,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Material {
    pub name: String,
    pub color_texture: Option<GameAssetPath>,
    #[serde(skip)]
    pub asset_path: Option<GameAssetPath>,
}

impl Material {
    pub fn is_empty(&self) -> bool {
        return self.color_texture.is_none();
    }
}

impl_asset_load_save_serde!(Material);

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
    pub asset_path_map: HashMap<GameAssetPath, MaterialId>,

    to_load_materials: Vec<(Option<MaterialId>, GameAssetPath)>,
    loading_materials: Vec<(Option<MaterialId>, AssetHandle)>,

    create_events: Vec<MaterialCreateEvent>,
    update_events: Vec<MaterialUpdateEvent>,
}

impl MaterialBank {
    pub fn new() -> Self {
        Self {
            materials: FreeList::new(),
            name_map: HashMap::new(),
            asset_path_map: HashMap::new(),

            to_load_materials: Vec::new(),
            loading_materials: Vec::new(),

            create_events: Vec::new(),
            update_events: Vec::new(),
        }
    }

    pub fn next_free_id(&self) -> MaterialId {
        return self.materials.next_free_handle();
    }

    pub fn request_material(&mut self, asset_path: GameAssetPath) {
        if self.asset_path_map.contains_key(&asset_path) {
            return;
        }

        self.to_load_materials.push((None, asset_path));
    }

    pub fn contains_material(&self, material_id: &MaterialId) -> bool {
        return self.materials.get(*material_id).is_some();
    }

    pub fn get_material(&self, material_id: MaterialId) -> Option<&Material> {
        return self.materials.get(material_id);
    }

    pub fn push_material(&mut self, material: Material) {
        let id = self.next_free_id();
        self.register_material(id.index(), material);
    }

    pub fn register_material(&mut self, id: u32, material: Material) {
        if let Some(_) = self.name_map.get(&material.name) {
            return;
            //panic!(
            //    "Tried to register material with name {} but that already exits",
            //    material.name
            //);
        }

        let material_name = material.name.clone();
        let material_id = FreeListHandle::new(id, 0);
        if !self.materials.is_free(material_id) {
            log::info!("TODO: Do we overwrite the material and reupdate or not idk here");
            return;
        }
        if let Some(asset_path) = &material.asset_path {
            let res = self.asset_path_map.insert(asset_path.clone(), material_id);
            if res.is_some() {
                return;
            }
            //assert!(
            //    res.is_none(),
            //    "Tried to register material with asset path {:?} but that already exists",
            //    asset_path
            //);
        }
        self.materials
            .insert_in_place(FreeListHandle::new(id, 0), material);
        self.name_map.insert(material_name, material_id);
        self.create_events.push(MaterialCreateEvent { material_id });
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

    pub fn update_material_loading(
        mut material_bank: ResMut<MaterialBank>,
        mut assets: ResMut<Assets>,
    ) {
        let material_bank = &mut *material_bank;
        let Some(assets_dir) = assets.project_assets_dir() else {
            return;
        };

        for (material_id, asset_path) in material_bank.to_load_materials.drain(..) {
            let asset_handle = assets.load_asset::<Material>(AssetPath::new_game_assets_dir(
                assets_dir.clone(),
                &asset_path.asset_path,
            ));
            material_bank
                .loading_materials
                .push((material_id, asset_handle));
        }

        let mut finished_materials = Vec::new();
        for (i, (material_id, asset_handle)) in material_bank.loading_materials.iter().enumerate() {
            match assets.get_asset_status(asset_handle) {
                AssetStatus::InProgress => {}
                AssetStatus::Saved => {
                    unreachable!("If material is loading it should not be saving.")
                }
                AssetStatus::Loaded => {
                    let mut material = assets.get_asset::<Material>(asset_handle).unwrap().clone();
                    material.asset_path =
                        Some(asset_handle.asset_path().asset_path.clone().unwrap());
                    finished_materials.push((i, (*material_id, Some(material))));
                }
                AssetStatus::NotFound => {
                    log::error!(
                        "Material asset not found at path {:?} for material id {:?}",
                        asset_handle.asset_path(),
                        material_id.map(|m| m.index())
                    );
                    finished_materials.push((i, (*material_id, None)));
                }
                AssetStatus::Error(error) => {
                    log::error!(
                        "Error loading material asset at path {:?} for material id {:?}: {}",
                        asset_handle.asset_path(),
                        material_id.map(|m| m.index()),
                        error
                    );
                    finished_materials.push((i, (*material_id, None)));
                }
            }
        }

        for (i, (material_id, material)) in finished_materials.into_iter().rev() {
            material_bank.loading_materials.swap_remove(i);
            if let Some(material) = material {
                if let Some(material_id) = material_id {
                    material_bank.register_material(material_id.index(), material);
                } else {
                    material_bank.push_material(material);
                }
            }
        }
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

struct MaterialSerializable {
    id: MaterialId,
    asset_path: GameAssetPath,
}

impl serde::ser::Serialize for MaterialSerializable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let mut state = serializer.serialize_struct("MaterialSerializable", 2)?;
        state.serialize_field("id", &self.id.index())?;
        state.serialize_field("asset", &self.asset_path)?;
        state.end()
    }
}

impl<'de> serde::de::Deserialize<'de> for MaterialSerializable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct MaterialSerializableHelper {
            id: u32,
            asset: GameAssetPath,
        }

        let helper = MaterialSerializableHelper::deserialize(deserializer)?;
        Ok(MaterialSerializable {
            id: FreeListHandle::new(helper.id, 0),
            asset_path: helper.asset,
        })
    }
}

impl serde::ser::Serialize for MaterialBank {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.materials.len()))?;
        for (material_id, material) in self.materials.iter_with_handle() {
            let Some(asset_path) = &material.asset_path else {
                continue;
            };

            seq.serialize_element(&MaterialSerializable {
                id: material_id,
                asset_path: asset_path.clone(),
            })?;
        }
        seq.end()
    }
}

pub struct MaterialBankDeserializer<'a> {
    pub material_bank: &'a mut MaterialBank,
}

impl<'de> serde::de::Visitor<'de> for MaterialBankDeserializer<'_> {
    type Value = ();

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        while let Some(material_serializable) = seq.next_element::<MaterialSerializable>()? {
            self.material_bank.to_load_materials.push((
                Some(material_serializable.id),
                material_serializable.asset_path,
            ));
        }

        Ok(())
    }

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("Material array")
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for MaterialBankDeserializer<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}
