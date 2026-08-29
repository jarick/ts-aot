use ts_aot_core::{Diagnostic, ModuleId, Severity, TypeTable};
use ts_aot_frontend::FrontendPass;
use ts_aot_ir_mir::{MirBlock, MirDecl, MirFunctionDecl, MirProgram, MirStmt, RuntimeOp};
use ts_aot_passes::{
    PassContext, convert_program, lower_async, lower_classes, lower_closures, lower_enums,
    lower_generators, monomorphize,
};

pub fn convert(src: &str) -> (MirProgram, TypeTable, Vec<Diagnostic>, String) {
    let mut types = TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", src, &mut types, false);
    let mut diags: Vec<Diagnostic> = frontend.diagnostics.iter().cloned().collect();
    if frontend.diagnostics.has_errors() {
        return (
            MirProgram::new(ModuleId::from_raw(0)),
            types,
            diags,
            String::new(),
        );
    }
    let mut hir = frontend.program;
    lower_enums(&mut hir, &mut types, &mut ctx);
    lower_classes(&mut hir, &mut types, &mut ctx);
    monomorphize(&mut hir, &mut types, &mut ctx);
    lower_closures(&mut hir, &mut types, &mut ctx);
    let _ = lower_async(&mut hir, &mut types, &mut ctx);
    let _ = lower_generators(&mut hir, &mut types, &mut ctx);
    let hir_dump = hir.dump_text();
    let mir = convert_program(&hir, &mut types, &mut ctx);
    diags.extend(ctx.diagnostics().iter().cloned());
    (mir, types, diags, hir_dump)
}

pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

pub fn find_mir_function<'a>(mir: &'a MirProgram, name: &str) -> Option<&'a MirFunctionDecl> {
    mir.declarations.iter().find_map(|d| match d {
        MirDecl::Function(f) if f.name.as_str() == name => Some(f),
        _ => None,
    })
}

pub fn count_runtime_ops(mir: &MirProgram, op: RuntimeOp) -> usize {
    fn walk_block(block: &MirBlock, op: RuntimeOp, count: &mut usize) {
        for s in &block.stmts {
            if let MirStmt::Runtime { op: o, .. } = s
                && *o == op
            {
                *count += 1;
            }
            match s {
                MirStmt::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    walk_block(then_block, op, count);
                    if let Some(eb) = else_block {
                        walk_block(eb, op, count);
                    }
                }
                MirStmt::While { body, .. }
                | MirStmt::DoWhile { body, .. }
                | MirStmt::ForOf { body, .. }
                | MirStmt::ForAwaitOf { body, .. }
                | MirStmt::ForIn { body, .. } => walk_block(body, op, count),
                MirStmt::Switch { cases, default, .. } => {
                    for case in cases {
                        walk_block(&case.body, op, count);
                    }
                    if let Some(def) = default {
                        walk_block(def, op, count);
                    }
                }
                MirStmt::Try {
                    body,
                    catch,
                    finally,
                    ..
                } => {
                    walk_block(body, op, count);
                    if let Some(catch_block) = catch {
                        walk_block(catch_block, op, count);
                    }
                    if let Some(fin) = finally {
                        walk_block(fin, op, count);
                    }
                }
                MirStmt::Let { .. }
                | MirStmt::Assign { .. }
                | MirStmt::Expr(_)
                | MirStmt::Return(_)
                | MirStmt::ReturnResultErr { .. }
                | MirStmt::Throw { .. }
                | MirStmt::Runtime { .. }
                | MirStmt::Break
                | MirStmt::Continue => {}
            }
        }
    }
    let mut count = 0;
    for d in &mir.declarations {
        match d {
            MirDecl::Function(f) => walk_block(&f.body.block, op, &mut count),
            MirDecl::Struct(s) => {
                for m in &s.methods {
                    walk_block(&m.body.block, op, &mut count);
                }
            }
            MirDecl::Global(_) => {}
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::{Atom, FunctionId, LocalId, StructId, TypeId};
    use ts_aot_ir_mir::{
        FunctionKind, MirBlock, MirBody, MirDecl, MirExpr, MirFunctionDecl, MirProgram, MirStmt,
        MirStructDecl,
    };

    #[test]
    fn count_runtime_ops_counts_dest_none_runtime_ops() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        let f = MirFunctionDecl {
            id: FunctionId::from_raw(0),
            name: Atom::from("noop"),
            export_name: None,
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: MirBody {
                locals: Vec::new(),
                block: MirBlock {
                    stmts: vec![
                        MirStmt::Runtime {
                            op: RuntimeOp::HostConsoleLog,
                            args: vec![MirExpr::Unit],
                            dest: None,
                            ty: TypeId::from_raw(0),
                            target_ty: None,
                        },
                        MirStmt::Runtime {
                            op: RuntimeOp::HostConsoleLog,
                            args: vec![MirExpr::Unit],
                            dest: Some(LocalId::from_raw(1)),
                            ty: TypeId::from_raw(0),
                            target_ty: None,
                        },
                    ],
                },
            },
            kind: FunctionKind::Plain,
            effects: Default::default(),
        };
        prog.push_decl(MirDecl::Function(f));
        let count = count_runtime_ops(&prog, RuntimeOp::HostConsoleLog);
        assert_eq!(
            count, 2,
            "count_runtime_ops must count BOTH dest:None and dest:Some matching runtime ops \
             (the dest filter was removed so the helper can locate side-effect-only ops like \
             HostConsoleLog), got count={count}"
        );
    }

    #[test]
    fn count_runtime_ops_traverses_struct_methods() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        let method = MirFunctionDecl {
            id: FunctionId::from_raw(1),
            name: Atom::from("iter"),
            export_name: None,
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: MirBody {
                locals: Vec::new(),
                block: MirBlock {
                    stmts: vec![MirStmt::Runtime {
                        op: RuntimeOp::HostConsoleLog,
                        args: vec![MirExpr::Unit],
                        dest: None,
                        ty: TypeId::from_raw(0),
                        target_ty: None,
                    }],
                },
            },
            kind: FunctionKind::GeneratorMethod {
                owner: StructId::from_raw(0),
                self_param: LocalId::from_raw(0),
            },
            effects: Default::default(),
        };
        let s = MirStructDecl {
            id: StructId::from_raw(0),
            name: Atom::from("Box"),
            fields: Vec::new(),
            methods: vec![method],
        };
        prog.push_decl(MirDecl::Struct(s));
        let count = count_runtime_ops(&prog, RuntimeOp::HostConsoleLog);
        assert_eq!(
            count, 1,
            "count_runtime_ops must walk MirDecl::Struct methods (covers Method and \
             GeneratorMethod bodies) so generator methods containing runtime ops are not \
             silently skipped, got count={count}"
        );
    }
}
