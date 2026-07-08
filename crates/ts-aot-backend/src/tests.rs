use proc_macro2::TokenStream;
use ts_aot_core::ModuleId;
use ts_aot_ir_mir::MirProgram;

use crate::error::BackendError;
use crate::render::{RenderConfig, render_tokens};
use crate::{compile_program, compile_to_string};

#[test]
fn compile_empty_program_emits_empty_token_stream() {
    let program = MirProgram::new(ModuleId::from_raw(0));
    let tokens: TokenStream = compile_program(&program).expect("empty MIR should compile to Ok");
    assert!(tokens.is_empty());
}

#[test]
fn compile_to_string_for_empty_program_yields_empty_string() {
    let program = MirProgram::new(ModuleId::from_raw(0));
    let s = compile_to_string(&program).expect("empty MIR should compile to Ok");
    assert!(s.is_empty());
}

#[test]
fn render_default_config_round_trips() {
    let cfg = RenderConfig::default();
    assert_eq!(cfg.module_name, "ts_aot_module");
    assert_eq!(cfg.indent, 4);
}

#[test]
fn render_tokens_uses_token_stream_to_string() {
    let tokens = quote::quote! {
        fn answer() -> i32 { 42 }
    };
    let cfg = RenderConfig::default();
    let rendered = render_tokens(&tokens, &cfg);
    assert!(
        rendered.contains("fn answer"),
        "render_tokens must surface the input tokens, got: {rendered:?}"
    );
    assert!(rendered.contains("42"));
}

#[test]
fn compile_non_empty_program_returns_not_implemented() {
    use ts_aot_core::{Atom, TypeId};
    use ts_aot_ir_mir::{FunctionKind, MirDecl, MirFunctionDecl, MirParam};

    let mut program = MirProgram::new(ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(MirFunctionDecl {
        id: ts_aot_core::FunctionId::from_raw(0),
        name: Atom::from("greet"),
        export_name: None,
        params: Vec::<MirParam>::new(),
        ret: TypeId::from_raw(0),
        throws: None,
        body: ts_aot_ir_mir::MirBody::default(),
        kind: FunctionKind::Plain,
        effects: ts_aot_ir_mir::FunctionEffects::default(),
    }));

    let result = compile_program(&program);
    assert_eq!(
        result.err(),
        Some(BackendError::NotImplemented),
        "non-empty MIR must surface NotImplemented, not silently emit empty tokens"
    );
}
