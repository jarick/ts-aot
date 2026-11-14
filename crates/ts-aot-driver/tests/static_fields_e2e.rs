//! Static class fields: class-level storage that `Class.field` reads and writes.
//!
//! A static field belongs to the class rather than to an instance, so it is not a
//! member of the instance struct: it lives in one module-level global
//! (`Counter.count` is `Counter__count_<type id>`), which every read and write goes through.

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

/// The crate stage (`--emit crate`) emits the lib-style module: exports are `pub`
/// and the module-init body is reachable as `__ts_aot_init`, which a string static
/// field needs (its global is seeded by the module init, not by the `static`).
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

/// A static field is stored in a module-level global, not in the instance struct,
/// and the value it holds is shared by every read and write.
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
    // `Counter.count` starts at its initializer, `bump()` writes the global (so the
    // next read sees it), and `ready` keeps the bool it was initialized with.
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

/// A string static field: a `static` cannot hold the arena-backed `JsString`, so the
/// literal seeds the global from the module-init body (`__ts_aot_init`) instead of
/// initializing the `static` directly - the route a module-level
/// `let s: string = "..."` already takes.
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

/// A generic class registers its static fields too: a static member may not
/// reference the class's type parameters, so the global that holds it does not
/// depend on them, even though the instance fields do.
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

/// A computed static initializer (`static total: number = compute()`) runs in the
/// module-init body, the way a module-level `let` does, instead of leaving the
/// global at its zero value.
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

/// A static field is reachable from a free function as well, and an instance of
/// the class carries no field for it: the two do not share storage.
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
    assert!(
        flat.contains("structBox{pubsize:f64,}"),
        "the instance struct keeps only its own fields; got:\n{flat}"
    );

    check_generated_crate("static_fields_instance", &rust);
    let stdout = run_generated_crate("static_fields_instance", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Two anonymous classes may both declare `static f`: their globals are identified by
/// the class's type, not by an (empty) class name, so the names cannot collide.
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
        flat.matches("static__f_").count(),
        2,
        "each anonymous class keeps its own static field global, named after its class \
         type; got:\n{flat}"
    );
    assert!(
        !flat.contains("E0503"),
        "the two globals must not collide; got:\n{flat}"
    );
}

/// A class expression evaluates to an instance of that class, so a variable holding it
/// can read the class's static fields (`var C = class { static f = 1 }; C.f`).
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
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_expression::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(
        !rust.contains("P0010"),
        "reading a static through a class value must not fall back to a placeholder \
         field; got:\n{rust}"
    );
    check_generated_crate("class_expression", &rust);
    let stdout = run_generated_crate("class_expression", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// Two class expressions may share a source name (`class Foo` in two places), so their
/// static-field globals are identified by the class's type, not by that name alone.
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
        flat.matches("staticFoo__s_").count(),
        2,
        "both class expressions keep their own static-field global; got:\n{flat}"
    );
    check_generated_crate("class_expression_names", &rust);
}

/// Evaluating a class expression does not run its constructor, so a constructor with
/// required parameters must not turn the expression into an arity error (`new C()` would).
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

/// A class expression whose constructor takes arguments is still a class value: the
/// instance is built from the class's fields *without* running the constructor (only
/// `new` runs it), so the crate compiles and the class's statics are readable.
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
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(class_expression_ctor::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(
        !rust.contains("P0004"),
        "evaluating a class expression passes no arguments to its constructor; got:\n{rust}"
    );
    check_generated_crate("class_expression_ctor", &rust);
    let stdout = run_generated_crate("class_expression_ctor", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

/// A module-level array is stored in the global as an owned `std::vec::Vec`: a `static`
/// needs a constant initializer and an arena `Vec` has none, so the value is converted at
/// the boundary - reads build an arena `Vec`, writes go through the owned place.
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

/// A module-level static field is initialized in the module-init body, in declaration order,
/// on the same path as a computed initializer: preinitializing the global would apply the
/// literal before the body runs, so an earlier initializer would already observe it.
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

/// An instance receiver stays on the instance-field path even when the class also declares a
/// static field of the same name: `class C { x = 1; static x = 2 }` is legal, so `c.x` is the
/// instance field and `C.x` is the module-level global.
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

/// Evaluating a class value must not run the class's field initializers: JavaScript runs those
/// when an instance is constructed with `new`, not when the class expression is evaluated.
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

/// A called generic method is specialized, emitted inside its class's `impl`, and the call is
/// retargeted to the specialization - both with an explicit type argument and with one
/// inferred through the receiver.
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

/// A class value reached through a function-local binding, or through the class expression
/// itself, still reads its statics from the module-level global; only a plain instance keeps
/// the instance-field path.
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

/// The push argument is evaluated before the push, and may itself mutate the global: the
/// write-back must keep those effects.
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

/// A generic method called through an optional chain (`k?.m(1)`) is specialized too: the
/// receiver is typed `K | null`, so the lookup sees through the option instead of leaving the
/// call on an unsubstituted generic.
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

/// A class may declare a field and a method with one name; `k.m` then names the field, so a
/// method call must not be answered by the method's specialization.
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

/// A class value is its class tag, so a binding may be rebound to another class and a static
/// read picks the class at run time.
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

/// A module-level `let` holding a class value is a tag too, so it can be rebound and a static
/// read through it selects the class at run time.
#[test]
fn a_module_level_class_value_rebound_to_another_class_reads_the_new_static() {
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
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    module_rebound::__ts_aot_init();
    assert_eq!(module_rebound::read(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = crate_rust_of(SOURCE);
    check_generated_crate("module_rebound", &rust);
    let stdout = run_generated_crate("module_rebound", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
