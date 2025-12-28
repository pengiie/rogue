macro_rules! impl_unit_type_serde {
    ($type_name:ty) => {
        impl serde::Serialize for $type_name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_unit_struct(stringify!($type_name))
            }
        }

        impl<'de> serde::de::Deserialize<'de> for $type_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::de::Deserializer<'de>,
            {
                struct UnitStructVisitor;

                impl<'de> serde::de::Visitor<'de> for UnitStructVisitor {
                    type Value = $type_name;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("unit struct")
                    }

                    fn visit_unit<E>(self) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        Ok(<$type_name as std::default::Default>::default())
                    }
                }

                deserializer.deserialize_unit_struct(stringify!($type_name), UnitStructVisitor)
            }
        }
    };
}
pub(crate) use impl_unit_type_serde;
