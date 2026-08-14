use std::collections::HashSet;

use ts_aot_core::{Atom, Span, Type, TypeTable};
use ts_aot_ir_hir::{HirDecl, HirFunction, HirProgram, HirStmt};

use crate::PassContext;
use crate::hir_to_mir::qualified_name;

mod diagnostics;
mod type_propagation;

use diagnostics::{GENERATOR_DIAG_UNSUPPORTED_YIELD, analyze_generator_body, first_body_span};
use type_propagation::propagate_generator_types;

pub use diagnostics::GENERATOR_DIAG_DEFERRED_METHOD;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LowerGeneratorsStats {
    pub generators_transformed: usize,
    pub generators_rejected: usize,
}

pub fn lower_generators(
    program: &mut HirProgram,
    types: &mut TypeTable,
    ctx: &mut PassContext,
) -> LowerGeneratorsStats {
    let mut stats = LowerGeneratorsStats::default();

    let mut generator_fn_names: HashSet<Atom> = HashSet::new();
    let mut non_generator_names: HashSet<Atom> = HashSet::new();
    process_decls(
        &mut program.declarations,
        types,
        ctx,
        &mut generator_fn_names,
        &mut non_generator_names,
        &mut stats,
        &[],
    );

    reject_generator_methods(program, ctx, &mut stats);
    reject_nested_generator_decls(&program.declarations, ctx, &mut stats);

    if !generator_fn_names.is_empty() {
        propagate_generator_types(
            program,
            types,
            ctx,
            &generator_fn_names,
            &non_generator_names,
        );
    }
    stats
}

fn process_decls(
    decls: &mut [HirDecl],
    types: &mut TypeTable,
    ctx: &mut PassContext,
    generator_fn_names: &mut HashSet<Atom>,
    non_generator_names: &mut HashSet<Atom>,
    stats: &mut LowerGeneratorsStats,
    namespace_path: &[String],
) {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                if f.is_generator {
                    process_generator_function(
                        f,
                        types,
                        ctx,
                        generator_fn_names,
                        stats,
                        namespace_path,
                    );
                } else {
                    non_generator_names.insert(qualified_name(namespace_path, f.name.as_str()));
                }
            }
            HirDecl::Class(c) => {
                non_generator_names.insert(qualified_name(namespace_path, c.name.as_str()));
            }
            HirDecl::Global { name, .. } => {
                non_generator_names.insert(qualified_name(namespace_path, name.as_str()));
            }
            HirDecl::Enum { name, .. } => {
                non_generator_names.insert(qualified_name(namespace_path, name.as_str()));
            }
            HirDecl::Namespace { name, members } => {
                let mut child_path = namespace_path.to_vec();
                child_path.push(name.as_str().to_owned());
                process_decls(
                    members,
                    types,
                    ctx,
                    generator_fn_names,
                    non_generator_names,
                    stats,
                    &child_path,
                );
            }
            HirDecl::TypeAlias { .. } | HirDecl::Interface { .. } => {}
        }
    }
}

fn process_generator_function(
    f: &mut HirFunction,
    types: &mut TypeTable,
    ctx: &mut PassContext,
    generator_fn_names: &mut HashSet<Atom>,
    stats: &mut LowerGeneratorsStats,
    namespace_path: &[String],
) {
    let analysis = analyze_generator_body(&f.body);
    let inner_ty = f.ret;
    let inner_resolved = types.resolve(inner_ty);
    let inner_is_error = matches!(inner_resolved, Some(Type::Error) | None);
    if inner_is_error {
        let span = analysis
            .first_valued_yield_span
            .or(analysis.first_throw_span)
            .or(analysis.first_expression_yield_span)
            .or_else(|| first_body_span(&f.body))
            .unwrap_or_else(|| Span::new(0, 0));
        ctx.error(
            GENERATOR_DIAG_UNSUPPORTED_YIELD,
            "generator functions must declare their yield type \
             (e.g. `function* gen(): i64 { yield 1; }`)",
            span,
        );
        stats.generators_rejected += 1;
        return;
    }

    if matches!(inner_resolved, Some(Type::Void)) && analysis.has_valued_yield {
        let span = analysis
            .first_valued_yield_span
            .unwrap_or_else(|| Span::new(0, 0));
        ctx.error(
            GENERATOR_DIAG_UNSUPPORTED_YIELD,
            "`yield <expr>;` requires the generator to declare a non-void yield type \
             (e.g. `function* gen(): i64 { yield 1; }`)",
            span,
        );
        stats.generators_rejected += 1;
        return;
    }

    if let Some(span) = analysis.first_expression_yield_span {
        ctx.error(
            GENERATOR_DIAG_UNSUPPORTED_YIELD,
            "yield in expression position is not supported yet \
             (use a statement-level `yield <expr>;` instead)",
            span,
        );
        stats.generators_rejected += 1;
        return;
    }

    if analysis.has_throw {
        let span = analysis.first_throw_span.unwrap_or_else(|| Span::new(0, 0));
        ctx.error(
            GENERATOR_DIAG_UNSUPPORTED_YIELD,
            "throw inside a generator is not supported yet \
             (throw before the generator is created instead)",
            span,
        );
        stats.generators_rejected += 1;
        return;
    }

    if analysis.has_await {
        let span = analysis.first_await_span.unwrap_or_else(|| Span::new(0, 0));
        ctx.error(
            GENERATOR_DIAG_UNSUPPORTED_YIELD,
            "await inside a generator is not supported yet \
             (the generator body is wrapped in `async move` but `Generator::next` \
              polls the future synchronously — yielding through `Poll::Pending` \
              would panic at runtime; resolve the awaited value before yielding instead)",
            span,
        );
        stats.generators_rejected += 1;
        return;
    }

    if let Some(span) = analysis
        .first_try_body_yield_span
        .or(analysis.first_catch_yield_span)
    {
        ctx.error(
            GENERATOR_DIAG_UNSUPPORTED_YIELD,
            "yield inside a try body or catch clause is not supported \
             (the catch_unwind closure is non-async, but `co.yield_(...).await` \
              requires async context; move the yield outside the try/catch)",
            span,
        );
        stats.generators_rejected += 1;
        return;
    }

    if f.body.last().is_none_or(|last| !last.is_terminal()) {
        f.body.push(HirStmt::Return { value: None });
    }

    let generator_ty = types.intern(&Type::Generator { inner: inner_ty });
    f.ret = generator_ty;
    generator_fn_names.insert(qualified_name(namespace_path, f.name.as_str()));
    stats.generators_transformed += 1;
}

fn reject_generator_methods(
    program: &HirProgram,
    ctx: &mut PassContext,
    stats: &mut LowerGeneratorsStats,
) {
    fn reject_decl(decl: &HirDecl, ctx: &mut PassContext, stats: &mut LowerGeneratorsStats) {
        match decl {
            HirDecl::Class(c) => {
                for m in &c.methods {
                    if m.is_generator {
                        let span = first_body_span(&m.body).unwrap_or_else(|| Span::new(0, 0));
                        ctx.error(
                            GENERATOR_DIAG_UNSUPPORTED_YIELD,
                            format!(
                                "generator method `{}()` is not supported yet \
                                 (move the generator out of the class)",
                                m.name.as_str()
                            ),
                            span,
                        );
                        stats.generators_rejected += 1;
                    }
                }
            }
            HirDecl::Namespace { members, .. } => {
                for m in members {
                    reject_decl(m, ctx, stats);
                }
            }
            HirDecl::Function(_)
            | HirDecl::TypeAlias { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Global { .. }
            | HirDecl::Interface { .. } => {}
        }
    }
    for decl in &program.declarations {
        reject_decl(decl, ctx, stats);
    }
}

fn reject_nested_generator_decls(
    decls: &[HirDecl],
    ctx: &mut PassContext,
    stats: &mut LowerGeneratorsStats,
) {
    use ts_aot_ir_hir::{HirExpr, Visitor, walk_expr, walk_stmt};
    struct NestedGeneratorRejector<'a> {
        ctx: &'a mut PassContext,
        stats: &'a mut LowerGeneratorsStats,
    }
    impl Visitor for NestedGeneratorRejector<'_> {
        fn visit_stmt(&mut self, stmt: &HirStmt) {
            if let HirStmt::Decl(HirDecl::Function(f)) = stmt {
                if f.is_generator {
                    let span = first_body_span(&f.body).unwrap_or_default();
                    report_nested_generator(self.ctx, self.stats, f.name.as_str(), span);
                }
                self.visit_block(&f.body);
                return;
            }
            if let HirStmt::Decl(HirDecl::Namespace { members, .. }) = stmt {
                reject_nested_decl_list(members, self);
                return;
            }
            walk_stmt(self, stmt);
        }
        fn visit_expr(&mut self, expr: &HirExpr) {
            if let HirExpr::Closure { captures, body, .. } = expr {
                for c in captures {
                    self.visit_expr(c);
                }
                self.visit_block(body);
                return;
            }
            walk_expr(self, expr);
        }
    }

    fn report_nested_generator(
        ctx: &mut PassContext,
        stats: &mut LowerGeneratorsStats,
        name: &str,
        span: ts_aot_core::Span,
    ) {
        ctx.error(
            GENERATOR_DIAG_UNSUPPORTED_YIELD,
            format!(
                "nested generator `{}` is not supported yet \
                 (hoist it to module scope)",
                name
            ),
            span,
        );
        stats.generators_rejected += 1;
    }

    fn reject_nested_decl_list(members: &[HirDecl], rejector: &mut NestedGeneratorRejector<'_>) {
        for m in members {
            match m {
                HirDecl::Function(f) => {
                    if f.is_generator {
                        let span = first_body_span(&f.body).unwrap_or_default();
                        report_nested_generator(
                            rejector.ctx,
                            rejector.stats,
                            f.name.as_str(),
                            span,
                        );
                    }
                    rejector.visit_block(&f.body);
                }
                HirDecl::Class(c) => {
                    for cm in &c.methods {
                        if cm.is_generator {
                            let span = first_body_span(&cm.body).unwrap_or_default();
                            report_nested_generator(
                                rejector.ctx,
                                rejector.stats,
                                cm.name.as_str(),
                                span,
                            );
                        }
                        rejector.visit_block(&cm.body);
                    }
                }
                HirDecl::Namespace { members: inner, .. } => {
                    reject_nested_decl_list(inner, rejector);
                }
                _ => {}
            }
        }
    }
    let mut rejector = NestedGeneratorRejector { ctx, stats };
    for d in decls {
        match d {
            HirDecl::Function(f) => rejector.visit_block(&f.body),
            HirDecl::Class(c) => {
                for m in &c.methods {
                    rejector.visit_block(&m.body);
                }
            }
            HirDecl::Namespace { members, .. } => {
                reject_nested_generator_decls(members, rejector.ctx, rejector.stats);
            }
            HirDecl::Global {
                init: Some(expr), ..
            } => rejector.visit_expr(expr),
            HirDecl::TypeAlias { .. }
            | HirDecl::Enum { .. }
            | HirDecl::Global { init: None, .. }
            | HirDecl::Interface { .. } => {}
        }
    }
}
