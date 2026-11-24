use super::super::emit_decls;
use super::empty_func;
use ts_aot_core::{Atom, FieldId, LocalId, ModuleId, StructId, Type, TypeTable, Visibility};
use ts_aot_ir_mir::{
    MirBlock, MirBody, MirDecl, MirExpr, MirFieldDecl, MirLocalDecl, MirParam, MirProgram, MirStmt,
    MirStructDecl, RuntimeOp,
};
use ts_aot_test_support::check_generated_crate;

#[test]
fn generated_arena_crate_passes_cargo_check() {
    let mut types = TypeTable::new();
    let string_ty = types.intern(&Type::String);
    let point_id = StructId::from_raw(0);
    let point_ty = types.intern(&Type::Struct { id: point_id });
    let str_arr_ty = types.intern(&Type::Array { element: string_ty });

    let mut prog = MirProgram::new(ModuleId::from_raw(0));
    prog.push_decl(MirDecl::Struct(MirStructDecl {
        id: point_id,
        name: Atom::from("Point"),
        fields: vec![
            MirFieldDecl {
                id: FieldId::from_raw(0),
                name: Atom::from("name"),
                ty: string_ty,
                mutable: false,
                visibility: Visibility::Public,
            },
            MirFieldDecl {
                id: FieldId::from_raw(1),
                name: Atom::from("tag"),
                ty: string_ty,
                mutable: false,
                visibility: Visibility::Public,
            },
        ],
        methods: Vec::new(),
    }));

    let xs = LocalId::from_raw(0);
    let mut copy = empty_func("copy_strings");
    copy.export_name = Some("copy_strings".to_owned());
    copy.ret = str_arr_ty;
    copy.params = vec![MirParam {
        id: xs,
        name: Atom::from("xs"),
        ty: str_arr_ty,
    }];
    copy.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(xs)))],
        },
    };
    prog.push_decl(MirDecl::Function(copy));

    let p = LocalId::from_raw(0);
    let keys = LocalId::from_raw(1);
    let mut keys_of = empty_func("keys_of");
    keys_of.export_name = Some("keys_of".to_owned());
    keys_of.ret = str_arr_ty;
    keys_of.params = vec![MirParam {
        id: p,
        name: Atom::from("p"),
        ty: point_ty,
    }];
    keys_of.body = MirBody {
        locals: vec![MirLocalDecl {
            id: keys,
            name: Atom::from("keys"),
            ty: str_arr_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::ObjectKeys,
                    args: vec![MirExpr::Local(p)],
                    dest: Some(keys),
                    ty: str_arr_ty,
                    target_ty: None,
                },
                MirStmt::Return(Some(MirExpr::Local(keys))),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(keys_of));

    let mut tagged = empty_func("tagged");
    tagged.export_name = Some("tagged".to_owned());
    tagged.ret = str_arr_ty;
    tagged.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::TemplateStringsArray {
                cooked: vec![Atom::from("hi "), Atom::from("!")],
                ty: str_arr_ty,
            }))],
        },
    };
    prog.push_decl(MirDecl::Function(tagged));

    let json_arg = LocalId::from_raw(0);
    let json_out = LocalId::from_raw(1);
    let mut parse_strings = empty_func("parse_strings");
    parse_strings.export_name = Some("parse_strings".to_owned());
    parse_strings.ret = str_arr_ty;
    parse_strings.params = vec![MirParam {
        id: json_arg,
        name: Atom::from("j"),
        ty: string_ty,
    }];
    parse_strings.body = MirBody {
        locals: vec![MirLocalDecl {
            id: json_out,
            name: Atom::from("out"),
            ty: str_arr_ty,
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Runtime {
                    op: RuntimeOp::JsonParse,
                    args: vec![MirExpr::Local(json_arg)],
                    dest: Some(json_out),
                    ty: str_arr_ty,
                    target_ty: Some(str_arr_ty),
                },
                MirStmt::Return(Some(MirExpr::Local(json_out))),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(parse_strings));

    let caught = LocalId::from_raw(0);
    let mut caught_to_string = empty_func("caught_to_string");
    caught_to_string.export_name = Some("caught_to_string".to_owned());
    caught_to_string.ret = string_ty;
    caught_to_string.body = MirBody {
        locals: vec![MirLocalDecl {
            id: caught,
            name: Atom::from("e"),
            ty: types.intern(&Type::Error),
            mutable: false,
            narrowed: false,
        }],
        block: MirBlock {
            stmts: vec![
                MirStmt::Try {
                    body: MirBlock {
                        stmts: vec![MirStmt::Throw {
                            error: MirExpr::String {
                                id: Atom::from("boom"),
                                ty: string_ty,
                            },
                            error_ty: string_ty,
                        }],
                    },
                    catch_param: Some(caught),
                    catch: Some(MirBlock {
                        stmts: vec![MirStmt::Return(Some(MirExpr::Local(caught)))],
                    }),
                    finally: None,
                },
                MirStmt::Return(Some(MirExpr::String {
                    id: Atom::from(""),
                    ty: string_ty,
                })),
            ],
        },
    };
    prog.push_decl(MirDecl::Function(caught_to_string));

    let rust = emit_decls(&prog, &types)
        .expect("the arena program must emit")
        .to_string();
    let compact: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("ts_aot_runtime::bumpalo::collections::Vec"),
        "array types must be qualified through the runtime crate so consumers need no direct \
         bumpalo dependency; got:\n{rust}"
    );
    assert!(
        compact.contains("to_js_string(__ctx)"),
        "an untyped catch binding coerced to a string must call `to_js_string(__ctx)` so the \
         generated crate compiles against the arena-aware runtime; got:\n{rust}"
    );
    assert!(
        compact.contains("ts_aot_runtime::bumpalo::vec!"),
        "vec literals must be qualified through the runtime crate so consumers need no direct \
         bumpalo dependency; got:\n{rust}"
    );
    assert!(
        !compact
            .replace("ts_aot_runtime::bumpalo", "")
            .contains("bumpalo"),
        "no emitted path may reach bumpalo except through ts_aot_runtime; got:\n{rust}"
    );
    check_generated_crate("generated_check_full", &rust);

    let mut min_types = TypeTable::new();
    let min_string = min_types.intern(&Type::String);
    let min_arr = min_types.intern(&Type::Array {
        element: min_string,
    });
    let arg = LocalId::from_raw(0);
    let mut min_copy = empty_func("copy_strings");
    min_copy.export_name = Some("copy_strings".to_owned());
    min_copy.ret = min_arr;
    min_copy.params = vec![MirParam {
        id: arg,
        name: Atom::from("xs"),
        ty: min_arr,
    }];
    min_copy.body = MirBody {
        locals: Vec::new(),
        block: MirBlock {
            stmts: vec![MirStmt::Return(Some(MirExpr::Local(arg)))],
        },
    };
    let mut min_prog = MirProgram::new(ModuleId::from_raw(0));
    min_prog.push_decl(MirDecl::Function(min_copy));
    let min_rust = emit_decls(&min_prog, &min_types)
        .expect("the minimal array program must emit")
        .to_string();
    assert!(
        !min_rust.contains("use ts_aot_runtime"),
        "the minimal program must not import the runtime glob, otherwise a bare `bumpalo` path \
         would resolve through the runtime re-export and the `cargo check` below would not \
         discriminate; got:\n{min_rust}"
    );
    check_generated_crate("generated_check_min", &min_rust);
}
