use std::collections::HashMap;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};

use ts_aot_core::{Atom, StructId, Type, TypeId, TypeTable, Visibility};
use ts_aot_ir_mir::{
    FunctionKind, MirDecl, MirExpr, MirFieldDecl, MirFunctionDecl, MirGlobalDecl, MirProgram,
    MirStructDecl,
};

use crate::error::BackendError;

struct EmitCtx<'a> {
    types: &'a TypeTable,
    struct_names: HashMap<StructId, Ident>,
}

impl<'a> EmitCtx<'a> {
    fn new(program: &MirProgram, types: &'a TypeTable) -> Self {
        let struct_names = program
            .declarations
            .iter()
            .filter_map(|decl| match decl {
                MirDecl::Struct(s) => Some((s.id, ident_from(&s.name))),
                _ => None,
            })
            .collect();
        Self {
            types,
            struct_names,
        }
    }

    #[cfg(test)]
    fn standalone(types: &'a TypeTable) -> Self {
        Self {
            types,
            struct_names: HashMap::new(),
        }
    }

    fn struct_ident(&self, id: StructId) -> Ident {
        self.struct_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format_ident!("__struct{}", id.raw()))
    }
}

pub fn emit_decls(program: &MirProgram, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::new(program, types);
    let mut tokens = TokenStream::new();
    for decl in &program.declarations {
        tokens.extend(emit_decl(decl, &ctx)?);
    }
    Ok(tokens)
}

fn emit_decl(decl: &MirDecl, ctx: &EmitCtx<'_>) -> Result<TokenStream, BackendError> {
    match decl {
        MirDecl::Function(f) => Ok(emit_function_with_ctx(f, ctx)),
        MirDecl::Struct(s) => Ok(emit_struct_with_ctx(s, ctx)),
        MirDecl::Global(g) => emit_global_with_ctx(g, ctx),
    }
}

#[cfg(test)]
fn emit_function(f: &MirFunctionDecl, types: &TypeTable) -> TokenStream {
    let ctx = EmitCtx::standalone(types);
    emit_function_with_ctx(f, &ctx)
}

fn emit_function_with_ctx(f: &MirFunctionDecl, ctx: &EmitCtx<'_>) -> TokenStream {
    let name = ident_from(&f.name);
    let params = emit_params(f, ctx);
    let ret = emit_type_id_with_ctx(f.ret, ctx);
    let vis = if f.export_name.is_some() {
        quote!(pub)
    } else {
        quote!()
    };
    let asyncness = if f.effects.is_async {
        quote!(async)
    } else {
        quote!()
    };
    let self_token = self_param_token(f.kind);
    let body = quote!({ unimplemented!() });

    quote! {
        #vis #asyncness fn #name(#self_token #(#params),*) -> #ret #body
    }
}

fn emit_struct_with_ctx(s: &MirStructDecl, ctx: &EmitCtx<'_>) -> TokenStream {
    let name = ctx.struct_ident(s.id);
    let fields = s.fields.iter().map(|f| emit_field(f, ctx));
    let mut methods = TokenStream::new();
    for m in &s.methods {
        methods.extend(emit_function_with_ctx(m, ctx));
    }
    quote! {
        pub struct #name {
            #(#fields,)*
        }

        impl #name {
            #methods
        }
    }
}

fn emit_field(field: &MirFieldDecl, ctx: &EmitCtx<'_>) -> TokenStream {
    let name = ident_from(&field.name);
    let ty = emit_type_id_with_ctx(field.ty, ctx);
    let vis = visibility_token(field.visibility);
    quote! {
        #vis #name: #ty
    }
}

#[cfg(test)]
fn emit_global(g: &MirGlobalDecl, types: &TypeTable) -> Result<TokenStream, BackendError> {
    let ctx = EmitCtx::standalone(types);
    emit_global_with_ctx(g, &ctx)
}

fn emit_global_with_ctx(g: &MirGlobalDecl, ctx: &EmitCtx<'_>) -> Result<TokenStream, BackendError> {
    let Some(expr) = &g.init else {
        return Err(BackendError::NotImplemented);
    };
    let name = ident_from(&g.name);
    let ty = emit_type_id_with_ctx(g.ty, ctx);
    let vis = visibility_token(g.visibility);
    let mutability = if g.mutable { quote!(mut) } else { quote!() };
    let init = emit_const_expr(expr)?;
    Ok(quote! {
        #vis static #mutability #name: #ty = #init;
    })
}

fn emit_const_expr(expr: &MirExpr) -> Result<TokenStream, BackendError> {
    match expr {
        MirExpr::Unit | MirExpr::Null { .. } => Ok(quote!(())),
        MirExpr::Bool(value) => Ok(quote!(#value)),
        MirExpr::Int { value, .. } => Ok(emit_whole_number_literal(*value)),
        MirExpr::Float { value, .. } if value.is_finite() => {
            let literal = Literal::f64_unsuffixed(*value);
            Ok(quote!(#literal))
        }
        _ => Err(BackendError::NotImplemented),
    }
}

fn emit_whole_number_literal(value: i128) -> TokenStream {
    if value < 0 {
        let magnitude = Literal::u128_unsuffixed(value.unsigned_abs());
        quote!(-#magnitude)
    } else {
        let value = u128::try_from(value).expect("non-negative literal must fit u128");
        let literal = Literal::u128_unsuffixed(value);
        quote!(#literal)
    }
}

fn emit_params(f: &MirFunctionDecl, ctx: &EmitCtx<'_>) -> Vec<TokenStream> {
    let self_param = match f.kind {
        FunctionKind::Method { self_param, .. } => Some(self_param),
        _ => None,
    };
    f.params
        .iter()
        .filter(|p| Some(p.id) != self_param)
        .map(|p| {
            let name = ident_from(&p.name);
            let ty = emit_type_id_with_ctx(p.ty, ctx);
            quote!(#name: #ty)
        })
        .collect()
}

fn self_param_token(kind: FunctionKind) -> TokenStream {
    match kind {
        FunctionKind::Method { .. } | FunctionKind::Constructor { .. } => quote!(self,),
        _ => TokenStream::new(),
    }
}

fn visibility_token(vis: Visibility) -> TokenStream {
    match vis {
        Visibility::Public => quote!(pub),
        Visibility::Private | Visibility::Protected => quote!(),
    }
}

fn ident_from(atom: &Atom) -> Ident {
    let raw = atom.as_str();
    let sanitized = sanitize_ident(raw);
    Ident::new(&sanitized, Span::call_site())
}

fn sanitize_ident(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for (i, ch) in raw.chars().enumerate() {
        let valid = ch == '_' || ch.is_ascii_alphanumeric();
        if valid {
            if i == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    if is_rust_keyword(&out) {
        out.push('_');
    }
    out
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}

#[cfg(test)]
fn emit_type_id(id: TypeId, types: &TypeTable) -> TokenStream {
    let ctx = EmitCtx::standalone(types);
    emit_type_id_with_ctx(id, &ctx)
}

fn emit_type_id_with_ctx(id: TypeId, ctx: &EmitCtx<'_>) -> TokenStream {
    if let Some(ty) = ctx.types.resolve(id) {
        emit_type(ty, ctx)
    } else {
        let ident = format_ident!("__ty{}", id.raw());
        quote!(#ident)
    }
}

fn emit_type(ty: &Type, ctx: &EmitCtx<'_>) -> TokenStream {
    match ty {
        Type::Void | Type::Null | Type::Error => quote!(()),
        Type::Never => quote!(!),
        Type::Bool => quote!(bool),
        Type::I8 => quote!(i8),
        Type::I16 => quote!(i16),
        Type::I32 => quote!(i32),
        Type::I64 => quote!(i64),
        Type::U8 => quote!(u8),
        Type::U16 => quote!(u16),
        Type::U32 => quote!(u32),
        Type::U64 => quote!(u64),
        Type::F32 => quote!(f32),
        Type::F64 => quote!(f64),
        Type::String => quote!(String),
        Type::Optional { inner } => {
            let inner_tokens = emit_type_id_with_ctx(*inner, ctx);
            quote!(Option<#inner_tokens>)
        }
        Type::Array { element } => {
            let element_tokens = emit_type_id_with_ctx(*element, ctx);
            quote!(Vec<#element_tokens>)
        }
        Type::Struct { id } => {
            let ident = ctx.struct_ident(*id);
            quote!(#ident)
        }
        Type::Fn { .. } | Type::Promise { .. } | Type::Result { .. } => {
            quote!(())
        }
        Type::Named { symbol } => {
            let raw = symbol.as_str();
            let sanitized = sanitize_ident(raw);
            let ident = Ident::new(&sanitized, Span::call_site());
            quote!(#ident)
        }
        Type::GenericParam { id } => {
            let ident = format_ident!("__generic{}", id.raw());
            quote!(#ident)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, ModuleId, StructId, TypeId};
    use ts_aot_ir_mir::{FunctionEffects, FunctionKind, MirBody, MirParam};

    fn empty_func(name: &str) -> MirFunctionDecl {
        MirFunctionDecl {
            id: FunctionId::from_raw(0),
            name: Atom::from(name),
            export_name: None,
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: MirBody::default(),
            kind: FunctionKind::Plain,
            effects: FunctionEffects::default(),
        }
    }

    #[test]
    fn empty_program_emits_no_decls() {
        let prog = MirProgram::new(ModuleId::from_raw(0));
        let types = TypeTable::new();
        let tokens = emit_decls(&prog, &types).expect("decls should emit");
        assert!(tokens.is_empty());
    }

    #[test]
    fn plain_function_emits_fn_signature() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Function(empty_func("greet")));
        let types = TypeTable::new();
        let tokens = emit_decls(&prog, &types).expect("decls should emit");
        let s = tokens.to_string();
        assert!(s.contains("fn greet"), "got: {s}");
        assert!(s.contains("->"), "expected ret arrow, got: {s}");
    }

    #[test]
    fn exported_function_emits_pub_keyword() {
        let mut f = empty_func("render");
        f.export_name = Some("render".to_owned());
        let tokens = emit_function(&f, &TypeTable::new());
        let s = tokens.to_string();
        assert!(s.starts_with("pub "), "expected `pub` prefix, got: {s}");
    }

    #[test]
    fn private_function_omits_pub_keyword() {
        let tokens = emit_function(&empty_func("internal"), &TypeTable::new());
        let s = tokens.to_string();
        assert!(
            !s.contains("pub "),
            "private function should not have `pub`, got: {s}"
        );
    }

    #[test]
    fn async_function_emits_async_keyword() {
        let mut f = empty_func("fetch_data");
        f.effects.is_async = true;
        let tokens = emit_function(&f, &TypeTable::new());
        let s = tokens.to_string();
        assert!(s.contains("async fn"), "got: {s}");
    }

    #[test]
    fn method_kind_emits_self_param() {
        let mut f = empty_func("method");
        f.kind = FunctionKind::Method {
            owner: StructId::from_raw(0),
            self_param: LocalId::from_raw(0),
        };
        let tokens = emit_function(&f, &TypeTable::new());
        let s = tokens.to_string();
        assert!(s.contains("self"), "method must emit `self`, got: {s}");
    }

    #[test]
    fn method_kind_omits_synthetic_this_param() {
        let mut types = TypeTable::new();
        let number_ty = types.intern(&Type::I32);
        let mut f = empty_func("method");
        f.kind = FunctionKind::Method {
            owner: StructId::from_raw(0),
            self_param: LocalId::from_raw(0),
        };
        f.params = vec![
            MirParam {
                id: LocalId::from_raw(0),
                name: Atom::from("this"),
                ty: number_ty,
            },
            MirParam {
                id: LocalId::from_raw(1),
                name: Atom::from("value"),
                ty: number_ty,
            },
        ];
        let tokens = emit_function(&f, &types);
        let s = tokens.to_string();
        assert!(
            s.contains("self"),
            "method must emit Rust receiver, got: {s}"
        );
        assert!(
            !s.contains("this :"),
            "method signature must hide synthetic receiver param, got: {s}"
        );
        assert!(s.contains("value : i32"), "expected value param, got: {s}");
    }

    #[test]
    fn struct_decl_emits_struct_keyword() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Struct(MirStructDecl {
            id: StructId::from_raw(0),
            name: Atom::from("Point"),
            fields: Vec::new(),
            methods: Vec::new(),
        }));
        let tokens = emit_decls(&prog, &TypeTable::new()).expect("decls should emit");
        let s = tokens.to_string();
        assert!(s.contains("pub struct Point"), "got: {s}");
    }

    #[test]
    fn struct_with_fields_emits_field_list() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Struct(MirStructDecl {
            id: StructId::from_raw(0),
            name: Atom::from("Rect"),
            fields: vec![
                MirFieldDecl {
                    id: FieldId::from_raw(0),
                    name: Atom::from("width"),
                    ty: TypeId::from_raw(0),
                    mutable: false,
                    visibility: Visibility::Public,
                },
                MirFieldDecl {
                    id: FieldId::from_raw(1),
                    name: Atom::from("height"),
                    ty: TypeId::from_raw(0),
                    mutable: false,
                    visibility: Visibility::Private,
                },
            ],
            methods: Vec::new(),
        }));
        let tokens = emit_decls(&prog, &TypeTable::new()).expect("decls should emit");
        let s = tokens.to_string();
        assert!(s.contains("pub struct Rect"), "got: {s}");
        assert!(s.contains("width :"), "expected field width, got: {s}");
        assert!(s.contains("height :"), "expected field height, got: {s}");
        assert!(
            s.contains("pub width"),
            "expected `pub` on public field, got: {s}"
        );
        assert!(
            !s.contains("pub height"),
            "private field must not have `pub`, got: {s}"
        );
    }

    #[test]
    fn struct_type_reference_uses_declared_struct_name() {
        let struct_id = StructId::from_raw(7);
        let mut types = TypeTable::new();
        let point_ty = types.intern(&Type::Struct { id: struct_id });
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Struct(MirStructDecl {
            id: struct_id,
            name: Atom::from("Point"),
            fields: Vec::new(),
            methods: Vec::new(),
        }));
        let mut f = empty_func("make_point");
        f.ret = point_ty;
        prog.push_decl(MirDecl::Function(f));

        let tokens = emit_decls(&prog, &types).expect("decls should emit");
        let s = tokens.to_string();
        assert!(s.contains("pub struct Point"), "got: {s}");
        assert!(s.contains("-> Point"), "got: {s}");
        assert!(!s.contains("__struct7"), "got: {s}");
    }

    #[test]
    fn global_without_init_returns_not_implemented() {
        let mut prog = MirProgram::new(ModuleId::from_raw(0));
        prog.push_decl(MirDecl::Global(MirGlobalDecl {
            name: Atom::from("counter"),
            ty: TypeId::from_raw(0),
            mutable: false,
            visibility: Visibility::Public,
            export_name: None,
            init: None,
        }));
        let err = emit_decls(&prog, &TypeTable::new()).expect_err("global init is required");
        assert_eq!(err, BackendError::NotImplemented);
    }

    #[test]
    fn global_with_non_const_init_returns_not_implemented() {
        let err = emit_global(
            &MirGlobalDecl {
                name: Atom::from("counter"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Public,
                export_name: None,
                init: Some(MirExpr::Global(Atom::from("other"))),
            },
            &TypeTable::new(),
        )
        .expect_err("non-const global init must not emit invalid static initializer");
        assert_eq!(err, BackendError::NotImplemented);
    }

    #[test]
    fn global_with_const_int_init_emits_initializer() {
        let tokens = emit_global(
            &MirGlobalDecl {
                name: Atom::from("counter"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Public,
                export_name: None,
                init: Some(MirExpr::Int {
                    value: 42,
                    ty: TypeId::from_raw(0),
                }),
            },
            &TypeTable::new(),
        )
        .expect("const global init should emit");
        let s = tokens.to_string();
        assert!(s.contains("= 42"), "got: {s}");
        assert!(!s.contains("Default :: default"), "got: {s}");
        assert!(!s.contains("unimplemented"), "got: {s}");
    }

    #[test]
    fn public_global_emits_pub_from_visibility_without_export_name() {
        let tokens = emit_global(
            &MirGlobalDecl {
                name: Atom::from("counter"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Public,
                export_name: None,
                init: Some(MirExpr::Int {
                    value: 0,
                    ty: TypeId::from_raw(0),
                }),
            },
            &TypeTable::new(),
        )
        .expect("const global init should emit");
        let s = tokens.to_string();
        assert!(s.starts_with("pub static counter"), "got: {s}");
    }

    #[test]
    fn private_global_omits_pub_from_visibility() {
        let tokens = emit_global(
            &MirGlobalDecl {
                name: Atom::from("secret"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Private,
                export_name: Some("secret".to_owned()),
                init: Some(MirExpr::Bool(true)),
            },
            &TypeTable::new(),
        )
        .expect("const global init should emit");
        let s = tokens.to_string();
        assert!(s.starts_with("static secret"), "got: {s}");
    }

    #[test]
    fn mutable_global_emits_mut_from_flag() {
        let tokens = emit_global(
            &MirGlobalDecl {
                name: Atom::from("counter"),
                ty: TypeId::from_raw(0),
                mutable: true,
                visibility: Visibility::Public,
                export_name: None,
                init: Some(MirExpr::Int {
                    value: 0,
                    ty: TypeId::from_raw(0),
                }),
            },
            &TypeTable::new(),
        )
        .expect("const global init should emit");
        let s = tokens.to_string();
        assert!(s.starts_with("pub static mut counter"), "got: {s}");
    }

    #[test]
    fn immutable_global_omits_mut_from_flag() {
        let tokens = emit_global(
            &MirGlobalDecl {
                name: Atom::from("counter"),
                ty: TypeId::from_raw(0),
                mutable: false,
                visibility: Visibility::Public,
                export_name: None,
                init: Some(MirExpr::Int {
                    value: 0,
                    ty: TypeId::from_raw(0),
                }),
            },
            &TypeTable::new(),
        )
        .expect("const global init should emit");
        let s = tokens.to_string();
        assert!(s.starts_with("pub static counter"), "got: {s}");
        assert!(!s.contains("static mut"), "got: {s}");
    }

    #[test]
    fn sanitize_ident_replaces_dash_with_underscore() {
        assert_eq!(sanitize_ident("foo-bar"), "foo_bar");
    }

    #[test]
    fn sanitize_ident_prefixes_digit_start() {
        assert_eq!(sanitize_ident("7greet"), "_7greet");
    }

    #[test]
    fn sanitize_ident_appends_underscore_to_keyword() {
        assert_eq!(sanitize_ident("type"), "type_");
        assert_eq!(sanitize_ident("fn"), "fn_");
    }

    #[test]
    fn emit_type_resolves_primitives() {
        let mut types = TypeTable::new();
        let i32_id = types.intern(&Type::I32);
        let bool_id = types.intern(&Type::Bool);
        let tokens = emit_type_id(i32_id, &types);
        assert_eq!(tokens.to_string(), "i32");
        let tokens = emit_type_id(bool_id, &types);
        assert_eq!(tokens.to_string(), "bool");
    }

    #[test]
    fn emit_type_for_unknown_id_emits_placeholder() {
        let types = TypeTable::new();
        let tokens = emit_type_id(TypeId::from_raw(42), &types);
        assert!(tokens.to_string().contains("__ty42"), "got: {tokens}");
    }

    #[test]
    fn emit_type_optional_resolves_inner_via_table() {
        let mut types = TypeTable::new();
        let i32_id = types.intern(&Type::I32);
        let opt_id = types.intern(&Type::Optional { inner: i32_id });
        let tokens = emit_type_id(opt_id, &types);
        assert_eq!(tokens.to_string(), "Option < i32 >");
    }

    #[test]
    fn emit_type_array_resolves_element_via_table() {
        let mut types = TypeTable::new();
        let str_id = types.intern(&Type::String);
        let arr_id = types.intern(&Type::Array { element: str_id });
        let tokens = emit_type_id(arr_id, &types);
        assert_eq!(tokens.to_string(), "Vec < String >");
    }
}
