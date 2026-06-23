macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u32);

        impl $name {
            #[must_use]
            pub const fn from_raw(raw: u32) -> Self {
                Self(raw)
            }

            #[must_use]
            pub const fn raw(self) -> u32 {
                self.0
            }
        }

        impl From<u32> for $name {
            fn from(raw: u32) -> Self {
                Self::from_raw(raw)
            }
        }

        impl From<$name> for u32 {
            fn from(id: $name) -> Self {
                id.raw()
            }
        }
    };
}

define_id!(TypeId);
define_id!(StructId);
define_id!(FunctionId);
define_id!(FieldId);
define_id!(LocalId);
define_id!(SymbolId);
define_id!(StringId);
define_id!(UnionId);
define_id!(EnumId);
define_id!(VariantId);
define_id!(ModuleId);
define_id!(AwaitPointId);
define_id!(ClosureId);
define_id!(AsyncTaskId);
define_id!(ErrorId);
define_id!(GenericParamId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct_for_distinct_raw_values() {
        assert_ne!(TypeId::from_raw(0), TypeId::from_raw(1));
        assert_eq!(TypeId::from_raw(7), TypeId::from_raw(7));
    }

    #[test]
    fn raw_roundtrips() {
        assert_eq!(TypeId::from_raw(42).raw(), 42);
    }

    #[test]
    fn from_u32_matches_from_raw() {
        assert_eq!(TypeId::from(99u32), TypeId::from_raw(99));
    }

    #[test]
    fn into_u32_matches_raw() {
        let id = TypeId::from_raw(123);
        let raw: u32 = id.into();
        assert_eq!(raw, 123);
    }
}
