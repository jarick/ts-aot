use super::*;
use ts_aot_core::{Atom, TypeId, TypeTable};
use ts_aot_ir_hir::{HirClass, HirField, HirFunction, HirParam, HirStmt};

fn setup() -> (HirProgram, TypeTable, PassContext) {
    let types = TypeTable::new();
    let ctx = PassContext::default();
    (
        HirProgram::new(ts_aot_core::ModuleId::from_raw(0)),
        types,
        ctx,
    )
}

fn class_with_fields_and_methods(
    name: &str,
    fields: Vec<(&str, u32)>,
    methods: Vec<(&str, u32)>,
    extends: Option<&str>,
) -> HirClass {
    HirClass {
        name: Atom::new_inline(name),
        ty: TypeId::from_raw(0),
        fields: fields
            .into_iter()
            .map(|(n, t)| HirField {
                name: Atom::new_inline(n),
                ty: TypeId::from_raw(t),
            })
            .collect(),
        methods: methods
            .into_iter()
            .map(|(n, p)| HirFunction {
                name: Atom::new_inline(n),
                params: vec![HirParam {
                    name: Atom::new_inline("self"),
                    ty: TypeId::from_raw(p),
                }],
                ret: TypeId::from_raw(0),
                throws: None,
                body: vec![HirStmt::Return { value: None }],
                is_async: false,
                is_generator: false,
                is_exported: false,
                type_params: Vec::new(),
                async_info: None,
            })
            .collect(),
        extends: extends.map(Atom::new_inline),
        type_params: Vec::new(),
    }
}

fn find_class<'a>(program: &'a HirProgram, name: &str) -> &'a HirClass {
    program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Class(c) if c.name.as_str() == name => Some(c),
            HirDecl::Namespace { members, .. } => members.iter().find_map(|m| match m {
                HirDecl::Class(c) if c.name.as_str() == name => Some(c),
                _ => None,
            }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("class {name} not found"))
}

fn find_class_in_ns<'a>(program: &'a HirProgram, ns: &str, name: &str) -> &'a HirClass {
    program
        .declarations
        .iter()
        .find_map(|d| match d {
            HirDecl::Namespace { name: n, members } if n.as_str() == ns => {
                members.iter().find_map(|m| match m {
                    HirDecl::Class(c) if c.name.as_str() == name => Some(c),
                    _ => None,
                })
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{ns}.{name} not found"))
}

#[test]
fn class_without_extends_passes_through_unchanged() {
    let (mut program, mut types, mut ctx) = setup();
    let class = class_with_fields_and_methods("A", vec![("x", 1)], vec![], None);
    program.push_decl(HirDecl::Class(class.clone()));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "A");
    assert_eq!(c.fields, class.fields);
    assert_eq!(c.methods, class.methods);
    assert_eq!(c.extends, class.extends);
    assert_eq!(stats.flattened_classes, 0);
    assert!(!ctx.has_errors());
}

#[test]
fn extends_inherits_parent_fields_ordered_before_own() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Parent",
        vec![("a", 1), ("b", 2)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Child",
        vec![("c", 3)],
        vec![],
        Some("Parent"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "Child");
    let names: Vec<String> = c
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    assert_eq!(names, vec!["a", "b", "c"], "parent fields first, own last");
    assert_eq!(stats.flattened_classes, 1);
    assert_eq!(stats.inherited_fields, 2);
}

#[test]
fn extends_inherits_parent_methods() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Parent",
        vec![],
        vec![("greet", 7)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Child",
        vec![],
        vec![("other", 8)],
        Some("Parent"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "Child");
    let names: Vec<String> = c
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert_eq!(names, vec!["greet", "other"]);
    assert_eq!(stats.inherited_methods, 1);
}

#[test]
fn own_method_overrides_parent_method_by_name() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Parent",
        vec![],
        vec![("m", 7), ("kept", 8)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Child",
        vec![],
        vec![("m", 9)],
        Some("Parent"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "Child");
    let names: Vec<String> = c
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert_eq!(
        names,
        vec!["kept", "m"],
        "non-overridden parent first, then own"
    );

    let own_m = c.methods.iter().find(|m| m.name.as_str() == "m").unwrap();
    assert_eq!(
        own_m.params[0].ty,
        TypeId::from_raw(9),
        "own version must win"
    );
    assert_eq!(stats.overridden_methods, 1);
}

#[test]
fn multi_level_inheritance_grandparent_parent_child() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "GP",
        vec![("g", 1)],
        vec![("gm", 5)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P",
        vec![("p", 2)],
        vec![("pm", 6)],
        Some("GP"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![("c", 3)],
        vec![("cm", 7)],
        Some("P"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "C");
    let f_names: Vec<String> = c
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    assert_eq!(f_names, vec!["g", "p", "c"], "fields: gp -> p -> c");

    let m_names: Vec<String> = c
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert_eq!(m_names, vec!["gm", "pm", "cm"], "methods: gp -> p -> c");

    assert_eq!(
        stats.flattened_classes, 2,
        "P (extends GP) and C (extends P) flattened"
    );
    assert_eq!(
        stats.inherited_fields, 3,
        "P inherits 1, C inherits 2 → total 3"
    );
    assert_eq!(
        stats.inherited_methods, 3,
        "P inherits 1, C inherits 2 → total 3"
    );
}

#[test]
fn diamond_inheritance_keeps_each_ancestor_fields_in_chain_order() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Top",
        vec![("t", 1)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Left",
        vec![("l", 2)],
        vec![],
        Some("Top"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Right",
        vec![("r", 3)],
        vec![],
        Some("Top"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Bottom",
        vec![("b", 4)],
        vec![],
        Some("Left"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let b = find_class(&program, "Bottom");
    let names: Vec<String> = b
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    assert_eq!(names, vec!["t", "l", "b"]);
    assert_eq!(
        stats.flattened_classes, 3,
        "Left, Right, Bottom flattened; Top has no extends"
    );
}

#[test]
fn unknown_parent_emits_diagnostic_and_leaves_class_alone() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Child",
        vec![("c", 1)],
        vec![],
        Some("Missing"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.missing_parents, 1);
    assert_eq!(stats.flattened_classes, 0);
    assert!(ctx.diagnostics().iter().any(|d| d.code.as_str() == "P0009"));
    let c = find_class(&program, "Child");
    assert_eq!(
        c.extends, None,
        "extends must be cleared after missing parent"
    );
}

#[test]
fn self_extends_is_a_cycle() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Loopy",
        vec![("v", 1)],
        vec![],
        Some("Loopy"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.cycles_detected, 1);
    assert!(ctx.diagnostics().iter().any(|d| d.code.as_str() == "P0008"));
    let c = find_class(&program, "Loopy");
    assert_eq!(c.extends, None);
}

#[test]
fn two_class_cycle_is_detected() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "A",
        vec![("a", 1)],
        vec![],
        Some("B"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "B",
        vec![("b", 2)],
        vec![],
        Some("A"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.cycles_detected, 2, "both A and B hit the cycle");
    assert!(
        ctx.diagnostics()
            .iter()
            .filter(|d| d.code.as_str() == "P0008")
            .count()
            >= 1
    );
}

#[test]
fn three_class_cycle_is_detected() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "A",
        vec![],
        vec![],
        Some("B"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "B",
        vec![],
        vec![],
        Some("C"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![],
        vec![],
        Some("A"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    assert!(stats.cycles_detected >= 1);
    assert!(ctx.diagnostics().iter().any(|d| d.code.as_str() == "P0008"));
}

#[test]
fn parent_chain_inherits_through_three_levels() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "A",
        vec![("a", 1)],
        vec![("ga", 5)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "B",
        vec![("b", 2)],
        vec![("gb", 6)],
        Some("A"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![("c", 3)],
        vec![("gc", 7)],
        Some("B"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "C");
    let f: Vec<_> = c
        .fields
        .iter()
        .map(|x| x.name.as_str().to_owned())
        .collect();
    let m: Vec<_> = c
        .methods
        .iter()
        .map(|x| x.name.as_str().to_owned())
        .collect();
    assert_eq!(f, vec!["a", "b", "c"]);
    assert_eq!(m, vec!["ga", "gb", "gc"]);
    assert_eq!(
        stats.flattened_classes, 2,
        "B (extends A) and C (extends B) flattened, A passed through"
    );
    assert!(!ctx.has_errors());
}

#[test]
fn class_inside_namespace_resolves_extends() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Top",
        vec![("t", 1)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("ns"),
        members: vec![HirDecl::Class(class_with_fields_and_methods(
            "Nested",
            vec![("n", 2)],
            vec![],
            Some("Top"),
        ))],
    });

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.flattened_classes, 1);
    let inner = find_class(&program, "Nested");
    let names: Vec<String> = inner
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    assert_eq!(names, vec!["t", "n"]);
}

#[test]
fn same_name_class_in_different_namespaces_each_inherits_its_own_base() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "BaseA",
        vec![("a", 1)],
        vec![("m_a", 11)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "BaseB",
        vec![("b", 2)],
        vec![("m_b", 22)],
        None,
    )));
    program.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("ns1"),
        members: vec![HirDecl::Class(class_with_fields_and_methods(
            "Child",
            vec![("c1", 3)],
            vec![],
            Some("BaseA"),
        ))],
    });
    program.push_decl(HirDecl::Namespace {
        name: Atom::new_inline("ns2"),
        members: vec![HirDecl::Class(class_with_fields_and_methods(
            "Child",
            vec![("c2", 4)],
            vec![],
            Some("BaseB"),
        ))],
    });

    let _ = lower_classes(&mut program, &mut types, &mut ctx);

    let ns1_child = find_class_in_ns(&program, "ns1", "Child");
    let ns2_child = find_class_in_ns(&program, "ns2", "Child");

    let ns1_fields: Vec<String> = ns1_child
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    let ns2_fields: Vec<String> = ns2_child
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    let ns1_methods: Vec<String> = ns1_child
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    let ns2_methods: Vec<String> = ns2_child
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();

    assert_eq!(
        ns1_fields,
        vec!["a", "c1"],
        "ns1.Child inherits BaseA field then own"
    );
    assert_eq!(ns1_methods, vec!["m_a"], "ns1.Child inherits BaseA method");
    assert_eq!(
        ns2_fields,
        vec!["b", "c2"],
        "ns2.Child inherits BaseB field then own"
    );
    assert_eq!(ns2_methods, vec!["m_b"], "ns2.Child inherits BaseB method");
}

#[test]
fn class_with_no_field_or_method_keeps_chain_extends() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Empty",
        vec![],
        vec![],
        Some("Other"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Other",
        vec![("o", 1)],
        vec![],
        None,
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let e = find_class(&program, "Empty");
    assert_eq!(e.fields.len(), 1);
    assert_eq!(e.fields[0].name.as_str(), "o");
    assert_eq!(stats.flattened_classes, 1);
}

#[test]
fn multiple_independent_class_pairs_all_flatten() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P1",
        vec![("p1", 1)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C1",
        vec![("c1", 2)],
        vec![],
        Some("P1"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P2",
        vec![("p2", 3)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C2",
        vec![("c2", 4)],
        vec![],
        Some("P2"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.flattened_classes, 2);
    let c1 = find_class(&program, "C1");
    let c2 = find_class(&program, "C2");
    assert_eq!(
        c1.fields
            .iter()
            .map(|f| f.name.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec!["p1", "c1"]
    );
    assert_eq!(
        c2.fields
            .iter()
            .map(|f| f.name.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec!["p2", "c2"]
    );
}

#[test]
fn parent_field_with_same_name_as_child_field_is_kept_twice() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Parent",
        vec![("x", 1)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Child",
        vec![("x", 2)],
        vec![],
        Some("Parent"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "Child");
    assert_eq!(c.fields.len(), 2);
    assert_eq!(stats.inherited_fields, 1);
}

#[test]
fn empty_program_is_a_noop() {
    let (mut program, mut types, mut ctx) = setup();
    let stats = lower_classes(&mut program, &mut types, &mut ctx);
    assert_eq!(stats.flattened_classes, 0);
    assert!(program.declarations.is_empty());
    assert!(!ctx.has_errors());
}

#[test]
fn ids_are_used_as_type_ids_for_fields() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "A",
        vec![("a", 11), ("b", 12)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "B",
        vec![("c", 13)],
        vec![],
        Some("A"),
    )));

    lower_classes(&mut program, &mut types, &mut ctx);

    let b = find_class(&program, "B");
    let types_: Vec<u32> = b.fields.iter().map(|f| f.ty.raw()).collect();
    assert_eq!(types_, vec![11, 12, 13]);
}

#[test]
fn stats_are_zero_for_no_inheritance() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Lonely",
        vec![("x", 1)],
        vec![],
        None,
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    assert_eq!(stats.flattened_classes, 0);
    assert_eq!(stats.inherited_fields, 0);
    assert_eq!(stats.inherited_methods, 0);
    assert_eq!(stats.overridden_methods, 0);
    assert_eq!(stats.cycles_detected, 0);
    assert_eq!(stats.missing_parents, 0);
}

#[test]
fn stats_track_inheritance_correctly() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Base",
        vec![("a", 1), ("b", 2)],
        vec![("m1", 5), ("m2", 6)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Mid",
        vec![("c", 3)],
        vec![("m2", 7), ("new_m", 8)],
        Some("Base"),
    )));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let mid = find_class(&program, "Mid");
    let f: Vec<_> = mid
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    let m: Vec<_> = mid
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert_eq!(f, vec!["a", "b", "c"]);
    assert_eq!(m, vec!["m1", "m2", "new_m"]);

    assert_eq!(stats.flattened_classes, 1);
    assert_eq!(stats.inherited_fields, 2);
    assert_eq!(stats.inherited_methods, 1);
    assert_eq!(stats.overridden_methods, 1, "m2 overridden by Mid");
}

#[test]
fn flatten_is_idempotent_when_run_twice() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P",
        vec![("p", 1)],
        vec![],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![("c", 2)],
        vec![],
        Some("P"),
    )));

    let _ = lower_classes(&mut program, &mut types, &mut ctx);
    let second = lower_classes(&mut program, &mut types, &mut ctx);

    assert_eq!(second.flattened_classes, 0, "second run sees extends=None");
    let c = find_class(&program, "C");
    assert_eq!(c.extends, None);
    let f: Vec<_> = c
        .fields
        .iter()
        .map(|f| f.name.as_str().to_owned())
        .collect();
    assert_eq!(f, vec!["p", "c"]);
}

#[test]
fn parent_definition_is_preserved_unchanged() {
    let (mut program, mut types, mut ctx) = setup();
    let parent = class_with_fields_and_methods("Parent", vec![("x", 1)], vec![("g", 5)], None);
    program.push_decl(HirDecl::Class(parent.clone()));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Child",
        vec![("y", 2)],
        vec![],
        Some("Parent"),
    )));

    let _ = lower_classes(&mut program, &mut types, &mut ctx);

    let p = find_class(&program, "Parent");
    assert_eq!(p.fields, parent.fields);
    assert_eq!(p.methods, parent.methods);
    assert_eq!(p.extends, parent.extends);
}

#[test]
fn closer_ancestor_method_wins_over_grandparent_method() {
    let (mut program, mut types, mut ctx) = setup();
    let gp_with_m = class_with_fields_and_methods("GP", vec![], vec![("m", 1)], None);
    let p_overrides_m = class_with_fields_and_methods("P", vec![], vec![("m", 2)], Some("GP"));
    let c_no_methods = class_with_fields_and_methods("C", vec![], vec![], Some("P"));

    program.push_decl(HirDecl::Class(gp_with_m));
    program.push_decl(HirDecl::Class(p_overrides_m));
    program.push_decl(HirDecl::Class(c_no_methods));

    let stats = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "C");
    let m = c
        .methods
        .iter()
        .find(|m| m.name.as_str() == "m")
        .expect("m must exist");
    assert_eq!(m.params[0].ty, TypeId::from_raw(2), "P.m wins over GP.m");
    assert_eq!(c.methods.len(), 1, "only P.m survives; GP.m is shadowed");
    assert!(
        stats.overridden_methods >= 1,
        "GP.m shadowed by P must be counted as override"
    );
}

#[test]
fn grandchild_inherits_non_overridden_methods_from_each_ancestor() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "GP",
        vec![],
        vec![("g_only", 1), ("shared", 11)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P",
        vec![],
        vec![("p_only", 2), ("shared", 12)],
        Some("GP"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![],
        vec![],
        Some("P"),
    )));

    let _ = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "C");
    let names: Vec<String> = c
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert!(names.contains(&"g_only".to_owned()));
    assert!(names.contains(&"p_only".to_owned()));
    let shared_count = c
        .methods
        .iter()
        .filter(|m| m.name.as_str() == "shared")
        .count();
    assert_eq!(
        shared_count, 1,
        "shared must collapse to one entry (P.shared wins)"
    );
    let shared = c
        .methods
        .iter()
        .find(|m| m.name.as_str() == "shared")
        .unwrap();
    assert_eq!(
        shared.params[0].ty,
        TypeId::from_raw(12),
        "P.shared wins over GP.shared"
    );
    assert_eq!(c.methods.len(), 3);
}

#[test]
fn child_method_wins_over_closer_ancestor_method() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "GP",
        vec![],
        vec![("m", 1)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P",
        vec![],
        vec![],
        Some("GP"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![],
        vec![("m", 99)],
        Some("P"),
    )));

    let _ = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "C");
    assert_eq!(c.methods.len(), 1);
    let m = c.methods.iter().find(|m| m.name.as_str() == "m").unwrap();
    assert_eq!(
        m.params[0].ty,
        TypeId::from_raw(99),
        "own C.m wins over GP.m"
    );
}

#[test]
fn parent_methods_preserve_declaration_order_within_parent() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Parent",
        vec![],
        vec![("a", 1), ("b", 2), ("c", 3)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "Child",
        vec![],
        vec![],
        Some("Parent"),
    )));

    let _ = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "Child");
    let names: Vec<String> = c
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert_eq!(
        names,
        vec!["a", "b", "c"],
        "inherited methods stay in parent declaration order"
    );
}

#[test]
fn inherited_methods_keep_rootmost_first_order_across_three_levels() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "GP",
        vec![],
        vec![("g1", 1), ("g2", 2)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P",
        vec![],
        vec![("p1", 3), ("p2", 4)],
        Some("GP"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![],
        vec![("c1", 5), ("c2", 6)],
        Some("P"),
    )));

    let _ = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "C");
    let names: Vec<String> = c
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert_eq!(
        names,
        vec!["g1", "g2", "p1", "p2", "c1", "c2"],
        "gp block first, then p block, then own — each in declaration order"
    );
}

#[test]
fn inherited_methods_order_with_closest_ancestor_override() {
    let (mut program, mut types, mut ctx) = setup();
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "GP",
        vec![],
        vec![("a", 1), ("shared", 10), ("b", 2)],
        None,
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "P",
        vec![],
        vec![("shared", 20), ("c", 3)],
        Some("GP"),
    )));
    program.push_decl(HirDecl::Class(class_with_fields_and_methods(
        "C",
        vec![],
        vec![],
        Some("P"),
    )));

    let _ = lower_classes(&mut program, &mut types, &mut ctx);

    let c = find_class(&program, "C");
    let names: Vec<String> = c
        .methods
        .iter()
        .map(|m| m.name.as_str().to_owned())
        .collect();
    assert_eq!(
        names,
        vec!["a", "b", "shared", "c"],
        "GP block first (a, b) skipping GP.shared, then P block (shared=P.shared wins, c) — each block in declaration order"
    );
    let shared = c
        .methods
        .iter()
        .find(|m| m.name.as_str() == "shared")
        .unwrap();
    assert_eq!(
        shared.params[0].ty,
        TypeId::from_raw(20),
        "P.shared wins over GP.shared"
    );
}
