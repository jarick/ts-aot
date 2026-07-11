use std::collections::HashSet;

use ts_aot_backend::compile_to_string_with_types;
use ts_aot_core::{Diagnostic, Span, TypeTable};
use ts_aot_frontend::FrontendPass;
use ts_aot_passes::{
    PassContext, convert_program, lower_async, lower_classes, lower_closures, lower_enums,
    lower_result, monomorphize,
};

use crate::{CompileOptions, DriverOutput, EmitStage};

pub(crate) fn run(name: &str, source: &str, opts: &CompileOptions) -> DriverOutput {
    let mut out = DriverOutput::default();
    let mut types = TypeTable::new();
    let mut ctx = PassContext::new();

    let frontend = FrontendPass::new().run_with_types(name, source, &mut types);
    out.diagnostics.extend(frontend.diagnostics.iter().cloned());
    if frontend.diagnostics.has_errors() {
        return out;
    }
    let mut hir = frontend.program;

    lower_enums(&mut hir, &mut types, &mut ctx);
    lower_classes(&mut hir, &mut types, &mut ctx);
    monomorphize(&mut hir, &mut types, &mut ctx);
    let closures = lower_closures(&mut hir, &mut ctx);
    let _ = lower_async(&mut hir, &mut types, &mut ctx);
    out.diagnostics
        .extend(ctx.take_diagnostics().iter().cloned());
    if out.diagnostics.has_errors() {
        return out;
    }

    if matches!(opts.emit, EmitStage::Hir) {
        out.hir_text = Some(hir.dump_text());
        return out;
    }

    let closure_set: HashSet<_> = closures.closure_names.into_iter().collect();
    let mut mir = convert_program(&hir, &mut ctx, &closure_set);
    out.diagnostics
        .extend(ctx.take_diagnostics().iter().cloned());
    if out.diagnostics.has_errors() {
        return out;
    }

    lower_result(&mut mir, &mut types);

    match opts.emit {
        EmitStage::Hir => {
            out.hir_text = Some(hir.dump_text());
        }
        EmitStage::Mir => {
            out.mir_text = Some(mir.dump_text());
        }
        EmitStage::Rust => match compile_to_string_with_types(&mir, &types) {
            Ok(s) => out.rust_source = Some(s),
            Err(e) => {
                out.diagnostics
                    .push(Diagnostic::error("E0300", e.to_string(), Span::new(0, 0)));
            }
        },
    }

    out
}
