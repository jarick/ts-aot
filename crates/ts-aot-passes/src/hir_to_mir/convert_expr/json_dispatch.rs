use std::collections::HashMap;

use ts_aot_core::{Span, StructId, Type, TypeId, TypeTable};
use ts_aot_ir_hir::{HirCallee, HirExpr};
use ts_aot_ir_mir::{MirExpr, MirStmt, RuntimeOp};

use crate::PassContext;
use crate::hir_to_mir::convert_expr::util::hir_expr_type_id;
use crate::hir_to_mir::converter::ExprConverter;

use super::globals::{
    JsonOpKind, is_json_supported_target_type, is_string_typed_source, json_target_type_name,
};

impl ExprConverter {
    pub(in crate::hir_to_mir::convert_expr) fn try_json_static_method_dispatch(
        &mut self,
        callee: &HirCallee,
        args: &[HirExpr],
        ty: TypeId,
        out: &mut Vec<MirStmt>,
        shared_struct_ids: &mut HashMap<TypeId, StructId>,
        shared_next_struct: &mut u32,
        types: &mut TypeTable,
        ctx: &mut PassContext,
    ) -> Option<MirExpr> {
        let HirCallee::Indirect(inner) = callee else {
            return None;
        };
        let HirExpr::Field {
            owner: json_owner,
            field_name: json_field,
            ..
        } = inner.as_ref()
        else {
            return None;
        };
        if !super::globals::is_global_json_reference(json_owner) {
            return None;
        }
        let op_kind = match json_field.as_str() {
            "parse" => JsonOpKind::Parse,
            "stringify" => JsonOpKind::Stringify,
            _ => return None,
        };
        let op_name = op_kind.name();
        if args.len() != 1 {
            ctx.error(
                "E0406",
                format!(
                    "JSON.{op_name} requires exactly 1 argument; got {}",
                    args.len()
                ),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        if op_kind == JsonOpKind::Parse && !is_string_typed_source(&args[0], types) {
            ctx.error(
                "E0406",
                "JSON.parse() argument must be a string (per ECMAScript spec); got non-string \
                 expression. Coerce the argument to a string (e.g. String(n)) before calling \
                 JSON.parse()."
                    .to_string(),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let target_ty_result: Result<TypeId, &'static str> = match op_kind {
            JsonOpKind::Parse => self
                .current_call_type_args
                .first()
                .copied()
                .ok_or("missing type argument"),
            JsonOpKind::Stringify => {
                hir_expr_type_id(&args[0]).ok_or("value with unresolvable type")
            }
        };
        let target_ty = match target_ty_result {
            Ok(t) => t,
            Err(reason) => {
                let hint = match op_kind {
                    JsonOpKind::Parse => {
                        "JSON.parse requires an explicit type argument \
                         `JSON.parse<T>(text)`; type-inference from the call result type or \
                         surrounding context is not supported in this AOT target. The type \
                         argument <T> must be a primitive type (i64, f64, bool, \
                         string/JsString) or a Vec<T>/Option<T> aggregate. User struct types \
                         are deferred (require #[derive(Deserialize)] on the user type)."
                    }
                    JsonOpKind::Stringify => {
                        "JSON.stringify requires the value argument to \
                         carry a statically-known type; untyped expressions (numeric \
                         literals, computed values, or expressions whose type is not \
                         resolved at the HIR level) cannot be used. The value type must be a \
                         primitive type (i64, f64, bool, string/JsString) or a Vec<T>/Option<T> \
                         aggregate. User struct types are deferred (require \
                         #[derive(Serialize)] on the user type)."
                    }
                };
                ctx.error(
                    "E0406",
                    format!("JSON.{op_name} {reason}: {hint}"),
                    Span::new(0, 0),
                );
                return Some(MirExpr::Unit);
            }
        };
        if !is_json_supported_target_type(types, target_ty) {
            let ty_desc = json_target_type_name(types, target_ty);
            let hint = match op_kind {
                JsonOpKind::Parse => {
                    "JSON.parse is generic in this AOT target; the type \
                     argument <T> must be a primitive type (i64, f64, bool, \
                     string/JsString) or a Vec<T>/Option<T> aggregate. User struct types are \
                     deferred (require #[derive(Deserialize)] on the user type)."
                }
                JsonOpKind::Stringify => {
                    "JSON.stringify target type must be a primitive \
                     type (i64, f64, bool, string/JsString) or a Vec<T>/Option<T> aggregate. \
                     User struct types are deferred (require #[derive(Serialize)] on the user \
                     type)."
                }
            };
            ctx.error(
                "E0406",
                format!("JSON.{op_name} target type `{ty_desc}` is not supported: {hint}"),
                Span::new(0, 0),
            );
            return Some(MirExpr::Unit);
        }
        let op = match (op_kind, types.resolve(target_ty)) {
            (JsonOpKind::Parse, Some(Type::String)) => RuntimeOp::JsonParseString,
            (JsonOpKind::Parse, _) => RuntimeOp::JsonParse,
            (JsonOpKind::Stringify, Some(Type::String)) => RuntimeOp::JsonStringifyString,
            (JsonOpKind::Stringify, _) => RuntimeOp::JsonStringify,
        };
        let mir_args: Vec<MirExpr> = args
            .iter()
            .map(|a| self.convert_expr(a, out, shared_struct_ids, shared_next_struct, types, ctx))
            .collect();
        let dest = self.fresh_local();
        self.push_temp_local(dest, ty);
        out.push(MirStmt::Runtime {
            op,
            args: mir_args,
            dest: Some(dest),
            ty,
            target_ty: Some(target_ty),
        });
        Some(MirExpr::Local(dest))
    }
}
