use ts_aot_runtime::TemplateStringsArray;

#[test]
fn index_returns_str_slice_not_string_clone() {
    let tpl = TemplateStringsArray::new(
        vec!["Hello, ".to_owned(), "!".to_owned()],
        vec!["Hello, ".to_owned(), "!".to_owned()],
    );
    let s: &str = &tpl[0];
    assert_eq!(s, "Hello, ");
    let s: &str = &tpl[1];
    assert_eq!(s, "!");
}

#[test]
fn index_works_with_str_methods_no_clone() {
    let tpl = TemplateStringsArray::new(
        vec!["foo".to_owned(), "bar".to_owned()],
        vec!["foo".to_owned(), "bar".to_owned()],
    );
    assert_eq!(tpl[0].len(), 3);
    assert_eq!(tpl[0].to_uppercase(), "FOO");
    assert_eq!(format!("{}{}", &tpl[0], &tpl[1]), "foobar");
}

#[test]
fn index_output_type_is_str() {
    let tpl = TemplateStringsArray::new(vec!["x".to_owned()], vec!["x".to_owned()]);
    let s: &str = &tpl[0];
    assert_eq!(s, "x");
    let s: &str = tpl[0].as_ref();
    assert_eq!(s, "x");
}

#[test]
fn len_works() {
    let tpl = TemplateStringsArray::new(vec![], vec![]);
    assert_eq!(tpl.len(), 0);
    assert!(tpl.is_empty());
    let tpl = TemplateStringsArray::new(
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
    );
    assert_eq!(tpl.len(), 3);
    assert!(!tpl.is_empty());
}

#[test]
fn debug_format_includes_cooked_and_raw() {
    let tpl = TemplateStringsArray::new(
        vec!["a".to_owned(), "b".to_owned()],
        vec!["A".to_owned(), "B".to_owned()],
    );
    let s = format!("{tpl:?}");
    assert!(s.contains("cooked"), "Debug should mention cooked: {s}");
    assert!(s.contains("raw"), "Debug should mention raw: {s}");
}
