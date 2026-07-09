use proc_macro2::TokenStream;
use ts_aot_core::{ModuleId, TypeId, TypeTable};
use ts_aot_ir_mir::MirProgram;

use crate::render::{RenderConfig, render_tokens};
use crate::{compile_program, compile_to_string, compile_with_types};

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
fn compile_non_empty_program_emits_decl_token() {
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
    assert!(
        result.is_ok(),
        "non-empty MIR with empty body must compile to tokens, got: {:?}",
        result.err()
    );
    let s = result.unwrap().to_string();
    assert!(
        s.contains("fn greet"),
        "expected `fn greet` in output, got: {s}"
    );
}

#[test]
fn compile_with_types_propagates_type_table() {
    use ts_aot_core::{Atom, Type};
    use ts_aot_ir_mir::MirDecl;
    use ts_aot_ir_mir::MirFunctionDecl;
    use ts_aot_ir_mir::MirParam;

    let mut program = MirProgram::new(ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(MirFunctionDecl {
        id: ts_aot_core::FunctionId::from_raw(0),
        name: Atom::from("answer"),
        export_name: None,
        params: Vec::<MirParam>::new(),
        ret: TypeId::from_raw(0),
        throws: None,
        body: ts_aot_ir_mir::MirBody::default(),
        kind: ts_aot_ir_mir::FunctionKind::Plain,
        effects: ts_aot_ir_mir::FunctionEffects::default(),
    }));

    let mut types = TypeTable::new();
    let i32_id = types.intern(&Type::I32);
    let mut f = MirFunctionDecl {
        id: ts_aot_core::FunctionId::from_raw(0),
        name: Atom::from("answer2"),
        export_name: None,
        params: Vec::<MirParam>::new(),
        ret: i32_id,
        throws: None,
        body: ts_aot_ir_mir::MirBody::default(),
        kind: ts_aot_ir_mir::FunctionKind::Plain,
        effects: ts_aot_ir_mir::FunctionEffects::default(),
    };
    f.ret = i32_id;
    program.push_decl(MirDecl::Function(f));

    let tokens = compile_with_types(&program, &types, &RenderConfig::default())
        .expect("compile_with_types should emit tokens");
    let s = tokens.to_string();
    assert!(
        s.contains("fn answer"),
        "expected fn answer in output, got: {s}"
    );
    assert!(
        s.contains("fn answer2"),
        "expected fn answer2 in output, got: {s}"
    );
    assert!(s.contains("-> i32"), "expected ret type i32, got: {s}");
}

#[test]
fn compile_to_string_includes_decl_tokens() {
    use ts_aot_core::Atom;
    use ts_aot_ir_mir::MirDecl;
    use ts_aot_ir_mir::MirFunctionDecl;
    use ts_aot_ir_mir::MirParam;

    let mut program = MirProgram::new(ModuleId::from_raw(0));
    program.push_decl(MirDecl::Function(MirFunctionDecl {
        id: ts_aot_core::FunctionId::from_raw(0),
        name: Atom::from("visible_fn"),
        export_name: None,
        params: Vec::<MirParam>::new(),
        ret: TypeId::from_raw(0),
        throws: None,
        body: ts_aot_ir_mir::MirBody::default(),
        kind: ts_aot_ir_mir::FunctionKind::Plain,
        effects: ts_aot_ir_mir::FunctionEffects::default(),
    }));

    let s = compile_to_string(&program).expect("compile_to_string should succeed");
    assert!(
        s.contains("fn visible_fn"),
        "expected fn in output, got: {s}"
    );
}
