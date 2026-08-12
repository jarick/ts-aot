use ts_aot_core::{Type, TypeId, TypeTable, canonical_integer_index};
use ts_aot_ir_hir::{HirCallee, HirExpr, ObjectLiteralField};

use crate::hir_to_mir::convert_expr::util::hir_expr_type_id;

pub(super) fn is_global_object_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Object")
}

pub(super) fn is_global_array_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Array")
}

pub(super) fn is_global_math_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Math")
}

pub(super) fn is_global_string_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "String")
}

pub(super) fn is_global_date_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Date")
}

pub(super) fn is_global_json_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "JSON")
}

pub(super) fn is_global_symbol_reference(owner: &HirExpr) -> bool {
    matches!(owner, HirExpr::Global { name, .. } if name.as_str() == "Symbol")
}

pub(super) fn is_global_typed_array_reference(owner: &HirExpr) -> bool {
    let HirExpr::Global { name, .. } = owner else {
        return false;
    };
    matches!(
        name.as_str(),
        "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
    )
}

pub(super) fn is_string_typed_source(arg: &HirExpr, types: &TypeTable) -> bool {
    if matches!(arg, HirExpr::String(_, _)) {
        return true;
    }
    let Some(ty) = hir_expr_type_id(arg) else {
        return false;
    };
    matches!(types.resolve(ty), Some(Type::String))
}

pub(super) fn is_array_static_type(arg: &HirExpr, types: &TypeTable) -> bool {
    let Some(ty) = hir_expr_type_id(arg) else {
        return false;
    };
    matches!(types.resolve(ty), Some(Type::Array { .. }))
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum JsonOpKind {
    Parse,
    Stringify,
}

impl JsonOpKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Stringify => "stringify",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum SymbolArgKind {
    String,
    Symbol,
}

pub(super) struct ArrayLikeObjectLiteral {
    pub(super) length: i128,
    pub(super) indexed: Vec<(u64, HirExpr)>,
}

pub(super) fn array_from_object_literal(
    callee: &HirCallee,
    args: &[HirExpr],
) -> Option<ArrayLikeObjectLiteral> {
    let HirCallee::Indirect(inner) = callee else {
        return None;
    };
    let HirExpr::Field {
        owner, field_name, ..
    } = inner.as_ref()
    else {
        return None;
    };
    if !is_global_array_reference(owner) || field_name.as_str() != "from" {
        return None;
    }
    if args.is_empty() || args.len() > 3 {
        return None;
    }
    let HirExpr::ObjectLiteral { fields, .. } = &args[0] else {
        return None;
    };
    let mut length: Option<i128> = None;
    let mut indexed: Vec<(u64, HirExpr)> = Vec::new();
    for field in fields {
        let ObjectLiteralField::Property { name, value } = field else {
            return None;
        };
        if name.as_str() == "length" {
            let HirExpr::Int(value, _) = value else {
                return None;
            };
            length = Some(i128::from(*value));
        } else {
            let idx = canonical_integer_index(name.as_str())?;
            indexed.push((idx, value.clone()));
        }
    }
    let length = length?;
    indexed.sort_by_key(|(idx, _)| *idx);
    Some(ArrayLikeObjectLiteral { length, indexed })
}

pub(super) fn is_json_supported_target_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.resolve(ty) {
        Some(Type::Bool | Type::I64 | Type::F64 | Type::String) => true,
        Some(Type::Optional { inner }) => is_json_supported_target_type(types, *inner),
        Some(Type::Array { element }) => is_json_supported_target_type(types, *element),
        _ => false,
    }
}

pub(super) fn json_target_type_name(types: &TypeTable, ty: TypeId) -> String {
    match types.resolve(ty) {
        Some(Type::Bool) => "bool".to_string(),
        Some(Type::I64) => "i64".to_string(),
        Some(Type::F64) => "f64".to_string(),
        Some(Type::String) => "string".to_string(),
        Some(Type::Symbol) => "symbol".to_string(),
        Some(Type::Optional { inner }) => {
            format!("Option<{}>", json_target_type_name(types, *inner))
        }
        Some(Type::Array { element }) => {
            format!("Vec<{}>", json_target_type_name(types, *element))
        }
        Some(Type::Struct { id }) => format!("struct#{id:?}"),
        Some(Type::Named { symbol }) => symbol.as_str().to_string(),
        _ => format!("<unresolved #{ty:?}>"),
    }
}

pub(super) fn type_label(types: &TypeTable, ty: TypeId) -> String {
    match types.resolve(ty) {
        Some(Type::Void) => "void".to_string(),
        Some(Type::Null) => "null".to_string(),
        Some(Type::Never) => "never".to_string(),
        Some(Type::Error) => "<error>".to_string(),
        Some(Type::Symbol) => "symbol".to_string(),
        Some(Type::I8) => "i8".to_string(),
        Some(Type::I16) => "i16".to_string(),
        Some(Type::I32) => "i32".to_string(),
        Some(Type::U8) => "u8".to_string(),
        Some(Type::U16) => "u16".to_string(),
        Some(Type::U32) => "u32".to_string(),
        Some(Type::U64) => "u64".to_string(),
        Some(Type::F32) => "f32".to_string(),
        Some(Type::Date) => "date".to_string(),
        Some(Type::ArrayBuffer) => "ArrayBuffer".to_string(),
        Some(Type::Optional { inner }) => format!("Option<{}>", type_label(types, *inner)),
        Some(Type::Array { element }) => format!("Vec<{}>", type_label(types, *element)),
        Some(Type::Struct { id }) => format!("struct#{id:?}"),
        Some(Type::Named { symbol }) => symbol.as_str().to_string(),
        _ => json_target_type_name(types, ty),
    }
}
