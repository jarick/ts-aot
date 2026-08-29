use ts_aot_frontend::FrontendPass;
use ts_aot_passes::{PassContext, convert_program};

fn convert(src: &str) -> (String, Vec<String>) {
    let mut types = ts_aot_core::TypeTable::new();
    let mut ctx = PassContext::new();
    let frontend = FrontendPass::new().run_with_types("test.ts", src, &mut types, false);
    let mut diags: Vec<String> = frontend
        .diagnostics
        .iter()
        .map(|d| format!("{:?}", d))
        .collect();
    if frontend.diagnostics.has_errors() {
        return (String::new(), diags);
    }
    let mut hir = frontend.program;
    ts_aot_passes::lower_enums(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::monomorphize(&mut hir, &mut types, &mut ctx);
    ts_aot_passes::lower_closures(&mut hir, &mut types, &mut ctx);
    let _ = ts_aot_passes::lower_async(&mut hir, &mut types, &mut ctx);
    let mir = convert_program(&hir, &mut types, &mut ctx);
    diags.extend(ctx.diagnostics().iter().map(|d| format!("{:?}", d)));
    (mir.dump_text(), diags)
}

#[test]
fn date_now_static_emits_runtime_call() {
    let (mir, diags) = convert("function f(): i64 { return Date.now(); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_now()"),
        "Date.now() must lower to runtime call __ts_aot_date_now, got:\r\n{mir}"
    );
}

#[test]
fn date_parse_static_emits_runtime_call() {
    let (mir, diags) =
        convert(r#"function f(): i64 { return Date.parse("2020-01-01T00:00:00Z"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_parse("),
        "Date.parse() must lower to runtime call __ts_aot_date_parse, got:\r\n{mir}"
    );
}

#[test]
fn new_date_no_args_emits_date_now() {
    let (mir, diags) = convert("function f(): Date { return new Date(); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_now()"),
        "new Date() with no args must lower to __ts_aot_date_now, got:\r\n{mir}"
    );
}

#[test]
fn new_date_with_ms_emits_new_from_ms() {
    let (mir, diags) = convert("function f(): Date { return new Date(0); }");
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_new_from_ms("),
        "new Date(0) must lower to __ts_aot_date_new_from_ms, got:\r\n{mir}"
    );
}

#[test]
fn new_date_with_string_emits_parse() {
    let (mir, diags) =
        convert(r#"function f(): Date { return new Date("2020-01-01T00:00:00Z"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_parse("),
        "new Date(string) must lower to __ts_aot_date_parse, got:\r\n{mir}"
    );
}

#[test]
fn new_date_with_string_variable_dispatches_to_parse_not_new_from_ms() {
    let (mir, diags) = convert(
        r#"
        function f(s: string): Date {
            return new Date(s);
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_parse("),
        "new Date(string-typed-local) must dispatch to DateParse (via HIR type), not DateNewFromMs, got:\r\n{mir}"
    );
    assert!(
        !mir.contains("date_new_from_ms("),
        "new Date(string-typed-local) must NOT dispatch to DateNewFromMs, got:\r\n{mir}"
    );
}

#[test]
fn new_date_with_number_variable_dispatches_to_new_from_ms_not_parse() {
    let (mir, diags) = convert(
        r#"
        function f(n: i64): Date {
            return new Date(n);
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_new_from_ms("),
        "new Date(number-typed-local) must dispatch to DateNewFromMs, not DateParse, got:\r\n{mir}"
    );
    assert!(
        !mir.contains("date_parse("),
        "new Date(number-typed-local) must NOT dispatch to DateParse, got:\r\n{mir}"
    );
}

#[test]
fn date_get_time_with_extra_args_emits_e0406_and_does_not_forward_args() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getTime(1, 2);
        }
        "#,
    );
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("getTime") && d.contains("extra argument"));
    assert!(
        has_e0406,
        "d.getTime(1, 2) must emit E0406 about extra arguments, got: {diags:?}"
    );
    assert!(
        !mir.contains("date_get_time("),
        "d.getTime(1, 2) must NOT emit any date_get_time runtime call (rejected before dispatch), got:\r\n{mir}"
    );
}

#[test]
fn date_to_iso_string_with_extra_arg_emits_e0406() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): string {
            return d.toISOString(123);
        }
        "#,
    );
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("toISOString"));
    assert!(
        has_e0406,
        "d.toISOString(123) must emit E0406 about extra arguments, got: {diags:?}"
    );
    assert!(
        !mir.contains("date_to_iso_string("),
        "d.toISOString(123) must NOT emit any date_to_iso_string runtime call, got:\r\n{mir}"
    );
}

#[test]
fn plain_i64_receiver_does_not_enter_date_dispatch() {
    let (mir, diags) = convert(
        r#"
        function f(n: i64): i64 {
            return n.getTime();
        }
        "#,
    );
    assert!(
        !mir.contains("date_get_time("),
        "plain i64.getTime() must NOT dispatch to __ts_aot_date_get_time (receiver is i64, not Date), got:\r\n{mir}"
    );
    assert!(
        !mir.contains("date_get_full_year(")
            && !mir.contains("date_value_of(")
            && !mir.contains("date_to_iso_string("),
        "plain i64 should not enter any Date runtime dispatch, got:\r\n{mir}"
    );
    let has_dispatch_failure = diags
        .iter()
        .any(|d| d.contains("P0012") || d.contains("P0005"));
    assert!(
        has_dispatch_failure,
        "plain i64.getTime() must produce a diagnostic (no Date runtime call AND no struct field access); got: {diags:?}"
    );
}

#[test]
fn date_get_time_instance_method_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getTime();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_time("),
        "d.getTime() must lower to __ts_aot_date_get_time, got:\r\n{mir}"
    );
}

#[test]
fn date_value_of_instance_method_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.valueOf();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_value_of("),
        "d.valueOf() must lower to __ts_aot_date_value_of, got:\r\n{mir}"
    );
}

#[test]
fn date_get_full_year_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getFullYear();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_full_year("),
        "d.getFullYear() must lower to __ts_aot_date_get_full_year, got:\r\n{mir}"
    );
}

#[test]
fn date_to_iso_string_dispatches_and_returns_string() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): string {
            return d.toISOString();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_to_iso_string("),
        "d.toISOString() must lower to __ts_aot_date_to_iso_string, got:\r\n{mir}"
    );
}

#[test]
fn date_get_month_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getMonth();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_month("),
        "d.getMonth() must lower to __ts_aot_date_get_month, got:\r\n{mir}"
    );
}

#[test]
fn date_get_date_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getDate();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_date("),
        "d.getDate() must lower to __ts_aot_date_get_date, got:\r\n{mir}"
    );
}

#[test]
fn date_get_hours_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getHours();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_hours("),
        "d.getHours() must lower to __ts_aot_date_get_hours, got:\r\n{mir}"
    );
}

#[test]
fn date_get_minutes_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getMinutes();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_minutes("),
        "d.getMinutes() must lower to __ts_aot_date_get_minutes, got:\r\n{mir}"
    );
}

#[test]
fn date_get_seconds_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getSeconds();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_seconds("),
        "d.getSeconds() must lower to __ts_aot_date_get_seconds, got:\r\n{mir}"
    );
}

#[test]
fn date_get_milliseconds_dispatches() {
    let (mir, diags) = convert(
        r#"
        function f(d: Date): i64 {
            return d.getMilliseconds();
        }
        "#,
    );
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_get_milliseconds("),
        "d.getMilliseconds() must lower to __ts_aot_date_get_milliseconds, got:\r\n{mir}"
    );
}

#[test]
fn date_parse_with_non_string_arg_emits_e0406_and_skips_runtime_call() {
    let (mir, diags) = convert("function f(): i64 { return Date.parse(123); }");
    assert!(
        !mir.contains("date_parse("),
        "Date.parse(123) must NOT lower to __ts_aot_date_parse (rejected before dispatch), got:\r\n{mir}"
    );
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("Date.parse"));
    assert!(
        has_e0406,
        "Date.parse(123) must emit E0406 error about non-string arg, got: {diags:?}"
    );
}

#[test]
fn date_parse_with_string_arg_emits_no_warning() {
    let (mir, diags) =
        convert(r#"function f(): i64 { return Date.parse("2020-01-01T00:00:00Z"); }"#);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert!(
        mir.contains("date_parse("),
        "Date.parse(string) must lower to __ts_aot_date_parse, got:\r\n{mir}"
    );
}

#[test]
fn date_now_with_extra_args_emits_e0406() {
    let (mir, diags) = convert("function f(): i64 { return Date.now(123); }");
    assert!(
        !diags.is_empty(),
        "expected E0406 diagnostic, got mir:\r\n{mir}"
    );
    let has_e0406 = diags.iter().any(|d| d.contains("E0406"));
    assert!(
        has_e0406,
        "diagnostic should be E0406 (Date.now requires 0 args), got: {diags:?}"
    );
}

#[test]
fn new_date_with_too_many_args_emits_e0406() {
    let (mir, diags) = convert("function f(): Date { return new Date(2020, 0, 1, 0, 0, 0, 0); }");
    assert!(
        !diags.is_empty(),
        "expected E0406 diagnostic, got mir:\r\n{mir}"
    );
    let has_e0406 = diags
        .iter()
        .any(|d| d.contains("E0406") && d.contains("Date"));
    assert!(
        has_e0406,
        "diagnostic should be E0406 (multi-arg Date ctor not yet supported), got: {diags:?}"
    );
}

#[test]
fn date_method_on_literal_int_receiver_with_no_typeid_does_not_dispatch() {
    let (mir, diags) = convert(
        r#"
        function f(): i64 {
            return (123).getTime();
        }
        "#,
    );
    assert!(
        !mir.contains("date_get_time("),
        "(123).getTime() must NOT dispatch to __ts_aot_date_get_time \
         (literal receiver has no TypeId, not Type::Date), got:\r\n{mir}"
    );
    assert!(
        !mir.contains("date_get_full_year(")
            && !mir.contains("date_value_of(")
            && !mir.contains("date_to_iso_string(")
            && !mir.contains("date_get_month(")
            && !mir.contains("date_get_date(")
            && !mir.contains("date_get_hours(")
            && !mir.contains("date_get_minutes(")
            && !mir.contains("date_get_seconds(")
            && !mir.contains("date_get_milliseconds("),
        "no Date prototype method should be dispatched on a literal receiver, got:\r\n{mir}"
    );
    let has_non_dispatch_diag = diags
        .iter()
        .any(|d| d.contains("E0406") || d.contains("P0012") || d.contains("P0005"));
    assert!(
        has_non_dispatch_diag,
        "literal receiver with no TypeId must surface a non-Date diagnostic, got: {diags:?}"
    );
}
