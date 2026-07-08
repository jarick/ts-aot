use ts_aot_core::Atom;

use super::Dumper;
use super::dump_sym;
use super::expr;

use crate::decl::{HirClass, HirDecl, HirEnumVariant, HirField, HirFunction, HirParam};
use crate::expr::HirExpr;

pub(crate) fn dump_decl(decl: &HirDecl, d: &mut Dumper) {
    match decl {
        HirDecl::Function(f) => dump_function(f, d),
        HirDecl::Class(c) => dump_class(c, d),
        HirDecl::TypeAlias { name, target } => d.line(&format!(
            "typealias {} = {}",
            dump_sym(name, d),
            target.raw()
        )),
        HirDecl::Enum { name, variants } => dump_enum(name, variants, d),
        HirDecl::Global { name, ty, init } => dump_global(name, *ty, init.as_ref(), d),
        HirDecl::Interface { name } => d.line(&format!("interface {}", dump_sym(name, d))),
        HirDecl::Namespace { name, members } => dump_namespace(name, members, d),
    }
}

fn dump_function(f: &HirFunction, d: &mut Dumper) {
    let prefix = if f.is_async { "async fn" } else { "fn" };
    dump_function_inner(f, prefix, d);
}

fn dump_method(m: &HirFunction, d: &mut Dumper) {
    let prefix = if m.is_async { "async method" } else { "method" };
    dump_function_inner(m, prefix, d);
}

fn dump_function_inner(f: &HirFunction, prefix: &str, d: &mut Dumper) {
    d.line(&format!(
        "{} {}({}) -> {}",
        prefix,
        dump_sym(&f.name, d),
        f.params
            .iter()
            .map(dump_param)
            .collect::<Vec<_>>()
            .join(", "),
        f.ret.raw(),
    ));
    if let Some(throws) = f.throws {
        d.line(&format!("throws {}", throws.raw()));
    }
    if f.is_exported {
        d.line("exported");
    }
    if f.is_generator {
        d.line("generator");
    }
    if !f.type_params.is_empty() {
        d.line(&format!("type_params: {:?}", f.type_params));
    }
    if f.body.is_empty() {
        return;
    }
    d.line("body: {");
    d.push();
    expr::dump_body(&f.body, d);
    d.pop();
    d.line("}");
}

fn dump_param(p: &HirParam) -> String {
    format!("{}: {}", p.name.as_str(), p.ty.raw())
}

fn dump_field(field: &HirField) -> String {
    format!("{}: {}", field.name.as_str(), field.ty.raw())
}

fn dump_class(c: &HirClass, d: &mut Dumper) {
    let mut header = format!("class {}", dump_sym(&c.name, d));
    if let Some(extends) = &c.extends {
        header.push_str(&format!(" extends {}", extends.as_str()));
    }
    if !c.type_params.is_empty() {
        header.push_str(&format!("<{:?}>", c.type_params));
    }
    d.line(&header);
    d.line("body: {");
    d.push();
    for field in &c.fields {
        d.line(&format!("field {}", dump_field(field)));
    }
    for method in &c.methods {
        dump_method(method, d);
    }
    d.pop();
    d.line("}");
}

fn dump_enum(name: &Atom, variants: &[HirEnumVariant], d: &mut Dumper) {
    d.line(&format!("enum {} {{", dump_sym(name, d)));
    d.push();
    for v in variants {
        match &v.value {
            Some(val) => {
                let mut tmp = Dumper::new();
                expr::dump_expr_inline(val, &mut tmp);
                d.line(&format!("{} = {}", v.name.as_str(), tmp.buf));
            }
            None => d.line(v.name.as_str()),
        }
    }
    if variants.is_empty() {
        d.line("");
    }
    d.pop();
    d.line("}");
}

fn dump_global(name: &Atom, ty: ts_aot_core::TypeId, init: Option<&HirExpr>, d: &mut Dumper) {
    d.line(&format!("global {}: {}", dump_sym(name, d), ty.raw()));
    if let Some(init) = init {
        let mut tmp = Dumper::new();
        expr::dump_expr_inline(init, &mut tmp);
        d.line(&format!("  = {}", tmp.buf));
    }
}

fn dump_namespace(name: &Atom, members: &[HirDecl], d: &mut Dumper) {
    d.line(&format!("namespace {} {{", dump_sym(name, d)));
    d.push();
    for m in members {
        dump_decl(m, d);
    }
    d.pop();
    d.line("}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::HirDecl;
    use crate::program::HirProgram;
    use ts_aot_core::{Atom, ModuleId, TypeId};

    fn empty_func(name: &str) -> HirFunction {
        HirFunction {
            name: Atom::new_inline(name),
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }
    }

    #[test]
    fn dump_function_with_async_flag() {
        let mut f = empty_func("a");
        f.is_async = true;
        let mut d = Dumper::new();
        dump_function(&f, &mut d);
        assert!(d.buf.contains("async fn a"));
    }

    #[test]
    fn dump_function_with_params() {
        let mut f = empty_func("sum");
        f.params = vec![HirParam {
            name: Atom::new_inline("a"),
            ty: TypeId::from_raw(1),
        }];
        let mut d = Dumper::new();
        dump_function(&f, &mut d);
        assert!(d.buf.contains("a: 1"));
    }

    #[test]
    fn dump_function_with_throws_and_exported_and_generator() {
        let mut f = empty_func("g");
        f.throws = Some(TypeId::from_raw(2));
        f.is_exported = true;
        f.is_generator = true;
        let mut d = Dumper::new();
        dump_function(&f, &mut d);
        assert!(d.buf.contains("throws 2"));
        assert!(d.buf.contains("exported"));
        assert!(d.buf.contains("generator"));
    }

    #[test]
    fn dump_function_with_body_dumps_body_block() {
        use crate::expr::HirExpr;
        use crate::stmt::HirStmt;
        let mut f = empty_func("withbody");
        f.body = vec![HirStmt::ret(Some(HirExpr::Int(7)))];
        let mut d = Dumper::new();
        dump_function(&f, &mut d);
        assert!(d.buf.contains("body: {"));
        assert!(d.buf.contains("return 7"));
    }

    #[test]
    fn dump_class_with_extends_and_methods() {
        let mut method = empty_func("m");
        method.params = vec![HirParam {
            name: Atom::new_inline("p"),
            ty: TypeId::from_raw(9),
        }];
        let class = HirClass {
            name: Atom::new_inline("K"),
            ty: TypeId::from_raw(0),
            fields: vec![HirField {
                name: Atom::new_inline("n"),
                ty: TypeId::from_raw(3),
            }],
            methods: vec![method],
            extends: Some(Atom::new_inline("Base")),
            type_params: Vec::new(),
        };
        let mut d = Dumper::new();
        dump_class(&class, &mut d);
        assert!(d.buf.contains("class K extends Base"));
        assert!(d.buf.contains("field n: 3"));
        assert!(d.buf.contains("method m"));
        assert!(d.buf.contains("p: 9"));
    }

    #[test]
    fn dump_method_emits_async_throws_body_braces_and_return_type() {
        use crate::expr::HirExpr;
        use crate::stmt::HirStmt;
        let mut m = empty_func("doThing");
        m.is_async = true;
        m.throws = Some(TypeId::from_raw(8));
        m.ret = TypeId::from_raw(3);
        m.body = vec![HirStmt::ret(Some(HirExpr::Int(0)))];
        let class = HirClass {
            name: Atom::new_inline("K"),
            ty: TypeId::from_raw(0),
            fields: Vec::new(),
            methods: vec![m],
            extends: None,
            type_params: Vec::new(),
        };
        let mut d = Dumper::new();
        dump_class(&class, &mut d);
        assert!(d.buf.contains("async method doThing"));
        assert!(d.buf.contains("-> 3"));
        assert!(d.buf.contains("throws 8"));
        assert!(d.buf.contains("body: {"));
        assert!(d.buf.contains("return 0"));
    }

    #[test]
    fn dump_method_empty_body_emits_only_class_body_block() {
        let m = empty_func("empty");
        let class = HirClass {
            name: Atom::new_inline("K"),
            ty: TypeId::from_raw(0),
            fields: Vec::new(),
            methods: vec![m],
            extends: None,
            type_params: Vec::new(),
        };
        let mut d = Dumper::new();
        dump_class(&class, &mut d);
        assert!(d.buf.contains("method empty"));
        assert_eq!(d.buf.matches("body: {").count(), 1);
    }

    #[test]
    fn dump_enum_with_and_without_values() {
        use crate::expr::HirExpr;
        let variants = vec![
            HirEnumVariant {
                name: Atom::new_inline("A"),
                value: None,
            },
            HirEnumVariant {
                name: Atom::new_inline("B"),
                value: Some(HirExpr::Int(11)),
            },
        ];
        let mut d = Dumper::new();
        dump_enum(&Atom::new_inline("E"), &variants, &mut d);
        assert!(d.buf.contains("enum E {"));
        assert!(d.buf.contains("A"));
        assert!(d.buf.contains("B = 11"));
    }

    #[test]
    fn dump_global_with_init() {
        use crate::expr::HirExpr;
        let mut d = Dumper::new();
        dump_global(
            &Atom::new_inline("g"),
            TypeId::from_raw(4),
            Some(&HirExpr::Int(99)),
            &mut d,
        );
        assert!(d.buf.contains("global g: 4"));
        assert!(d.buf.contains("= 99"));
    }

    #[test]
    fn dump_global_without_init_omits_eq() {
        let mut d = Dumper::new();
        dump_global(&Atom::new_inline("g"), TypeId::from_raw(4), None, &mut d);
        assert!(d.buf.contains("global g: 4"));
        assert!(!d.buf.contains("= "));
    }

    #[test]
    fn dump_namespace_with_member_decl() {
        let mut d = Dumper::new();
        dump_namespace(
            &Atom::new_inline("NS"),
            &[HirDecl::Interface {
                name: Atom::new_inline("I"),
            }],
            &mut d,
        );
        assert!(d.buf.contains("namespace NS"));
        assert!(d.buf.contains("interface I"));
    }

    #[test]
    fn dump_function_with_type_params() {
        let mut f = empty_func("id");
        f.type_params.push(ts_aot_core::GenericParamId::from_raw(0));
        let mut d = Dumper::new();
        dump_function(&f, &mut d);
        assert!(d.buf.contains("type_params:"));
    }

    #[test]
    fn dump_type_alias_emits_target_raw() {
        let mut d = Dumper::new();
        dump_decl(
            &HirDecl::TypeAlias {
                name: Atom::new_inline("MyT"),
                target: TypeId::from_raw(7),
            },
            &mut d,
        );
        assert!(d.buf.contains("typealias MyT = 7"));
    }

    #[test]
    fn dump_interface_emits_name() {
        let mut d = Dumper::new();
        dump_decl(
            &HirDecl::Interface {
                name: Atom::new_inline("IFace"),
            },
            &mut d,
        );
        assert!(d.buf.contains("interface IFace"));
    }

    #[test]
    fn dump_global_decl_routes_through_dump_decl() {
        let mut d = Dumper::new();
        dump_decl(
            &HirDecl::Global {
                name: Atom::new_inline("gx"),
                ty: TypeId::from_raw(0),
                init: None,
            },
            &mut d,
        );
        assert!(d.buf.contains("global gx"));
    }

    #[test]
    fn dump_enum_decl_routes_through_dump_decl() {
        let mut d = Dumper::new();
        dump_decl(
            &HirDecl::Enum {
                name: Atom::new_inline("E"),
                variants: vec![],
            },
            &mut d,
        );
        assert!(d.buf.contains("enum E {"));
    }

    #[test]
    fn dump_function_decl_routes_through_dump_decl() {
        let mut d = Dumper::new();
        dump_decl(&HirDecl::Function(empty_func("f")), &mut d);
        assert!(d.buf.contains("fn f"));
    }

    #[test]
    fn dump_class_decl_routes_through_dump_decl() {
        let mut d = Dumper::new();
        dump_decl(
            &HirDecl::Class(HirClass {
                name: Atom::new_inline("C"),
                ty: TypeId::from_raw(0),
                fields: vec![],
                methods: vec![],
                extends: None,
                type_params: vec![],
            }),
            &mut d,
        );
        assert!(d.buf.contains("class C"));
    }

    #[test]
    fn function_param_to_string_contains_name_and_type() {
        let p = HirParam {
            name: Atom::new_inline("a"),
            ty: TypeId::from_raw(2),
        };
        assert_eq!(dump_param(&p), "a: 2");
    }

    #[test]
    fn field_to_string_contains_name_and_type() {
        let f = HirField {
            name: Atom::new_inline("f"),
            ty: TypeId::from_raw(5),
        };
        assert_eq!(dump_field(&f), "f: 5");
    }

    #[test]
    fn empty_enum_body_emits_empty_line() {
        let mut d = Dumper::new();
        dump_enum(&Atom::new_inline("E"), &[], &mut d);
        assert!(d.buf.contains("enum E {"));
    }

    #[test]
    fn function_with_body_emits_open_close_braces() {
        use crate::expr::HirExpr;
        use crate::stmt::HirStmt;
        let mut f = empty_func("body");
        f.body = vec![HirStmt::ret(Some(HirExpr::Int(0)))];
        let mut d = Dumper::new();
        dump_function(&f, &mut d);
        assert!(d.buf.contains("body: {"));
        assert!(d.buf.contains("}"));
    }

    #[test]
    fn module_id_propagates_through_dump_text() {
        let mut prog = HirProgram::new(ModuleId::from_raw(123));
        prog.push_decl(HirDecl::Function(empty_func("hello")));
        let text = prog.dump_text();
        assert!(text.contains("HirProgram(module=123)"));
        assert!(text.contains("fn hello"));
    }
}
