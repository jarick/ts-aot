use ts_aot_core::{Diagnostic, Type};
use ts_aot_ir_mir::{FunctionKind, MirDecl, MirExpr, MirStmt, RuntimeOp};
use ts_aot_passes::GENERATOR_DIAG_DEFERRED_METHOD;

mod common;

use common::{convert, count_runtime_ops, find_mir_function, has_errors};

fn has_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code.as_str() == code)
}

#[test]
fn generator_next_method_lowers_to_runtime_generator_next_op() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
            return 2;
        }
        function main(): i64 {
            const g = gen();
            g.next();
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count, 1,
        "main must emit exactly one GeneratorNext op for g.next()"
    );
}

#[test]
fn generator_local_type_is_propagated_from_call() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
        }
        function main(): i64 {
            const g = gen();
            g.next();
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count, 1,
        "lowering must propagate Generator type to local so dispatch fires (count={count}, diags={diags:?})"
    );
}

#[test]
fn generator_return_method_emits_diagnostic_in_mvp() {
    let (_, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
        }
        function main(): i64 {
            const g = gen();
            g.return(5);
            return 0;
        }
        ",
    );
    assert!(
        has_code(&diags, GENERATOR_DIAG_DEFERRED_METHOD),
        "g.return() must use the dedicated E0502 deferred-method code, got: {diags:?}"
    );
    let deferred = diags
        .iter()
        .find(|d| d.code.as_str() == GENERATOR_DIAG_DEFERRED_METHOD)
        .expect("expected an E0502 deferred-method diagnostic");
    assert!(
        deferred.message.contains("Generator.prototype.return")
            && deferred.message.contains("deferred")
            && deferred.message.contains(".next()"),
        "the E0502 diagnostic must carry the deferred-method message for .return(), got: {deferred:?}"
    );
}

#[test]
fn generator_throw_method_emits_diagnostic_in_mvp() {
    let (_, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
        }
        function main(): i64 {
            const g = gen();
            g.throw(new Error());
            return 0;
        }
        ",
    );
    assert!(
        has_code(&diags, GENERATOR_DIAG_DEFERRED_METHOD),
        "g.throw() must use the dedicated E0502 deferred-method code, got: {diags:?}"
    );
    let deferred = diags
        .iter()
        .find(|d| d.code.as_str() == GENERATOR_DIAG_DEFERRED_METHOD)
        .expect("expected an E0502 deferred-method diagnostic");
    assert!(
        deferred.message.contains("Generator.prototype.throw")
            && deferred.message.contains("deferred")
            && deferred.message.contains(".next()"),
        "the E0502 diagnostic must carry the deferred-method message for .throw(), got: {deferred:?}"
    );
}

#[test]
fn generator_unknown_method_emits_e0406_and_does_not_panic() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
            return 0;
        }
        function main(): i64 {
            const g = gen();
            g.foo();
            return 0;
        }
        ",
    );
    assert!(
        has_code(&diags, "E0406"),
        "g.foo() on a generator must emit E0406 (unknown method), got: {diags:?}"
    );
    let diag = diags
        .iter()
        .find(|d| d.code.as_str() == "E0406")
        .expect("E0406 must be present");
    assert!(
        diag.message.contains("Generator<i64>")
            && diag.message.contains("`next`")
            && diag.message.contains("`return`")
            && diag.message.contains("`throw`"),
        "E0406 message must include the receiver type `Generator<i64>` and the recognized methods, got: {:?}",
        diag.message
    );
    assert!(
        count_runtime_ops(&mir, RuntimeOp::GeneratorNext) == 0,
        "g.foo() must not emit a GeneratorNext op (only the recognized methods dispatch), got: {diags:?}"
    );
}

#[test]
fn non_generator_local_does_not_trigger_generator_method_dispatch() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function main(): i64 {
            const s: i64 = 42;
            s.next();
            return 0;
        }
        ",
    );
    assert_eq!(
        count_runtime_ops(&mir, RuntimeOp::GeneratorNext),
        0,
        "s.next() on a non-Generator must not emit a GeneratorNext op (dispatch is gated on the owner being a Generator<T>), got: {diags:?}"
    );
    assert!(
        !has_code(&diags, "E0501"),
        "s.next() on a non-Generator must not raise the E0501 generator-method-dispatch diagnostic, got: {diags:?}"
    );
}

#[test]
fn generator_type_propagation_does_not_corrupt_other_functions_locals() {
    let (mir, mut types, diags, hir_dump) = convert(
        r"
        function* gen(): i64 {
            yield 1;
        }
        function double(x: i64): i64 {
            return x * 2;
        }
        function main(): i64 {
            const g = gen();
            g.next();
            return double(21);
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let double = find_mir_function(&mir, "double").expect("double must be present");
    assert!(
        double
            .params
            .iter()
            .all(|p| matches!(types.resolve(p.ty), Some(Type::I64))),
        "generator propagation must not retype `double`'s params to Generator, got {:?}",
        double
            .params
            .iter()
            .map(|p| types.resolve(p.ty))
            .collect::<Vec<_>>()
    );
    let i64_ty = types.intern(&Type::I64);
    let gen_ty = types.intern(&Type::Generator { inner: i64_ty });
    let double_body = hir_dump
        .split_once("fn double")
        .map(|(_, rest)| rest.split_once("fn ").map_or(rest, |(seg, _)| seg))
        .unwrap_or("");
    let corrupted = format!("local(0):{}", gen_ty.raw());
    let correct = format!("local(0):{}", i64_ty.raw());
    assert!(
        !double_body.contains(&corrupted),
        "double's body must not reference Local(0) with Generator type; got HIR:\n{double_body}"
    );
    assert!(
        double_body.contains(&correct),
        "double's body must reference x (Local(0)) with i64 type; got HIR:\n{double_body}"
    );
}

#[test]
fn generator_copy_propagates_type_through_fixpoint() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
        }
        function main(): i64 {
            const g = gen();
            const g2 = g;
            g.next();
            g2.next();
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(
        count, 2,
        "both g.next() and g2.next() must dispatch (copy must propagate Generator type), got {count}, diags: {diags:?}"
    );
}

#[test]
fn generator_used_as_value_is_rejected() {
    let (_, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
        }
        function main(): i64 {
            const f = gen;
            return 0;
        }
        ",
    );
    assert!(
        has_code(&diags, "E0501"),
        "using a generator function as a value must be rejected, got: {diags:?}"
    );
}

#[test]
fn trailing_yield_gets_fallthrough_done_return() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            yield 1;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let dump = mir.dump_text();
    assert!(
        dump.contains("yield(1"),
        "trailing yield must survive to MIR, got MIR:\n{dump}"
    );
    let gen_fn = mir
        .declarations
        .iter()
        .find_map(|d| match d {
            MirDecl::Function(f) if f.name.as_str() == "gen" => Some(f),
            _ => None,
        })
        .expect("gen function must be present");
    let stmts = &gen_fn.body.block.stmts;
    let yield_idx = stmts
        .iter()
        .rposition(|s| matches!(s, MirStmt::Expr(MirExpr::Yield { .. })))
        .expect("expected at least one yield stmt in gen body");
    let post_yield = &stmts[yield_idx + 1..];
    assert!(
        matches!(post_yield.last(), Some(MirStmt::Return(None))),
        "the sequence AFTER the final `yield(1)` must end with a fallthrough \
         `MirStmt::Return(None)` (so the producer always completes), got post-yield stmts: \
         {post_yield:?}"
    );
}

#[test]
fn expression_position_yield_is_rejected() {
    let (_, _types, diags, _hir) = convert(
        r"
        function consume(v: i64): i64 {
            return v;
        }
        function* gen(): i64 {
            consume(yield 1);
            return 2;
        }
        ",
    );
    assert!(
        has_code(&diags, "E0501"),
        "yield in expression position must be rejected, got: {diags:?}"
    );
}

#[test]
fn generator_method_is_rejected() {
    let (_, _types, diags, _hir) = convert(
        r"
        class Counter {
            *values(): i64 {
                yield 1;
                return 2;
            }
        }
        ",
    );
    assert!(
        has_code(&diags, "E0501"),
        "generator methods must be rejected, got: {diags:?}"
    );
}

#[test]
fn parameterized_generator_is_supported() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(n: i64): i64 {
            yield n;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let dump = mir.dump_text();
    assert!(
        dump.contains("yield("),
        "parameterized generator must keep its yield, got MIR:\n{dump}"
    );
}

#[test]
fn cross_yield_local_is_supported() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            const x: i64 = 1;
            yield x;
            yield x + 1;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let dump = mir.dump_text();
    assert!(
        dump.matches("yield(").count() == 2,
        "both yields must survive to MIR, got MIR:\n{dump}"
    );
}

#[test]
fn block_local_used_within_same_block_is_accepted() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            const x: i64 = 1;
            yield x;
            return 2;
        }
        function main(): i64 {
            const g = gen();
            g.next();
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(count, 1, "diags: {diags:?}");
}

#[test]
fn block_local_keeps_its_own_mir_local() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            const x: i64 = 1;
            yield x;
            return 2;
        }
        function main(): i64 {
            const g = gen();
            g.next();
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let dump = mir.dump_text();
    assert!(
        dump.contains("yield(local(0)"),
        "yield must reference the body local `x` (local 0) directly; got MIR:\n{dump}"
    );
}

#[test]
fn compound_update_cross_yield_local_is_supported() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            let x: i64 = 1;
            yield 1;
            x += 1;
            yield 2;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let dump = mir.dump_text();
    assert!(
        dump.matches("yield(").count() == 2,
        "both yields must survive to MIR, got MIR:\n{dump}"
    );
}

#[test]
fn yield_inside_compound_update_is_rejected() {
    let (_, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            let x: i64 = 1;
            x += yield 1;
        }
        ",
    );
    assert!(
        has_code(&diags, "E0501"),
        "yield in expression position inside a compound update must be rejected, got: {diags:?}"
    );
}

#[test]
fn compound_update_within_single_block_is_accepted() {
    let (mir, _types, diags, _hir) = convert(
        r"
        function* gen(): i64 {
            let x: i64 = 1;
            x += 1;
            yield x;
            return 2;
        }
        function main(): i64 {
            const g = gen();
            g.next();
            return 0;
        }
        ",
    );
    assert!(!has_errors(&diags), "errors in: {diags:?}");
    let count = count_runtime_ops(&mir, RuntimeOp::GeneratorNext);
    assert_eq!(count, 1, "diags: {diags:?}");
}

#[test]
fn void_yield_type_with_valued_yield_is_rejected() {
    let (_, _types, diags, _hir) = convert(
        r"
        function* gen(): void {
            yield 1;
        }
        ",
    );
    assert!(
        has_code(&diags, "E0501"),
        "`yield <expr>;` with a void yield type must be rejected, got: {diags:?}"
    );
}

#[test]
fn generator_without_return_type_is_rejected() {
    let (_, _types, diags, _hir) = convert(
        r"
        function* gen() {
            yield 1;
        }
        ",
    );
    assert!(
        has_code(&diags, "E0501"),
        "generators without a resolvable yield type must be rejected, got: {diags:?}"
    );
}

#[test]
fn generator_method_mir_function_decl_carries_self_param_and_generator_status() {
    let (mir, _types, diags, _hir) = convert(
        r"
        class Counter {
            *stream(): i64 {
                yield 1;
                return 2;
            }
        }
        ",
    );
    assert!(
        has_code(&diags, "E0501"),
        "class generator method `stream` must be rejected by lower_generators with E0501; \
         the MIR assertions below validate best-effort lowering despite the rejection, \
         got: {diags:?}"
    );
    let struct_decl = mir
        .declarations
        .iter()
        .find_map(|d| match d {
            MirDecl::Struct(s) => Some(s),
            _ => None,
        })
        .expect("class with a generator method must produce a MirDecl::Struct");
    let method = struct_decl
        .methods
        .iter()
        .find(|m| m.name.as_str().ends_with("stream"))
        .expect("generator method `stream` must be lowered into the struct");
    assert_eq!(
        method.params.len(),
        1,
        "generator method must keep its self param after HIR-to-MIR conversion"
    );
    assert_eq!(
        method.params[0].id,
        ts_aot_core::LocalId::from_raw(0),
        "self_param LocalId must be the synthesized `self` at local 0"
    );
    assert_eq!(
        method.params[0].name.as_str(),
        "this",
        "self param must keep the `this` name produced by the convert path"
    );
    let FunctionKind::GeneratorMethod { owner, self_param } = method.kind else {
        panic!(
            "expected FunctionKind::GeneratorMethod, got {:?}",
            method.kind
        );
    };
    assert_eq!(
        owner, struct_decl.id,
        "GeneratorMethod must carry the owner StructId of its enclosing struct"
    );
    assert_eq!(
        self_param,
        ts_aot_core::LocalId::from_raw(0),
        "GeneratorMethod must carry the synthesized self_param LocalId at local 0"
    );
}
