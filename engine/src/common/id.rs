macro_rules! create_id_type {
    ($name: ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(pub u64);

        impl $name {
            pub const fn null() -> Self {
                Self(u64::MAX)
            }

            pub fn is_null(&self) -> bool {
                *self == Self::null()
            }
        }

        impl std::ops::Deref for $name {
            type Target = u64;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

pub(crate) use create_id_type;
