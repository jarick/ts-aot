use super::super::{ClassPath, build_class_index, class_at_path, index_decl};
use super::common::*;
use std::collections::HashMap;
use ts_aot_ir_hir::{HirClass, HirField};

fn class_with_fields(name: &str, ty: TypeId, pre_pass_ty: Option<TypeId>) -> HirClass {
    HirClass {
        name: Atom::new_inline(name),
        ty,
        pre_pass_ty,
        fields: vec![HirField {
            name: Atom::new_inline("v"),
            ty: TypeId::from_raw(99),
        }],
        methods: Vec::new(),
        extends: None,
        type_params: Vec::new(),
    }
}

#[test]
fn build_class_index_top_level_class_uses_single_index_path() {
    let top_ty = TypeId::from_raw(11);
    let program = vec![HirDecl::Class(class_with_fields("Top", top_ty, None))];

    let index = build_class_index(&program);

    let path = index
        .get(&top_ty)
        .expect("top-level class ty must be indexed");
    assert_eq!(
        path.0,
        vec![0],
        "top-level class must map to a single-element path (declarations[0]), got {:?}",
        path.0
    );
    let class =
        class_at_path(&program, path.clone()).expect("class_at_path must resolve the top class");
    assert_eq!(class.name.as_str(), "Top");
}

#[test]
fn build_class_index_nested_class_in_namespace_uses_two_index_path() {
    let nested_ty = TypeId::from_raw(22);
    let program = vec![HirDecl::Namespace {
        name: Atom::new_inline("Outer"),
        members: vec![HirDecl::Class(class_with_fields("Inner", nested_ty, None))],
    }];

    let index = build_class_index(&program);

    let path = index
        .get(&nested_ty)
        .expect("nested class ty must be indexed");
    assert_eq!(
        path.0,
        vec![0, 0],
        "class inside a top-level namespace must map to a two-element path \
         (declarations[0] → members[0]), got {:?}",
        path.0
    );
    let class =
        class_at_path(&program, path.clone()).expect("class_at_path must walk the namespace path");
    assert_eq!(class.name.as_str(), "Inner");
}

#[test]
fn build_class_index_nested_class_records_pre_pass_ty_when_distinct() {
    let nested_ty = TypeId::from_raw(33);
    let pre_pass_ty = TypeId::from_raw(34);
    let program = vec![HirDecl::Namespace {
        name: Atom::new_inline("Outer"),
        members: vec![HirDecl::Class(class_with_fields(
            "Inner",
            nested_ty,
            Some(pre_pass_ty),
        ))],
    }];

    let index = build_class_index(&program);

    let ty_path = index
        .get(&nested_ty)
        .expect("nested class ty must be indexed");
    let pre_path = index
        .get(&pre_pass_ty)
        .expect("nested class pre_pass_ty must be indexed when distinct from ty");
    assert_eq!(
        ty_path.0, pre_path.0,
        "pre_pass_ty must point to the same nested class as ty, \
         got ty path {:?} vs pre_pass_ty path {:?}",
        ty_path.0, pre_path.0
    );
    let class_from_ty = class_at_path(&program, ty_path.clone()).expect("ty path must resolve");
    let class_from_pre =
        class_at_path(&program, pre_path.clone()).expect("pre_pass_ty path must resolve");
    assert_eq!(class_from_ty.name.as_str(), "Inner");
    assert_eq!(class_from_pre.name.as_str(), "Inner");
    assert!(std::ptr::eq(class_from_ty, class_from_pre));
}

#[test]
fn build_class_index_nested_class_inside_deeper_namespace_uses_recursive_path() {
    let deep_ty = TypeId::from_raw(44);
    let program = vec![HirDecl::Namespace {
        name: Atom::new_inline("Outer"),
        members: vec![HirDecl::Namespace {
            name: Atom::new_inline("Inner"),
            members: vec![HirDecl::Class(class_with_fields("Deep", deep_ty, None))],
        }],
    }];

    let index = build_class_index(&program);

    let path = index
        .get(&deep_ty)
        .expect("deeply nested class ty must be indexed");
    assert_eq!(
        path.0,
        vec![0, 0, 0],
        "class inside a nested namespace must use a three-element path \
         (declarations[0] → members[0] → members[0]), got {:?}",
        path.0
    );
    let class =
        class_at_path(&program, path.clone()).expect("class_at_path must walk recursive path");
    assert_eq!(class.name.as_str(), "Deep");
}

#[test]
fn build_class_index_preserves_top_level_class_indices_alongside_namespaces() {
    let top_ty = TypeId::from_raw(55);
    let nested_ty = TypeId::from_raw(56);
    let program = vec![
        HirDecl::Class(class_with_fields("Top", top_ty, None)),
        HirDecl::Namespace {
            name: Atom::new_inline("Outer"),
            members: vec![HirDecl::Class(class_with_fields("Inner", nested_ty, None))],
        },
    ];

    let index = build_class_index(&program);

    let top_path = index
        .get(&top_ty)
        .expect("top-level class ty must remain indexed alongside namespaces");
    assert_eq!(
        top_path.0,
        vec![0],
        "top-level class must still map to its single declaration index, got {:?}",
        top_path.0
    );
    let nested_path = index
        .get(&nested_ty)
        .expect("nested class must also be indexed in the same index");
    assert_eq!(
        nested_path.0,
        vec![1, 0],
        "nested class path must point to declarations[1] → members[0], got {:?}",
        nested_path.0
    );
}

#[test]
fn index_decl_inserts_at_supplied_path_for_classes() {
    let ty = TypeId::from_raw(77);
    let mut index = HashMap::new();
    let decl = HirDecl::Class(class_with_fields("Only", ty, None));

    index_decl(&decl, ClassPath(vec![3, 1]), &mut index);

    let path = index
        .get(&ty)
        .expect("index_decl must insert the class at the supplied path");
    assert_eq!(path.0, vec![3, 1]);
}

#[test]
fn build_class_index_empty_declarations_returns_empty_index() {
    let program: Vec<HirDecl> = Vec::new();

    let index = build_class_index(&program);

    assert!(
        index.is_empty(),
        "empty declarations must produce an empty class_index; got {index:?}"
    );
}

#[test]
fn build_class_index_skips_non_class_declarations_and_non_class_namespace_members() {
    let inner_ty = TypeId::from_raw(80);
    let program = vec![
        HirDecl::Function(HirFunction {
            name: Atom::new_inline("helper"),
            params: Vec::new(),
            ret: TypeId::from_raw(0),
            throws: None,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        }),
        HirDecl::Namespace {
            name: Atom::new_inline("Outer"),
            members: vec![
                HirDecl::Function(HirFunction {
                    name: Atom::new_inline("nsHelper"),
                    params: Vec::new(),
                    ret: TypeId::from_raw(0),
                    throws: None,
                    body: Vec::new(),
                    is_async: false,
                    is_generator: false,
                    is_exported: false,
                    type_params: Vec::new(),
                    async_info: None,
                }),
                HirDecl::Class(class_with_fields("Inner", inner_ty, None)),
            ],
        },
    ];

    let index = build_class_index(&program);

    assert_eq!(
        index.len(),
        1,
        "only the class must be indexed; functions must be skipped at top level and inside namespaces, got {index:?}"
    );
    let path = index
        .get(&inner_ty)
        .expect("nested class must still be indexed when other members are non-classes");
    assert_eq!(path.0, vec![1, 1]);
}

#[test]
fn class_at_path_out_of_bounds_index_returns_none() {
    let program = vec![HirDecl::Class(class_with_fields(
        "Only",
        TypeId::from_raw(90),
        None,
    ))];

    let missing = class_at_path(&program, ClassPath(vec![5]));

    assert!(
        missing.is_none(),
        "path pointing past the end of declarations must return None, got {missing:?}"
    );
}

#[test]
fn class_at_path_traverses_through_non_namespace_returns_none() {
    let program = vec![
        HirDecl::Class(class_with_fields("Top", TypeId::from_raw(91), None)),
        HirDecl::Class(class_with_fields("Mid", TypeId::from_raw(92), None)),
    ];

    let invalid = class_at_path(&program, ClassPath(vec![0, 0]));

    assert!(
        invalid.is_none(),
        "descending into a non-namespace mid-path must return None, got {invalid:?}"
    );
}
