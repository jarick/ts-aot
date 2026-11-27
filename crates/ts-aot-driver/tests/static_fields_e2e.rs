use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn compile_module(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    )
}

fn compile_script(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: false,
        },
    )
}

fn rust_of(source: &str) -> String {
    let out = compile_module(source);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn crate_rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Crate,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-crate must populate rust_source")
}

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

fn push_struct_field(segment: &str, names: &mut Vec<String>) {
    let segment = segment.trim();
    let segment = match segment.strip_prefix("pub") {
        Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim_start(),
        _ => segment,
    };
    if segment.is_empty() {
        return;
    }
    if let Some((name, _)) = segment.split_once(':') {
        names.push(name.trim().to_owned());
    }
}

fn struct_field_names(rust: &str, name: &str) -> Vec<String> {
    let marker = format!("struct {name} {{");
    let start = rust
        .find(&marker)
        .unwrap_or_else(|| panic!("no `{marker}` in generated code:\n{rust}"))
        + marker.len();
    let mut depth = 1i32;
    let mut end = rust.len();
    for (i, ch) in rust[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &rust[start..end];
    let mut names = Vec::new();
    let mut segment = 0usize;
    let mut angle = 0i32;
    let mut paren = 0i32;
    for (i, ch) in body.char_indices() {
        match ch {
            '<' => angle += 1,
            '>' => angle -= 1,
            '(' => paren += 1,
            ')' => paren -= 1,
            ',' if angle == 0 && paren == 0 => {
                push_struct_field(&body[segment..i], &mut names);
                segment = i + 1;
            }
            _ => {}
        }
    }
    push_struct_field(&body[segment..], &mut names);
    names
}

#[test]
fn a_static_field_lives_in_a_module_level_global() {
    const SOURCE: &str = r"
class Counter {
  static count: number = 0;
  static ready: boolean = false;
  static bump(): void { Counter.count = Counter.count + 1; }
  static get_(): number { return Counter.count; }
  static ready_(): boolean { return Counter.ready; }
}
export function twice(): number { Counter.bump(); Counter.bump(); return Counter.get_(); }
export function direct(): number { return Counter.count; }
export function ready(): boolean { return Counter.ready_(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(static_fields::direct(&ctx), 0.0);
    assert!(!static_fields::ready(&ctx));
    assert_eq!(static_fields::twice(&ctx), 2.0);
    assert_eq!(static_fields::direct(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("structCounter{}"),
        "a static field must not become an instance field; got:\n{flat}"
    );
    assert!(
        flat.contains("staticmutCounter__count_") && flat.contains(":f64=0.0"),
        "the static field is a module-level global; got:\n{flat}"
    );

    check_generated_crate("static_fields", &rust);
    let stdout = run_generated_crate("static_fields", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_string_static_field_is_seeded_by_the_module_init() {
    const SOURCE: &str = r#"
class K {
  static label: string = "hi";
  static is_hi(): boolean { return K.label === "hi"; }
  static len(): number { return K.label.length; }
}
export function label_is_hi(): boolean { return K.is_hi(); }
export function label_len(): number { return K.len(); }
"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    // The string global is seeded by the module-init body, not by the `static`.
    static_fields_string::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(static_fields_string::label_is_hi(&ctx));
    assert_eq!(static_fields_string::label_len(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("staticmutK__label"),
        "the string static field is a module-level global; got:\n{flat}"
    );
    check_generated_crate("static_fields_string", &rust);
    let stdout = run_generated_crate("static_fields_string", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_generic_class_registers_its_static_fields() {
    const SOURCE: &str = r"
class Counter<T> {
  static count: number = 0;
  static bump(): void { Counter.count = Counter.count + 1; }
  static get_(): number { return Counter.count; }
}
export function twice(): number { Counter.bump(); Counter.bump(); return Counter.get_(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(static_fields_generic::twice(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("staticmutCounter__count_") && flat.contains(":f64=0.0"),
        "a generic class's static field is a module-level global; got:\n{flat}"
    );
    check_generated_crate("static_fields_generic", &rust);
    let stdout = run_generated_crate("static_fields_generic", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_computed_static_initializer_runs_in_the_module_init() {
    const SOURCE: &str = r#"
function compute(): number { return 6; }
class Holder {
  static total: number = compute();
  static label: string = "t" + "otal";
  static total_(): number { return Holder.total; }
  static label_len(): number { return Holder.label.length; }
}
export function total(): number { return Holder.total_(); }
export function label_len(): number { return Holder.label_len(); }
"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    static_fields_computed::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(static_fields_computed::total(&ctx), 6.0);
    assert_eq!(static_fields_computed::label_len(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("static_fields_computed", &rust);
    let stdout = run_generated_crate("static_fields_computed", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_static_field_is_class_level_storage() {
    const SOURCE: &str = r"
class Box {
  static made: number = 0;
  size: number = 1;
  constructor() { Box.made = Box.made + 1; }
}
export function make_two(): number { const a = new Box(); const b = new Box(); return Box.made; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(static_fields_instance::make_two(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    let field_names = struct_field_names(&rust, "Box");
    assert_eq!(
        field_names,
        vec!["size".to_owned()],
        "the instance struct must hold exactly its own fields (`size`); got {field_names:?} in:\n{flat}"
    );
    assert!(
        !field_names.iter().any(|name| name == "made"),
        "the static field must not be stored on the instance struct; got {field_names:?} in:\n{flat}"
    );

    check_generated_crate("static_fields_instance", &rust);
    let stdout = run_generated_crate("static_fields_instance", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn two_anonymous_classes_do_not_collide_on_a_static_field() {
    const SOURCE: &str = r"
var a = class { static f = 1; };
var b = class { static f = 2; };
export function f(): number { return 0; }
";
    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    assert_eq!(
        flat.matches("staticmut__f_").count(),
        2,
        "each anonymous class keeps its own static field global, named after its class \
         type; got:\n{flat}"
    );
    assert!(
        !flat.contains("E0503"),
        "the two globals must not collide; got:\n{flat}"
    );
}

#[test]
fn a_class_expression_value_reads_its_static_fields() {
    const SOURCE: &str = r"
var C = class { static f = 1; };
export function got(): number { return C.f; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    class_expression::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_expression::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    assert!(
        !rust.contains("P0010"),
        "reading a static through a class value must not fall back to a placeholder \
         field; got:\n{rust}"
    );
    check_generated_crate("class_expression", &rust);
    let stdout = run_generated_crate("class_expression", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn class_expressions_sharing_a_source_name_do_not_collide() {
    const SOURCE: &str = r"
var A = class Foo { static s = 1; };
var B = class Foo { static s = 2; };
export function f(): number { return 0; }
";
    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    assert_eq!(
        flat.matches("staticmutFoo__s_").count(),
        2,
        "both class expressions keep their own static-field global; got:\n{flat}"
    );
    check_generated_crate("class_expression_names", &rust);
}

#[test]
fn a_class_expression_with_a_required_constructor_argument_is_not_an_arity_error() {
    const SOURCE: &str = r"
var C = class { constructor(x: number) { } };
export function f(): number { return 0; }
";
    let rust = rust_of(SOURCE);
    assert!(
        !rust.contains("P0004"),
        "evaluating a class expression does not call its constructor; got:\n{rust}"
    );
}

#[test]
fn a_class_expression_with_a_constructor_yields_a_usable_value() {
    const SOURCE: &str = r"
var C = class { v: number = 5; constructor(x: number) { this.v = x; } static s = 2; };
export function got(): number { return C.s; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    class_expression_ctor::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_expression_ctor::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    assert!(
        !rust.contains("P0004"),
        "evaluating a class expression passes no arguments to its constructor; got:\n{rust}"
    );
    check_generated_crate("class_expression_ctor", &rust);
    let stdout = run_generated_crate("class_expression_ctor", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_array_global_round_trips() {
    const SOURCE: &str = r"
let arr: number[] = [1.5, 2.5];
export function f(): number { arr[0] = 7; return arr[0]; }
export function pushed(): number { arr.push(3.5); return arr.length; }
export function at(i: number): number { return arr[i]; }
export function store(i: number): number { arr[i] = 9.25; return arr[i]; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    module_array_global::__ts_aot_init();
    assert_eq!(module_array_global::f(&ctx), 7.0);
    // `push` copies the owned global into the arena, pushes, and writes the elements back.
    assert_eq!(module_array_global::pushed(&ctx), 3.0);
    // An index read borrows the global instead of copying it into the arena.
    assert_eq!(module_array_global::at(&ctx, 2.0), 3.5);
    assert_eq!(module_array_global::store(&ctx, 1.0), 9.25);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("staticmutarr:::std::vec::Vec<f64>=::std::vec::Vec::new();"),
        "the array global must be stored owned; got:\n{flat}"
    );
    check_generated_crate("module_array_global", &rust);
    let stdout = run_generated_crate("module_array_global", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_literal_static_field_is_initialized_in_declaration_order() {
    const SOURCE: &str = r"
function peek(): number { return C.b; }
class C {
  static a: number = peek();
  static b: number = 7;
}
export function order(): number { return C.a; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    static_fields_order::__ts_aot_init();
    // `b` is declared after `a`, so `a` must observe its default: preinitializing the
    assert_eq!(static_fields_order::order(&ctx), 0.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("static_fields_order", &rust);
    let stdout = run_generated_crate("static_fields_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_instance_field_wins_over_a_same_named_static_field() {
    const SOURCE: &str = r"
class C { x: number = 1; static x: number = 2; }
export function both(): number { const c = new C(); return c.x * 10 + C.x; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    static_fields_shadow::__ts_aot_init();
    // 1 from the instance field, 2 from the static global.
    assert_eq!(static_fields_shadow::both(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("static_fields_shadow", &rust);
    let stdout = run_generated_crate("static_fields_shadow", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_expression_does_not_run_its_field_initializers() {
    const SOURCE: &str = r"
let hits = 0;
function bump(): number { hits = hits + 1; return hits; }
const C = class { x: number = bump(); };
export function after(): number { return hits; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_no_init::after(&ctx), 0.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_no_init", &rust);
    let stdout = run_generated_crate("class_value_no_init", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_generic_method_call_is_specialized() {
    const SOURCE: &str = r"
class K { m<T>(x: T): T { return x; } }
export function explicit(): number { const k = new K(); return k.m<number>(3); }
export function inferred(): number { const k = new K(); return k.m(4); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(generic_method::explicit(&ctx), 3.0);
    assert_eq!(generic_method::inferred(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("generic_method", &rust);
    let stdout = run_generated_crate("generic_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
#[test]
fn an_unspecialized_generic_method_does_not_break_the_generated_crate() {
    const SOURCE: &str = r"
class K { m<T>(x: T): T { return x; } }
export function got(): number { const k = new K(); return 1; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(generic_method::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("generic_method", &rust);
    let stdout = run_generated_crate("generic_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_static_resolves_through_a_local_and_a_literal_receiver() {
    const SOURCE: &str = r"
export function via_local(): number { const L = class { static k: number = 4; }; return L.k; }
export function via_literal(): number { return (class { static k: number = 5; }).k; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_statics::via_local(&ctx), 4.0);
    assert_eq!(class_value_statics::via_literal(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_statics", &rust);
    let stdout = run_generated_crate("class_value_statics", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_effectful_push_argument_keeps_the_nested_push() {
    const SOURCE: &str = r"
let xs: number[] = [1.5];
function grow(): number { xs.push(2.5); return 3.5; }
export function both(): number { xs.push(grow()); return xs.length; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    effectful_push::__ts_aot_init();
    // 1.5 from the initializer, 2.5 from `grow`, 3.5 from the outer push.
    assert_eq!(effectful_push::both(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("effectful_push", &rust);
    let stdout = run_generated_crate("effectful_push", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_optional_chain_generic_method_call_is_specialized() {
    const SOURCE: &str = r"
class K { m<T>(x: T): T { return x; } }
export function through(): number { const k = new K(); return k?.m(5) ?? 0; }
export function maybe(k: K | null): number { return k?.m(6) ?? 1; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_chain_generic::through(&ctx), 5.0);
    assert_eq!(optional_chain_generic::maybe(&ctx, None), 1.0);
    assert_eq!(optional_chain_generic::maybe(&ctx, Some(optional_chain_generic::K {})), 6.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    assert!(!rust.contains("__generic"), "got:\n{rust}");
    check_generated_crate("optional_chain_generic", &rust);
    let stdout = run_generated_crate("optional_chain_generic", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_field_shadows_a_same_named_generic_method() {
    const SOURCE: &str = r"
class K { m: number = 3; m<T>(x: T): T { return x; } }
export function field(): number { return new K().m; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(shadowed_method::field(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("shadowed_method", &rust);
    let stdout = run_generated_crate("shadowed_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_rebound_to_another_class_reads_the_new_static() {
    const SOURCE: &str = r"
export function rebound(): number {
    let L = class { static k: number = 1; };
    const before = L.k;
    L = class { static k: number = 2; };
    return before * 10 + L.k;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(rebound_static::rebound(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("rebound_static", &rust);
    let stdout = run_generated_crate("rebound_static", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_let_class_value_read_selects_the_final_class() {
    const SOURCE: &str = r"
let C = class { static k: number = 1; };
C = class { static k: number = 2; };
export function read(): number { return C.k; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    module_let_class_value::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(module_let_class_value::read(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("module_let_class_value", &rust);
    let stdout = run_generated_crate("module_let_class_value", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_let_class_value_read_walked_before_a_reassignment_is_refused() {
    const SOURCE: &str = r"
let C = class { static k: number = 1; };
export function read(): number { return C.k; }
C = class { static k: number = 2; };
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a read whose candidate set grows later must be reported, got {codes:?}"
    );
}

#[test]
fn a_class_value_captured_by_a_closure_reads_its_static() {
    const SOURCE: &str = r"
export function got(): number {
    const L = class { static k: number = 1; };
    const read = () => L.k;
    return read();
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(captured_class_value::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("captured_class_value", &rust);
    let stdout = run_generated_crate("captured_class_value", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_captured_by_a_function_expression_reads_its_static() {
    const SOURCE: &str = r"
export function got(): number {
    const L = class { static k: number = 4; };
    const read = function (): number { return L.k; };
    return read();
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(captured_class_value_fn::got(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("captured_class_value_fn", &rust);
    let stdout = run_generated_crate("captured_class_value_fn", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_captured_by_a_closure_sees_a_rebinding_before_the_closure() {
    const SOURCE: &str = r"
export function got(): number {
    let L = class { static k: number = 1; };
    L = class { static k: number = 2; };
    const read = () => L.k;
    return read();
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(captured_class_value_rebound::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("captured_class_value_rebound", &rust);
    let stdout = run_generated_crate("captured_class_value_rebound", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_captured_by_a_closure_rebound_afterwards_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
    let L = class { static k: number = 1; };
    const read = () => L.k;
    L = class { static k: number = 2; };
    return read();
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a captured class value rebound after the closure must be reported, got {codes:?}"
    );
}

#[test]
fn reassigning_a_class_value_binding_to_a_plain_value_is_reported() {
    const SOURCE: &str = r"
export function got(): number {
    let L = class { static k: number = 1; };
    L = 5;
    return L.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "reading a static off a binding that no longer holds a class value must be reported, \
         got {codes:?}"
    );
    let message = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0419")
        .map(|d| d.message.clone())
        .expect("expected an E0419 diagnostic");
    assert!(
        message.contains("not one"),
        "the diagnostic must say the binding no longer holds a class value, got: {message}"
    );
}

#[test]
fn reassigning_a_class_value_binding_back_to_a_class_is_not_reported() {
    const SOURCE: &str = r"
export function got(): number {
    let L = class { static k: number = 1; };
    L = 5;
    L = class { static k: number = 2; };
    return L.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        !codes.contains(&"E0419"),
        "a binding that holds a class value again must not be reported, got {codes:?}"
    );
}

#[test]
fn a_conditional_class_assignment_after_a_plain_value_is_refused() {
    const SOURCE: &str = r"
export function got(flag: boolean): number {
    let C = class { static k: number = 1; };
    C = 5;
    if (flag) { C = class { static k: number = 2; }; }
    return C.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a branch that may leave the binding a plain value must be reported, got {codes:?}"
    );
}

#[test]
fn a_new_on_a_module_var_class_value_is_refused() {
    const SOURCE: &str = r"
var C = class { static k: number = 1; };
export function make(): C { return new C(); }
C = class { static k: number = 2; };
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "`new` on a reassignable class value must be reported, got {codes:?}"
    );
}

#[test]
fn a_class_value_is_truthy_under_logical_not() {
    const SOURCE: &str = r"
export function not_class(): boolean {
    const C = class { static k: number = 1; };
    return !C;
}
export function not_not_class(): boolean {
    const C = class { static k: number = 1; };
    return !!C;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(!class_value_truthy::not_class(&ctx));
    assert!(class_value_truthy::not_not_class(&ctx));
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_truthy", &rust);
    let stdout = run_generated_crate("class_value_truthy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_logical_assignment_after_a_plain_value_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
    let C = class { static k: number = 1; };
    C = 5;
    C ||= class { static k: number = 2; };
    return C.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a logical assignment keeps the old value when it is truthy, so the binding may still \
         hold a plain value; got {codes:?}"
    );
}

#[test]
fn a_logical_not_on_a_maybe_class_value_is_refused() {
    const SOURCE: &str = r"
let C = class { static k: number = 1; };
C = 0;
export function got(): boolean { return !C; }
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a class value is truthy while a plain zero is not, so `!` must not fold to `false`; got \
         {codes:?}"
    );
}

#[test]
fn a_conditional_assignment_inside_an_optional_chain_is_refused() {
    const SOURCE: &str = r"
class Sink { take(x: number): number { return x; } }
export function got(s: Sink | null): number {
    let C = class { static k: number = 1; };
    C = 5;
    s?.take((C = class { static k: number = 2; }, 0));
    return C.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "the argument of an optional call is skipped when the receiver is nullish, so the \
         binding may still hold a plain value; got {codes:?}"
    );
}

#[test]
fn a_computed_static_field_key_is_reported() {
    const SOURCE: &str = r#"
const key = "k";
class C { static [key]: number = 1; }
export function got(): number { return C.k; }
"#;
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec!["E0300"],
        "a computed static field key must be reported once, got {codes:?}"
    );
}

#[test]
fn a_computed_instance_field_key_is_reported() {
    const SOURCE: &str = r#"
const key = "k";
class C { [key]: number = 1; }
export function got(): number { const c = new C(); return 1; }
"#;
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec!["E0300"],
        "a computed instance field key must be reported once, got {codes:?}"
    );
}

#[test]
fn a_literal_computed_field_key_is_lowered() {
    const SOURCE: &str = r#"
class A { static ["k"]: number = 1; }
class B { ["j"]: number = 2; }
export function got(): number { const b = new B(); return A.k; }
"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    literal_computed_key::__ts_aot_init();
    assert_eq!(literal_computed_key::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("literal_computed_key", &rust);
    let stdout = run_generated_crate("literal_computed_key", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_field_poisoned_through_one_instance_is_not_recovered_through_another() {
    let out = compile_module(
        "class Holder { C = class { static k: number = 1; }; } \
         export function got(): number { \
             const h1 = new Holder(); \
             const h2 = new Holder(); \
             h1.C = 5; \
             h2.C = class { static k: number = 2; }; \
             return h1.C.k; \
         }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0419"),
        "a non-class write to one instance's field must keep the whole type poisoned, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_field_poisoned_and_restored_through_the_same_instance_is_supported() {
    const SOURCE: &str = r"
class Holder { C = class { static k: number = 1; }; }
export function got(): number {
    const h = new Holder();
    h.C = 5;
    h.C = class { static k: number = 2; };
    return h.C.k;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(field_restored::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("field_restored", &rust);
    let stdout = run_generated_crate("field_restored", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_concrete_static_method_of_a_generic_class_is_still_emitted() {
    const SOURCE: &str = r"
class Box<T> { value: T; static make(): number { return 7; } }
export function got(): number { return Box.make(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(generic_static_method::got(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    assert!(
        rust.contains("fn Box__make"),
        "the concrete static method must be emitted even though the generic instance storage is \
         omitted; got:\n{rust}"
    );
    check_generated_crate("generic_static_method", &rust);
    let stdout = run_generated_crate("generic_static_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_field_poisoned_in_one_function_stays_poisoned_for_another_functions_binding() {
    let out = compile_module(
        "class Holder { C = class { static k: number = 1; }; } \
         function a(): Holder { const x = new Holder(); x.C = 5; return x; } \
         function b(h: Holder): number { h.C = class { static k: number = 2; }; return 0; } \
         function c(h: Holder): number { return h.C.k; } \
         export function got(): number { const p = a(); b(new Holder()); return c(p); }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0419"),
        "a field poisoned through one function's binding must not be un-poisoned by a class write \
         to a same-numbered binding of another function, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_field_restored_from_a_local_class_value_is_supported() {
    const SOURCE: &str = r"
class Holder { C = class { static k: number = 1; }; }
export function got(): number {
    const h = new Holder();
    const K = class { static k: number = 2; };
    h.C = 5;
    h.C = K;
    return h.C.k;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(field_restored_from_local::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("field_restored_from_local", &rust);
    let stdout = run_generated_crate("field_restored_from_local", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_that_copies_a_poisoned_class_value_stays_uncertain() {
    let out = compile_module(
        "export function got(): number { \
             let c = class { static k: number = 1; }; \
             c = 7; \
             const x = c; \
             return x.k; \
         }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0419"),
        "a declaration that copies a poisoned class value must stay uncertain, got {:?}",
        out.diagnostics
    );
}

#[test]
fn an_assignment_that_copies_a_poisoned_class_value_stays_uncertain() {
    let out = compile_module(
        "export function got(): number { \
             let c = class { static k: number = 1; }; \
             let x = class { static j: number = 2; }; \
             c = 7; \
             x = c; \
             return x.k; \
         }",
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "E0419"),
        "an assignment that copies a poisoned class value must stay uncertain, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_local_that_copies_a_known_class_value_keeps_its_static_read() {
    const SOURCE: &str = "
export function got(): number { const c = class { static k: number = 1; }; const x = c; return x.k; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(known_copy::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("known_copy", &rust);
    let stdout = run_generated_crate("known_copy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_instance_calls_its_generic_method_when_a_static_shares_the_name() {
    const SOURCE: &str = r"
class C {
    m<T>(x: T): number { return 1; }
    static m<T>(x: T): number { return 2; }
}
let g = new C();
export function got(): number { return g.m(0); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    receiver_global::__ts_aot_init();
    assert_eq!(receiver_global::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("receiver_global", &rust);
    let stdout = run_generated_crate("receiver_global", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_in_an_instance_field_reads_its_static() {
    const SOURCE: &str = r"
class Holder {
    C = class { static k: number = 1; };
}
export function got(): number {
    const h = new Holder();
    return h.C.k;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_field::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_field", &rust);
    let stdout = run_generated_crate("class_value_field", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_in_an_instance_field_reads_its_static_through_this() {
    const SOURCE: &str = r"
class Holder {
    C = class { static k: number = 3; };
    read(): number { return this.C.k; }
}
export function got(): number { return new Holder().read(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_field_this::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_field_this", &rust);
    let stdout = run_generated_crate("class_value_field_this", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_field_rebound_to_another_class_reads_the_new_static() {
    const SOURCE: &str = r"
class Holder {
    C = class { static k: number = 1; };
    swap(): void { this.C = class { static k: number = 2; }; }
}
export function got(): number {
    const h = new Holder();
    h.swap();
    return h.C.k;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_field_rebound::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_field_rebound", &rust);
    let stdout = run_generated_crate("class_value_field_rebound", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_field_reassigned_a_plain_value_is_reported() {
    const SOURCE: &str = r"
class Holder {
    C = class { static k: number = 1; };
    swap(): void { this.C = 5; }
}
export function got(): number {
    const h = new Holder();
    h.swap();
    return h.C.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "reading a static off a field that no longer holds a class value must be reported, \
         got {codes:?}"
    );
}

#[test]
fn a_class_value_in_an_instance_field_reads_its_static_before_the_class_declaration() {
    const SOURCE: &str = r"
export function got(): number {
    const h = new Holder();
    return h.C.k;
}
class Holder {
    C = class { static k: number = 5; };
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_field_forward::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_field_forward", &rust);
    let stdout = run_generated_crate("class_value_field_forward", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_in_an_annotated_field_is_refused() {
    const SOURCE: &str = r"
class Holder { C: string = class { static k: number = 1; }; }
export function got(): number {
    const h = new Holder();
    return h.C.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a class value in a field with a non-tag annotation must be reported, got {codes:?}"
    );
}

#[test]
fn a_class_value_field_walked_before_a_later_rebinding_is_refused() {
    const SOURCE: &str = r"
export function got(h: Holder): number { return h.C.k; }
class Holder {
    C = class { static k: number = 1; };
    swap(): void { this.C = class { static k: number = 2; }; }
}
export function run(): number {
    const h = new Holder();
    h.swap();
    return got(h);
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a field read whose class set grows later must be reported, got {codes:?}"
    );
}

#[test]
fn a_class_value_field_walked_before_a_later_plain_assignment_is_refused() {
    const SOURCE: &str = r"
export function got(h: Holder): number { return h.C.k; }
class Holder {
    C = class { static k: number = 1; };
    swap(): void { this.C = 5; }
}
export function run(): number {
    const h = new Holder();
    h.swap();
    return got(h);
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a field read after the field stops holding a class value must be reported, got {codes:?}"
    );
}

#[test]
fn a_class_value_read_inside_a_loop_reassigned_in_the_body_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
    let L = class { static k: number = 1; };
    let total = 0;
    for (let i = 0; i < 2; i = i + 1) {
        total = total + L.k;
        L = class { static k: number = 2; };
    }
    return total;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a class-value read inside a loop that rebinds it must be reported, got {codes:?}"
    );
}

#[test]
fn a_class_value_read_inside_a_loop_that_keeps_the_binding_is_supported() {
    const SOURCE: &str = r"
export function got(): number {
    const L = class { static k: number = 3; };
    let total = 0;
    for (let i = 0; i < 2; i = i + 1) {
        total = total + L.k;
    }
    return total;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_loop::got(&ctx), 6.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_loop", &rust);
    let stdout = run_generated_crate("class_value_loop", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_level_class_value_copied_from_a_reassignable_binding_is_refused() {
    const SOURCE: &str = r"
let C = class { static k: number = 1; };
let D = class { static k: number = 2; };
export function setC(): void { C = D; }
D = class { static k: number = 3; };
export function read(): number { return C.k; }
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a copy of a reassignable class value must be reported, got {codes:?}"
    );
}

#[test]
fn a_module_level_var_class_value_read_before_a_reassignment_is_refused() {
    const SOURCE: &str = r"
var C = class { static k: number = 1; };
export function read(): number { return C.k; }
C = class { static k: number = 9; };
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a `var` read whose set grows later must be reported, got {codes:?}"
    );
}

#[test]
fn assigning_a_class_value_captured_by_a_closure_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
    let C = class { static k: number = 1; };
    const g = () => { C = class { static k: number = 2; }; };
    g();
    return C.k;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "writing a captured class value from a closure must be reported, got {codes:?}"
    );
}

#[test]
fn a_class_value_copied_inside_a_loop_from_a_rebound_binding_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
    let L = class { static k: number = 1; };
    let total = 0;
    for (let i = 0; i < 2; i = i + 1) {
        const M = L;
        total = total + M.k;
        L = class { static k: number = 2; };
    }
    return total;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a copy of a class value whose source is rebound inside the loop must be reported, \
         got {codes:?}"
    );
}

#[test]
fn a_class_value_copied_inside_a_loop_from_a_stable_binding_is_supported() {
    const SOURCE: &str = r"
export function got(): number {
    const L = class { static k: number = 3; };
    let total = 0;
    for (let i = 0; i < 2; i = i + 1) {
        const M = L;
        total = total + M.k;
    }
    return total;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_loop_copy::got(&ctx), 6.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_loop_copy", &rust);
    let stdout = run_generated_crate("class_value_loop_copy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_captured_local_shadowing_a_module_class_reads_its_own_static() {
    const SOURCE: &str = r"
class L { static k: number = 1; }
export function got(): number {
    let L = class { static k: number = 2; };
    const f = () => L.k;
    return f();
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(captured_shadowed_class::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("captured_shadowed_class", &rust);
    let stdout = run_generated_crate("captured_shadowed_class", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_captured_local_shadowing_a_module_class_reads_its_own_field() {
    const SOURCE: &str = r"
class L { static k: number = 1; }
class Wrapper { k: number = 9; }
export function got(): number {
    const L = new Wrapper();
    const f = () => L.k;
    return f();
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(captured_shadowed_field::got(&ctx), 9.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("captured_shadowed_field", &rust);
    let stdout = run_generated_crate("captured_shadowed_field", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_read_in_a_nested_loop_reassigned_in_the_outer_body_is_refused() {
    const SOURCE: &str = r"
export function got(): number {
    let C = class { static k: number = 1; };
    let total = 0;
    for (let i = 0; i < 2; i = i + 1) {
        for (let j = 0; j < 2; j = j + 1) {
            total = total + C.k;
        }
        C = class { static k: number = 2; };
    }
    return total;
}
";
    let out = compile_module(SOURCE);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0419"),
        "a nested-loop read whose binding is reassigned in the outer body must be reported, \
         got {codes:?}"
    );
}

#[test]
fn a_class_value_read_in_a_nested_loop_that_keeps_the_binding_is_supported() {
    const SOURCE: &str = r"
export function got(): number {
    const C = class { static k: number = 3; };
    let total = 0;
    for (let i = 0; i < 2; i = i + 1) {
        for (let j = 0; j < 2; j = j + 1) {
            total = total + C.k;
        }
    }
    return total;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_nested_loop::got(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_nested_loop", &rust);
    let stdout = run_generated_crate("class_value_nested_loop", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_field_written_through_a_computed_property_reads_the_new_static() {
    const SOURCE: &str = r#"
class Holder {
    C = class { static k: number = 1; };
}
export function got(): number {
    const h = new Holder();
    h["C"] = class { static k: number = 2; };
    return h.C.k;
}
"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_computed_write::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_computed_write", &rust);
    let stdout = run_generated_crate("class_value_computed_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_plain_field_written_through_a_computed_property_stays_supported() {
    const SOURCE: &str = r#"
class Holder {
    n: number = 1;
}
export function got(): number {
    const h = new Holder();
    h["n"] = 7;
    return h.n;
}
"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(computed_field_write::got(&ctx), 7.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("computed_field_write", &rust);
    let stdout = run_generated_crate("computed_field_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_value_field_assigned_its_own_value_stays_supported() {
    const SOURCE: &str = r"
class Holder {
    C = class { static k: number = 1; };
}
const D = class { static k: number = 4; };
export function got(): number {
    const h = new Holder();
    h.C = h.C;
    h.C = D;
    return h.C.k;
}
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    class_value_field_assign::__ts_aot_init();
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_value_field_assign::got(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("class_value_field_assign", &rust);
    let stdout = run_generated_crate("class_value_field_assign", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn negating_a_class_value_does_not_need_to_know_its_class() {
    const SOURCE: &str = "let L = class { static k: number = 1; }; \
         export function notL(): boolean { return !L; } \
         L = class { static k: number = 2; }; \
         class Holder { C = class { static k: number = 1; }; } \
         export function notField(): boolean { const h = new Holder(); return !h.C; } \
         export function setField(): void { const h = new Holder(); h.C = class { static k: number = 2; }; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(not_class_values::notL(&ctx), false);
    not_class_values::setField(&ctx);
    assert_eq!(not_class_values::notField(&ctx), false);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("not_class_values", &rust);
    let stdout = run_generated_crate("not_class_values", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_lazy_struct_global_reads_its_default_before_module_init() {
    const SOURCE: &str = "class C { x: number = 3; } let g: C = new C(); \
         export function got(): number { return g.x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(lazy_before_init::got(&ctx), 0.0);
    lazy_before_init::__ts_aot_init();
    assert_eq!(lazy_before_init::got(&ctx), 3.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains(
            "get_or_insert_with(||C{x:::std::rc::Rc::new(::core::cell::RefCell::new(0.0))})"
        ),
        "a struct-typed global without a static initializer must lazily build the default instance \
         through `get_or_insert_with`; got:\n{flat}"
    );
    check_generated_crate("lazy_before_init", &rust);
    let stdout = run_generated_crate("lazy_before_init", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_struct_global_initialized_from_another_struct_global_runs_in_the_module_init() {
    const SOURCE: &str = "class C { x: number = 0; } \
         let a: C = new C(); a.x = 5; let b: C = a; \
         export function got(): number { return b.x; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(struct_global_copy::got(&ctx), 0.0);
    struct_global_copy::__ts_aot_init();
    assert_eq!(struct_global_copy::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("struct_global_copy", &rust);
    let stdout = run_generated_crate("struct_global_copy", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

fn assert_struct_global_init_is_rejected(out: &ts_aot_driver::DriverOutput, expected: &str) {
    assert!(
        out.has_errors(),
        "a struct global whose initializer cannot be its lazy default must be reported; got {:?}",
        out.diagnostics
    );
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec!["E0300"],
        "the unrepresentable struct-global default must be the only diagnostic; got {codes:?}"
    );
    let message = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "E0300")
        .map(|d| d.message.clone())
        .expect("expected an E0300 diagnostic");
    assert!(
        message.contains("struct-typed global") && message.contains(expected),
        "the diagnostic must name the initializer shape, got: {message}"
    );
    assert!(
        message.contains("| null") && message.contains("__tla_main"),
        "the diagnostic must tell the user to declare the global as `<struct> | null` or to \
         initialize it in the module body; got: {message}"
    );
    assert!(
        out.rust_source.is_none(),
        "a rejected struct-global default must not emit Rust source"
    );
}

#[test]
fn a_null_initialized_struct_global_is_rejected() {
    const SOURCE: &str = "class C { x: number = 0; } let g: C = null; \
         export function got(): number { return g.x; }";
    let out = compile_module(SOURCE);
    assert_struct_global_init_is_rejected(&out, "`null`");
}

#[test]
fn an_initialized_struct_global_with_a_number_const_init_is_rejected() {
    const SOURCE: &str = "class C { x: number = 0; } let g: C = 7; \
         export function got(): number { return g.x; }";
    let out = compile_module(SOURCE);
    assert_struct_global_init_is_rejected(&out, "a number");
}

#[test]
fn an_initialized_struct_global_with_a_boolean_const_init_is_rejected() {
    const SOURCE: &str = "class C { x: number = 0; } let g: C = true; \
         export function got(): number { return g.x; }";
    let out = compile_module(SOURCE);
    assert_struct_global_init_is_rejected(&out, "a boolean");
}

#[test]
fn a_script_mode_static_field_with_a_number_const_init_is_rejected() {
    const SOURCE: &str = "class C { x: number = 0; static c: C = 7; } \
         function got(): number { return C.c.x; }";
    let out = compile_script(SOURCE);
    assert_struct_global_init_is_rejected(&out, "a number");
}

#[test]
fn an_optional_struct_global_keeps_its_own_option_representation() {
    const SOURCE: &str = "class C { x: number = 5; } let g: C | null = null; \
         export function set(): void { g = new C(); } \
         export function isNull(): boolean { return g === null; } \
         export function readOr(v: number): number { return g?.x ?? v; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(optional_struct_global::isNull(&ctx), true);
    assert_eq!(optional_struct_global::readOr(&ctx, 9.0), 9.0);
    optional_struct_global::set(&ctx);
    assert_eq!(optional_struct_global::isNull(&ctx), false);
    assert_eq!(optional_struct_global::readOr(&ctx, 9.0), 5.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("staticmutg:Option<C>=::core::option::Option::None"),
        "a `C | null` global keeps its own `Option` storage; got:\n{flat}"
    );
    assert!(
        flat.contains("unsafe{g.clone()}")
            && flat.contains("unsafe{g=::core::option::Option::Some("),
        "a `C | null` global keeps its direct `Option` reads and writes; got:\n{flat}"
    );
    assert!(
        !flat.contains("get_or_insert_with"),
        "a `C | null` global must not go through the lazy struct accessor; got:\n{flat}"
    );
    check_generated_crate("optional_struct_global", &rust);
    let stdout = run_generated_crate("optional_struct_global", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_static_generic_method_is_specialized_and_runs() {
    const SOURCE: &str = r"
class C { static s<T>(x: T): T { return x; } }
export function got(): number { return C.s(5); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(static_generic_method::got(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("fnC__s_mono"),
        "a call to a static generic method must be rewritten to a specialization that is \
         emitted, not left pointing at an undefined generic name; got:\n{flat}"
    );
    check_generated_crate("static_generic_method", &rust);
    let stdout = run_generated_crate("static_generic_method", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_static_and_an_instance_generic_method_share_the_name() {
    const SOURCE: &str = r"
class C {
  static m<T>(x: T): number { return 2; }
  m<T>(x: T): number { return 4; }
}
export function via_static(): number { return C.m(0); }
export function via_instance(): number { const c = new C(); return c.m(0); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(shared_method_name::via_static(&ctx), 2.0);
    assert_eq!(shared_method_name::via_instance(&ctx), 4.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("shared_method_name", &rust);
    let stdout = run_generated_crate("shared_method_name", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn two_static_generic_methods_are_specialized_independently() {
    const SOURCE: &str = r"
class C {
  static a<T>(x: T, y: T): T { return x; }
  static b<T>(x: T, y: T): T { return y; }
}
export function got(): number { return C.a(3, 30) * 100 + C.b(4, 400); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(two_static_generics::got(&ctx), 700.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    let flat = normalized(&rust);
    assert!(
        flat.contains("fnC__a_mono") && flat.contains("fnC__b_mono"),
        "both static generic methods must get their own specialization; got:\n{flat}"
    );
    check_generated_crate("two_static_generics", &rust);
    let stdout = run_generated_crate("two_static_generics", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
