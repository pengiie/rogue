use std::collections::HashMap;

use serde::ser::SerializeMap;

use crate::{
    common::dyn_vec::{DynVecCloneable, TypeInfo, TypeInfoCloneable},
    engine::physics::{
        box_collider::BoxCollider,
        capsule_collider::CapsuleCollider,
        collider::{Collider, ColliderType},
        collider_registry::ColliderRegistry,
    },
};

pub struct ColliderRegistryAsset {
    pub colliders: HashMap<ColliderType, DynVecCloneable>,
}

impl Default for ColliderRegistryAsset {
    fn default() -> Self {
        Self::new()
    }
}

impl ColliderRegistryAsset {
    pub fn new() -> Self {
        Self {
            colliders: HashMap::new(),
        }
    }

    pub fn register_collider<C: Collider + Clone + 'static>(&mut self, collider: C) {
        let collider_type = collider.collider_type();

        let vec = self
            .colliders
            .entry(collider_type)
            .or_insert(DynVecCloneable::new(TypeInfoCloneable::new::<C>()));
        vec.push(collider);
    }
}

impl From<&ColliderRegistry> for ColliderRegistryAsset {
    fn from(registry: &ColliderRegistry) -> Self {
        return ColliderRegistryAsset {
            colliders: registry.colliders.clone(),
        };
    }
}

impl serde::Serialize for ColliderRegistryAsset {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = ser.serialize_map(None)?;
        for (collider_type, colliders) in self.colliders.iter() {
            match collider_type {
                ColliderType::Null => {}
                ColliderType::Capsule => {
                    map.serialize_entry(
                        collider_type,
                        &colliders.iter::<CapsuleCollider>().collect::<Vec<_>>(),
                    );
                }
                ColliderType::Plane => {}
                ColliderType::Box => {
                    map.serialize_entry(
                        collider_type,
                        &colliders.iter::<BoxCollider>().collect::<Vec<_>>(),
                    );
                }
            }
        }
        map.end()
    }
}

struct ColliderRegistryVisitor;

impl<'de> serde::de::Visitor<'de> for ColliderRegistryVisitor {
    type Value = ColliderRegistryAsset;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("Was expecting collider stuff things.")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut registry = ColliderRegistryAsset::new();
        while let Ok(e) = map.next_key::<ColliderType>() {
            match e {
                Some(collider_type) => match collider_type {
                    ColliderType::Null => {}
                    ColliderType::Capsule => {
                        if let Ok(capsules) = map.next_value::<Vec<CapsuleCollider>>() {
                            for capsule in capsules {
                                registry.register_collider(capsule);
                            }
                        }
                    }
                    ColliderType::Plane => {}
                    ColliderType::Box => {
                        if let Ok(boxes) = map.next_value::<Vec<BoxCollider>>() {
                            for b in boxes {
                                registry.register_collider(b);
                            }
                        }
                    }
                },
                None => break,
            }
        }

        return Ok(registry);
    }
}

impl<'de> serde::Deserialize<'de> for ColliderRegistryAsset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(ColliderRegistryVisitor)
    }
}
