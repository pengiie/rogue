use serde::{de::DeserializeSeed, ser::SerializeStruct};

use crate::physics::{
    collider::ColliderDeserializeFnPtr,
    collider_registry::{ColliderId, ColliderRegistry},
};
use crate::entity::component::{GameComponent, GameComponentSerializeContext};

#[derive(Clone)]
pub struct EntityColliders {
    pub colliders: Vec<ColliderId>,
}

impl Default for EntityColliders {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityColliders {
    pub fn new() -> Self {
        Self {
            colliders: Vec::new(),
        }
    }
}

impl GameComponent for EntityColliders {
    const NAME: &str = "Colliders";

    fn is_constructible() -> bool {
        true
    }

    fn construct_component(dst_ptr: *mut u8) {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(Self::new()) };
    }

    fn clone_component(
        &self,
        ctx: &mut crate::entity::component::GameComponentCloneContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let new_colliders = self
            .colliders
            .iter()
            .map(|id| ctx.collider_registry.clone_collider(id))
            .collect::<Vec<_>>();

        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe {
            (dst_ptr as *mut Self).write(EntityColliders {
                colliders: new_colliders,
            })
        };
    }

    fn serialize_component(
        &self,
        ctx: &GameComponentSerializeContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        let mut seq = ser
            .erased_serialize_seq(Some(self.colliders.len()))
            .map_err(|err| erased_serde::convert_ser_error(err))?;
        let mut visitor = ColliderStructSerializeVisitor {
            collider_registry: ctx.collider_registry,
            collider_id: ColliderId::null(),
        };
        for collider in &self.colliders {
            visitor.collider_id = *collider;
            seq.erased_serialize_element(&visitor)
                .map_err(|err| erased_serde::convert_ser_error(err))?;
        }
        seq.erased_end();
        Ok(())
    }

    unsafe fn deserialize_component(
        ctx: &mut crate::entity::component::GameComponentDeserializeContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        let mut visitor = ColliderArrayDeserializeVisitor {
            collider_registry: ctx.collider_registry,
        };
        let collider_ids = visitor.deserialize(de)?;

        let colliders_component = Self {
            colliders: collider_ids,
        };
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(colliders_component) };
        Ok(())
    }
}

pub struct ColliderArrayDeserializeVisitor<'a> {
    collider_registry: &'a mut ColliderRegistry,
}

impl<'de> serde::de::DeserializeSeed<'de> for ColliderArrayDeserializeVisitor<'_> {
    type Value = Vec<ColliderId>;

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        de.deserialize_seq(self)
    }
}

impl<'de> serde::de::Visitor<'de> for ColliderArrayDeserializeVisitor<'_> {
    type Value = Vec<ColliderId>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("Collider[]")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut collider_visitor = ColliderStructDeserializeVisitor {
            collider_registry: self.collider_registry,
        };
        let mut collider_ids = Vec::new();
        while let Some(collider_id) = seq.next_element_seed(&mut collider_visitor)? {
            collider_ids.push(collider_id);
        }
        Ok(collider_ids)
    }
}

pub struct ColliderStructDeserializeVisitor<'a> {
    collider_registry: &'a mut ColliderRegistry,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ColliderStructField {
    Name,
    Data,
}

impl<'de> serde::de::DeserializeSeed<'de> for &'_ mut ColliderStructDeserializeVisitor<'_> {
    type Value = ColliderId;

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        const FIELDS: [&'static str; 2] = ["name", "data"];
        de.deserialize_struct("Collider", &FIELDS, self)
    }
}

impl<'de> serde::de::Visitor<'de> for &'_ mut ColliderStructDeserializeVisitor<'_> {
    type Value = ColliderId;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("Collider Struct")
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut collider_data_visitor = ColliderStructDataDeserializeVisitor {
            collider_de_method: None,
            dst_ptr: std::ptr::null_mut(),
        };

        let mut type_info = None;
        let mut data = None;
        while let Some(key) = map.next_key::<ColliderStructField>()? {
            match key {
                ColliderStructField::Name => {
                    if type_info.is_some() {
                        return Err(serde::de::Error::duplicate_field("name"));
                    }
                    let name = map.next_value::<String>()?;
                    type_info = self
                        .collider_registry
                        .collider_type_info
                        .get(&name)
                        .map(|type_info| type_info.clone());
                    if type_info.is_none() {
                        return Err(serde::de::Error::custom(
format!("Tried to deserialize collider with Collider::NAME `{}` but it is not registered in the ColliderRegistry, cant get type info.", name)
                        ));
                    }
                }
                ColliderStructField::Data => {
                    let Some(type_info) = type_info else {
                        return Err(serde::de::Error::custom(
                            "Expect `name` to come before `data`.",
                        ));
                    };
                    if data.is_some() {
                        return Err(serde::de::Error::duplicate_field("data"));
                    }

                    collider_data_visitor.dst_ptr =
                        unsafe { std::alloc::alloc(type_info.layout(1)) };
                    if collider_data_visitor.dst_ptr.is_null() {
                        panic!("Failed to allocate collider");
                    }

                    let de_fn = self
                        .collider_registry
                        .collider_deserialize_fns
                        .get(&type_info.type_id())
                        .unwrap_or_else(|| panic!("Type info for {:?} exists but deserialize fn doesn't, something must have gone wrong when registering the collider type.", self.collider_registry.collider_names.get(&type_info.type_id())));
                    collider_data_visitor.collider_de_method = Some(*de_fn);

                    map.next_value_seed(&mut collider_data_visitor)?;
                    data = Some(collider_data_visitor.dst_ptr);
                    // Make null again to catch any accidental second uses.
                    collider_data_visitor.dst_ptr = std::ptr::null_mut();
                }
            }
        }

        let type_info = type_info.ok_or_else(|| serde::de::Error::missing_field("name"))?;
        let data = data.ok_or_else(|| serde::de::Error::missing_field("data"))?;

        // Safety: We allocate data with the same layout as type_info.
        let collider_id = unsafe {
            self.collider_registry
                .register_collider_raw(&type_info, data)
        };

        Ok(collider_id)
    }
}

pub struct ColliderStructDataDeserializeVisitor {
    collider_de_method: Option<ColliderDeserializeFnPtr>,
    dst_ptr: *mut u8,
}

impl<'de> serde::de::DeserializeSeed<'de> for &mut ColliderStructDataDeserializeVisitor {
    type Value = ();

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut erased_de = <dyn erased_serde::Deserializer>::erase(de);
        unsafe {
            // Collider::deserialize_collider(..)
            (self.collider_de_method.unwrap())(&mut erased_de, self.dst_ptr)
                .map_err(|err| serde::de::Error::custom(err))
        }?;
        Ok(())
    }
}

pub struct ColliderStructSerializeVisitor<'a> {
    collider_registry: &'a ColliderRegistry,
    collider_id: ColliderId,
}

impl serde::Serialize for ColliderStructSerializeVisitor<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = se.serialize_struct("Collider", 2)?;
        s.serialize_field(
            "name",
            self.collider_registry
                .collider_names
                .get(&self.collider_id.collider_type)
                .expect("Somehow collider type is not a register collider type."),
        )?;
        s.serialize_field(
            "data",
            &ColliderStructDataSerializeVisitor {
                collider_registry: self.collider_registry,
                collider: self.collider_id,
            },
        )?;
        s.end()
    }
}

pub struct ColliderStructDataSerializeVisitor<'a> {
    collider_registry: &'a ColliderRegistry,
    collider: ColliderId,
}

impl serde::Serialize for ColliderStructDataSerializeVisitor<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut erased_se = <dyn erased_serde::Serializer>::erase(se);
        // The actual result is stored for later.
        let _ = self
            .collider_registry
            .get_collider_dyn(&self.collider)
            .serialize_collider(&mut erased_se);
        erased_se.result()
    }
}
