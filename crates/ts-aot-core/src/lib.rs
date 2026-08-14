mod canonical_index;
mod diagnostics;
mod ids;
mod rust_ident;
mod ty;
mod type_table;
mod visibility;

pub use canonical_index::canonical_integer_index;
pub use diagnostics::{Diagnostic, DiagnosticBag, DiagnosticCode, Severity};
pub use ids::{
    Atom, ClosureId, EnumId, ErrorId, FieldId, FunctionId, GenericParamId, LocalId, ModuleId,
    STRUCT_ID_DYNAMIC, StructId, TypeId, UnionId, VariantId,
};
pub use oxc_span::Span;
pub use rust_ident::sanitize_rust_ident;
pub use ty::{MemoryKind, Type};
pub use type_table::TypeTable;
pub use visibility::Visibility;

pub const MAX_DENSE_ARRAY_LEN: u32 = 1 << 24;
