use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

mod common;

#[test]
fn an_array_destructuring_assignment_writes_the_existing_bindings() {
    const SOURCE: &str = r"
export function optionalAbsent(): number { let x: number | undefined = 5; const arr: (number | undefined)[] = [undefined]; [x] = arr; return x === undefined ? 100 : 7; }
export function numberPresent(): number { let x: number = 6; const arr: number[] = [10]; [x] = arr; return x; }
export function varTarget(): number { var x: number = 6; const arr: number[] = [10]; [x] = arr; return x; }
export function pair(): number { let x: number = 1; let y: number = 2; const arr: number[] = [10, 20]; [x, y] = arr; return x * 100 + y; }
export function hole(): number { let x: number = 1; const arr: number[] = [10, 20]; [, x] = arr; return x; }
export function inIf(): number { let x: number = 1; if (x === 1) { const arr: number[] = [7]; [x] = arr; } return x; }
export function inLoop(): number { let x: number = 0; for (let i = 0; i < 3; i = i + 1) { const arr: number[] = [5]; [x] = arr; x = x + 1; } return x; }
export function inTry(): number { let x: number = 1; try { const arr: number[] = [9]; [x] = arr; } catch { return 0; } return x; }
export function fromEmpty(): number { let x: number | undefined = 1; const arr: number[] = []; [x] = arr; return x === undefined ? 100 : 7; }
export function fromNonEmpty(): number { let x: number | undefined = 1; const arr: number[] = [3]; [x] = arr; if (x === undefined) { return 100; } return x * 10; }
export function guarded(): number { let x: number | undefined = 5; const arr: (number | undefined)[] = [undefined]; if (x === undefined) { return 1; } [x] = arr; return x === undefined ? 100 : 7; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_assignment::optionalAbsent(&ctx), 100.0);
    assert_eq!(destructuring_assignment::numberPresent(&ctx), 10.0);
    assert_eq!(destructuring_assignment::varTarget(&ctx), 10.0);
    assert_eq!(destructuring_assignment::pair(&ctx), 1020.0);
    assert_eq!(destructuring_assignment::hole(&ctx), 20.0);
    assert_eq!(destructuring_assignment::inIf(&ctx), 7.0);
    assert_eq!(destructuring_assignment::inLoop(&ctx), 6.0);
    assert_eq!(destructuring_assignment::inTry(&ctx), 9.0);
    assert_eq!(destructuring_assignment::fromEmpty(&ctx), 100.0);
    assert_eq!(destructuring_assignment::fromNonEmpty(&ctx), 30.0);
    assert_eq!(destructuring_assignment::guarded(&ctx), 100.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "node assigns the element reads back into the bindings (`[x] = arr` is not a \
         declaration), so every shape must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_assignment", &rust);
    let stdout = run_generated_crate("destructuring_assignment", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_destructuring_assignment_reuses_the_declaration_element_reads() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
        function provable(): i64 { let x: i64 = 0; const arr: i64[] = [7]; [x] = arr; return x; }
        function optionalFromParam(arr: i64[]): i64 { let x: i64 | undefined = 0; [x] = arr; if (x === undefined) { return 1; } return x; }
        ",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert_eq!(
        rust.matches("__ts_aot_array_get_or_default::<i64>").count(),
        1,
        "a provable element must use the `Default` accessor the destructuring declaration uses, \
         got:\n{rust}"
    );
    assert_eq!(
        rust.matches("__ts_aot_array_get::<i64>").count(),
        1,
        "an element of a parameter array may be absent, so the assignment must use the fallible \
         accessor the destructuring declaration uses, got:\n{rust}"
    );
}

#[test]
fn an_unsupported_destructuring_assignment_is_a_hard_error() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    for (what, source, code) in [
        (
            "object pattern",
            "function f(o: { a: i64 }): i64 { let a: i64 = 1; ({ a } = o); return a; }",
            "E0500",
        ),
        (
            "nested pattern",
            "function f(inner: i64[]): i64 { let x: i64 = 1; [[x]] = [inner]; return x; }",
            "E0500",
        ),
        (
            "rest element",
            "function f(arr: i64[]): i64 { let x: i64 = 1; let r: i64[] = []; [x, ...r] = arr; return x; }",
            "E0500",
        ),
        (
            "default value",
            "function f(arr: i64[]): i64 { let x: i64 = 1; [x = 5] = arr; return x; }",
            "E0500",
        ),
        (
            "member target",
            "function f(o: { a: i64 }, arr: i64[]): i64 { [o.a] = arr; return o.a; }",
            "E0500",
        ),
        (
            "non-array rhs",
            "function f(): i64 { let x: i64 = 1; [x] = 42; return x; }",
            "E0500",
        ),
        (
            "expression position",
            "function f(): i64 { let x: i64 = 1; const arr: i64[] = [2]; const r = ([x] = arr); return x + r.length; }",
            "E0500",
        ),
        (
            "optional element in a non-optional target",
            "function f(): i64 { let x: i64 = 1; const arr: (i64 | undefined)[] = [undefined]; [x] = arr; return x === undefined ? 100 : 7; }",
            "P0018",
        ),
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.has_errors(),
            "`{what}` must be refused instead of leaving the target at its previous value; got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == code),
            "`{what}` must be refused with {code}; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{what}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_destructuring_assignment_loop_head_of_an_existing_binding_is_a_hard_error() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    for (what, source) in [
        (
            "for-of over a plain binding",
            "function f(arr: i64[]): i64 { let x: i64 = 0; for (x of arr) { } return x; }",
        ),
        (
            "for-in over a plain binding",
            "function f(o: { a: i64 }): i64 { let k: string = ''; for (k in o) { } return k.length; }",
        ),
        (
            "for-of over an array pattern",
            "function f(inner: i64[]): i64 { let x: i64 = 0; for ([x] of [inner]) { } return x; }",
        ),
        (
            "for-of over a declared array pattern",
            "function f(inner: i64[]): i64 { for (let [x] of [inner]) { } return 0; }",
        ),
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.has_errors(),
            "`{what}` currently lowers to a type-error crate, so it must be a hard error; got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "E0500"),
            "`{what}` must be refused with E0500; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{what}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn an_array_destructuring_of_a_temporary_reads_every_element() {
    const SOURCE: &str = r"
        export function fromLiteral(): number {
            const [a, b] = [10.5, 20.5];
            return a + b;
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
    assert_eq!(destructuring_temporary::fromLiteral(&ctx), 31.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_temporary", &rust);
    let stdout = run_generated_crate("destructuring_temporary", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn e2e_array_destructuring_emits_runtime_call_per_element() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
        function f(arr: i64[]): i64 {
            let [a, b, c] = arr;
            return (a === undefined ? 0 : 1) + (b === undefined ? 0 : 1) + (c === undefined ? 0 : 1);
        }
        ",
        &opts,
    );
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let rust = common::normalize_rust(&rust);
    assert_eq!(
        rust.matches("__ts_aot_array_get::<").count(),
        3,
        "array destructuring must emit one typed runtime call per binding, got:\n{rust}"
    );
    assert_eq!(
        rust.matches("__ts_aot_array_get_or_default::<").count(),
        0,
        "an element of a parameter array may be absent, so JavaScript gives `undefined` and the \
         `Default`-requiring accessor would silently produce `0`; every binding must be read \
         through the fallible accessor, got:\n{rust}"
    );
}

#[test]
fn e2e_array_destructuring_on_non_array_rhs_emits_diagnostic() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
        function f(): i64 {
            let [a, b] = 42;
            return a;
        }
        ",
        &opts,
    );
    let has_diagnostic = out
        .diagnostics
        .iter()
        .any(|d| d.message.contains("array type") || d.message.contains("destructuring"));
    assert!(
        has_diagnostic,
        "destructuring non-array rhs must surface a diagnostic, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn a_destructured_mutable_receiver_reassigned_by_a_callback_is_refused() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    for (what, source) in [
        (
            "`let`",
            "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } }\n\
             export function got0(): number { const xs: Counter[] = [new Counter()]; let [c] = xs; \
             const bump = (): number => { c = new Counter(); return 5; }; c.add(bump()); return \
             c.n; }",
        ),
        (
            "`var`",
            "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } }\n\
             export function got1(): number { const xs: Counter[] = [new Counter()]; var [c] = xs; \
             const bump = (): number => { c = new Counter(); return 6; }; c.add(bump()); return \
             c.n; }",
        ),
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.has_errors(),
            "a {what} binding declared by array destructuring is a mutable place, so a receiver \
             read from it is resolved before the arguments run and the callback may reassign it; \
             got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics
                .iter()
                .filter(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign"))
                .count()
                >= 1,
            "the {what} destructured mutable receiver must be refused with its own `may reassign` \
             E0419; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "a refused source must not emit rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_destructured_receiver_that_no_argument_reassigns_is_accepted() {
    const SOURCE: &str = "
class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } }
export function constReceiver(): number {
    const xs: Counter[] = [new Counter()];
    const [c] = xs;
    c.add(3);
    return c.n;
}
export function letReceiver(): number {
    const xs: Counter[] = [new Counter()];
    let [c] = xs;
    c.add(4);
    return c.n;
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
    assert_eq!(destructuring_receiver_no_default::constReceiver(&ctx), 3.0);
    assert_eq!(destructuring_receiver_no_default::letReceiver(&ctx), 4.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "a destructured receiver that no argument reassigns must stay accepted; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0419" && d.message.contains("may reassign")),
        "registering a destructured `let`/`var` binding as a mutable place must not refuse a call \
         whose arguments do not reassign it, and a destructured `const` must stay out of the \
         registration; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get::<").count(),
        2,
        "a destructured element whose type has no Rust `Default` (a class) must be read through \
         the fallible accessor, not the `Default`-requiring one; got:\n{normalized}"
    );
    assert_eq!(
        normalized
            .matches("__ts_aot_array_get_or_default::<")
            .count(),
        0,
        "no destructured element of a `Counter[]` may go through \
         `__ts_aot_array_get_or_default`, which needs `Counter: Default`; got:\n{normalized}"
    );
    check_generated_crate("destructuring_receiver_no_default", &rust);
    let stdout = run_generated_crate("destructuring_receiver_no_default", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_destructured_element_is_optional_when_the_array_may_be_empty() {
    const SOURCE: &str = r#"
class C { n: number = 5; }
export function empty(): number { const xs: C[] = []; const [c] = xs; return c === undefined ? 1 : 2; }
export function present(): number { const xs: C[] = [new C()]; const [c] = xs; return c.n + 1; }
export function presentSecond(): number { const xs: C[] = [new C(), new C()]; const [a, b] = xs; return a.n + b.n; }
export function hole(): number { const xs: C[] = [new C(), new C()]; const [, b] = xs; return b.n; }
export function twice(): number { const xs: C[] = [new C()]; const [a] = xs; const [b] = xs; return a.n + b.n; }
export function letEmpty(): number { let xs: C[] = []; let [c] = xs; return c === undefined ? 1 : 2; }
export function varEmpty(): number { var xs: C[] = []; var [c] = xs; return c === undefined ? 1 : 2; }
export function nestedEmpty(): number { const xs: i64[][] = []; const [a] = xs; return a === undefined ? 1 : 2; }
export function numEmpty(): number { const xs: number[] = []; const [n] = xs; return n === undefined ? 1 : 0; }
export function numEmptyNaN(): number { const xs: number[] = []; const [n] = xs; const z = 0.0; if (n === undefined) { return 1.5 % z; } return 0.0; }
export function strEmpty(): number { const xs: string[] = []; const [s] = xs; return s === undefined ? 1 : 0; }
export function boolEmpty(): number { const xs: boolean[] = []; const [b] = xs; return b === undefined ? 1 : 0; }
export function strPresent(): number { const xs: string[] = ["ab"]; const [s] = xs; return s.length; }
"#;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_absent_element::empty(&ctx), 1.0);
    assert_eq!(destructuring_absent_element::present(&ctx), 6.0);
    assert_eq!(destructuring_absent_element::presentSecond(&ctx), 10.0);
    assert_eq!(destructuring_absent_element::hole(&ctx), 5.0);
    assert_eq!(destructuring_absent_element::twice(&ctx), 10.0);
    assert_eq!(destructuring_absent_element::letEmpty(&ctx), 1.0);
    assert_eq!(destructuring_absent_element::varEmpty(&ctx), 1.0);
    assert_eq!(destructuring_absent_element::nestedEmpty(&ctx), 1.0);
    assert_eq!(destructuring_absent_element::numEmpty(&ctx), 1.0);
    assert!(
        destructuring_absent_element::numEmptyNaN(&ctx).is_nan(),
        "node evaluates `1.5 % 0.0` in the absent branch to NaN"
    );
    assert_eq!(destructuring_absent_element::strEmpty(&ctx), 1.0);
    assert_eq!(destructuring_absent_element::boolEmpty(&ctx), 1.0);
    assert_eq!(destructuring_absent_element::strPresent(&ctx), 2.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized
            .matches("__ts_aot_array_get_or_default::<")
            .count(),
        1,
        "only the provably present `string` element may use the `Default`-requiring accessor; an \
         absent element of `number`, `string` or `boolean` must not silently become the Rust \
         default; got:\n{normalized}"
    );
    assert_eq!(
        normalized.matches("__ts_aot_array_get::<").count(),
        14,
        "every other destructured element must be read through the fallible accessor; got:\n{normalized}"
    );
    assert!(
        normalized.contains("let c : Option<C>="),
        "an element of a `C[]` that may be absent must be bound as `Option<C>`; got:\n{normalized}"
    );
    assert!(
        normalized.contains("let n : Option<f64>="),
        "an element of a `number[]` that may be absent must be bound as `Option<f64>`; got:\n{normalized}"
    );
    assert_eq!(
        normalized.matches("is_none").count(),
        8,
        "each `=== undefined` test on an element that may be absent must test the `Option` \
         instead of folding to a constant; got:\n{normalized}"
    );
    assert!(
        !normalized.contains("if (false)"),
        "no `=== undefined` test on an absent element may fold to a constant; got:\n{normalized}"
    );
    check_generated_crate("destructuring_absent_element", &rust);
    let stdout = run_generated_crate("destructuring_absent_element", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unprovable_destructured_element_in_arithmetic_is_refused() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    for source in [
        "function f(): number { const xs: number[] = []; const [n] = xs; return n + 1; }",
        "function f(): number { const xs: string[] = []; const [s] = xs; return (s + \"x\").length; }",
        "function f(): number { const xs: number[] = []; const [n] = xs; return n < 2 ? 1 : 0; }",
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.has_errors(),
            "`{source}` must be refused instead of emitting `Option<T> + T`; got {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code.as_str() == "P0018" && d.message.contains("optional value")),
            "`{source}` must be refused with P0018: node evaluates the absent element as \
             `undefined` (so `n + 1` is NaN and `s + \"x\"` is `\"undefinedx\"`), and the AOT \
             target has no arithmetic operator for `Option<T>`; got {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn an_absent_element_of_a_nullable_type_is_undefined_not_null() {
    const SOURCE: &str = r"
class C { n: number = 5; }
export function absentNullable(): number { const xs: (C | null)[] = []; const [c] = xs; return c === undefined ? 1 : 2; }
export function presentNull(): number { const xs: (C | null)[] = [null]; const [c] = xs; return c === null ? 3 : 4; }
export function presentValue(): number { const xs: (C | null)[] = [new C()]; const [c] = xs; return c === null ? 5 : 6; }
export function absentVsNull(): number { const xs: (C | null)[] = []; const [c] = xs; return c === null ? 7 : 8; }
export function absentLooseNull(): number { const xs: (C | null)[] = []; const [c] = xs; return c == null ? 9 : 10; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_nullable_absent::absentNullable(&ctx), 1.0);
    assert_eq!(destructuring_nullable_absent::presentNull(&ctx), 3.0);
    assert_eq!(destructuring_nullable_absent::presentValue(&ctx), 6.0);
    assert_eq!(destructuring_nullable_absent::absentVsNull(&ctx), 8.0);
    assert_eq!(destructuring_nullable_absent::absentLooseNull(&ctx), 9.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(!out.has_errors(), "must compile; got {:?}", out.diagnostics);
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert!(
        normalized.contains("Option<Option<C>>"),
        "an absent element of `(C | null)[]` is `undefined`, which the element's own `Option<C>` \
         cannot distinguish from `null`; the binding must carry the undefined kind as a second \
         `Option`, got:\n{normalized}"
    );
    check_generated_crate("destructuring_nullable_absent", &rust);
    let stdout = run_generated_crate("destructuring_nullable_absent", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_undefined_guard_narrows_a_destructured_element() {
    const SOURCE: &str = r"
export function guardAbsent(): number { const xs: number[] = []; const [n] = xs; if (n === undefined) { return 1; } return n + 1; }
export function guardPresent(): number { const xs: number[] = []; xs.push(3); const [n] = xs; if (n === undefined) { return 1; } return n + 1; }
export function neAbsent(): number { const xs: number[] = []; const [n] = xs; if (n !== undefined) { return n + 1; } return 1; }
export function nePresent(): number { const xs: number[] = []; xs.push(4); const [n] = xs; if (n !== undefined) { return n + 1; } return 1; }
export function elsePresent(): number { const xs: number[] = []; xs.push(5); const [n] = xs; if (n === undefined) { return 1; } else { return n + 1; } }
export function reassigned(): number { let xs: number[] = []; xs.push(3); let [n] = xs; if (n === undefined) { return 1; } n = 9; return n + 1; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_undefined_guard::guardAbsent(&ctx), 1.0);
    assert_eq!(destructuring_undefined_guard::guardPresent(&ctx), 4.0);
    assert_eq!(destructuring_undefined_guard::neAbsent(&ctx), 1.0);
    assert_eq!(destructuring_undefined_guard::nePresent(&ctx), 5.0);
    assert_eq!(destructuring_undefined_guard::elsePresent(&ctx), 6.0);
    assert_eq!(destructuring_undefined_guard::reassigned(&ctx), 10.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "the P0018 text tells the author to narrow, so `n === undefined` / `n !== undefined` on an \
         optional destructured element must narrow it for the rest of the block; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_undefined_guard", &rust);
    let stdout = run_generated_crate("destructuring_undefined_guard", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_var_redeclaration_after_an_undefined_guard_keeps_the_binding_optional() {
    const SOURCE: &str = r"
export function got(): number { let xs: number[] = []; xs.push(3); var [n] = xs; if (n === undefined) { return 1; } var n = 7; return n + 1; }
";
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "P0018" && d.message.contains("optional value")),
        "node evaluates this to 8 (`var n = 7` writes the same binding), but a narrowed copy taken \
         before the `var` redeclaration is stale; the write must drop the narrowing and the read \
         must be refused with P0018 instead of emitting the stale value; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn a_var_redeclaration_after_an_undefined_guard_reads_the_reassigned_value() {
    const SOURCE: &str = r"
export function got(): number {
    let xs: number[] = [];
    xs.push(3);
    var [n] = xs;
    if (n === undefined) { return 1; }
    var n = 7;
    return n === 7 ? 70 : 30;
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
    assert_eq!(destructuring_var_redeclare_value::got(&ctx), 70.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "node reads `n` after `var n = 7` as 7 and the equality test answers 70 only for that \
         value; a stale narrowed copy (3) or any other stale value answers 30, so this shape must \
         compile and run to discriminate the dropped narrowing; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_var_redeclare_value", &rust);
    let stdout = run_generated_crate("destructuring_var_redeclare_value", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn calling_a_destructured_function_element_resolves_the_optional() {
    const SOURCE: &str = r"
type F = () => number;
export function present(): number { const xs: F[] = []; xs.push((): number => 7); const [g] = xs; return g(); }
export function guarded(): number { const xs: F[] = []; const [g] = xs; const fallback: F = (): number => 8; return (g ?? fallback)(); }
export function compared(): number { const xs: F[] = []; const [g] = xs; return g === undefined ? 1 : 2; }
export function passed(): number { const xs: F[] = []; const [g] = xs; return take(g); }
export function take(f: F | undefined): number { return f === undefined ? 3 : 4; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_fn_call::present(&ctx), 7.0);
    assert_eq!(destructuring_fn_call::guarded(&ctx), 8.0);
    assert_eq!(destructuring_fn_call::compared(&ctx), 1.0);
    assert_eq!(destructuring_fn_call::passed(&ctx), 3.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "calling a destructured function element must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_fn_call", &rust);
    let stdout = run_generated_crate("destructuring_fn_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn calling_an_absent_destructured_function_element_panics() {
    const SOURCE: &str = r"
type F = () => number;
export function absent(): number { const xs: F[] = []; const [g] = xs; return g(); }
export function read(): number { const xs: F[] = []; const [g] = xs; return g === undefined ? 7 : 9; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(
        destructuring_fn_absent::read(&ctx),
        7.0,
        "reading the absent element as a value must not panic; only the call panics"
    );
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        destructuring_fn_absent::absent(&ctx)
    }));
    assert!(
        called.is_err(),
        "node throws a TypeError when a destructured function element is absent; the AOT target \
         has no exceptions, so the generated call must panic instead of returning a value"
    );
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "an absent-able destructured function element must still be accepted; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    assert!(
        rust.contains("unwrap"),
        "calling an absent-able destructured function element must unwrap the `Option` before the \
         fn-value call, otherwise the generated crate calls an `Option` and does not compile; \
         got:\n{rust}"
    );
    check_generated_crate("destructuring_fn_absent", &rust);
    let stdout = run_generated_crate("destructuring_fn_absent", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_provably_present_literal_element_writes_a_numeric_target_of_another_width() {
    const SOURCE: &str = r"
export function literal(): number { let x: number = 0; [x] = [10]; return x; }
export function pair(): number { let x: number = 0; let y: number = 0; [x, y] = [10, 20]; return x * 100 + y; }
export function hole(): number { let x: number = 0; [,, x] = [1, 2, 3]; return x; }
export function optionalTarget(): number { let x: number | undefined = 0; [x] = [10]; if (x === undefined) { return 100; } return x; }
export function aliasLocal(): number { let x: number = 0; const arr = [4]; const t = arr; [x] = t; return x; }
export function annotatedLocal(): number { let x: number = 0; const arr: number[] = [5]; [x] = arr; return x; }
export function expressionList(): number { let x: number = 0; let y: number = 0; ([x] = [1], [y] = [2]); return x * 10 + y; }
export function nestedExpressionList(): number { let x: number = 0; let y: number = 0; ([x] = [1], ([y] = [2], [x] = [3])); return x * 10 + y; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_provable_numeric::literal(&ctx), 10.0);
    assert_eq!(destructuring_provable_numeric::pair(&ctx), 1020.0);
    assert_eq!(destructuring_provable_numeric::hole(&ctx), 3.0);
    assert_eq!(destructuring_provable_numeric::optionalTarget(&ctx), 10.0);
    assert_eq!(destructuring_provable_numeric::aliasLocal(&ctx), 4.0);
    assert_eq!(destructuring_provable_numeric::annotatedLocal(&ctx), 5.0);
    assert_eq!(destructuring_provable_numeric::expressionList(&ctx), 12.0);
    assert_eq!(
        destructuring_provable_numeric::nestedExpressionList(&ctx),
        32.0
    );
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "node writes the literal element into the target (`[x] = [10]` sets `x` to 10), so a \
         provably present element must be accepted even when the literal's numeric width differs \
         from the annotated target's; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized
            .matches("__ts_aot_array_get_or_default::<")
            .count(),
        12,
        "a provably present element must be read through the `Default` accessor and then written \
         with the target's numeric width; got:\n{normalized}"
    );
    assert_eq!(
        normalized.matches("__ts_aot_array_get::<").count(),
        0,
        "none of these arrays may be empty at the destructured index, so no read may go through \
         the fallible accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_provable_numeric", &rust);
    let stdout = run_generated_crate("destructuring_provable_numeric", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_unprovable_numeric_element_in_a_non_optional_target_is_still_refused() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    for source in [
        "function f(xs: number[]): number { let x: number = 0; [x] = xs; return x; }",
        "function f(xs: i64[]): number { let x: number = 0; [x] = xs; return x; }",
        "function f(): i64 { let x: i64 = 1; const arr: (i64 | undefined)[] = [undefined]; [x] = arr; return x === undefined ? 100 : 7; }",
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{source}` reads an element that the array may not hold into a non-optional target; \
             node would write `undefined`, so it must stay a P0018 refusal; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_declaration_destructuring_of_a_function_call_builds_and_reads_the_declared_return() {
    const SOURCE: &str = r"
export function makeFloat(): number[] { return [7.5]; }
export function makeIntLiteral(): number[] { return [7]; }
export function gotFloat(): number { const [x] = makeFloat(); if (x === undefined) { return 100; } return x; }
export function gotIntLiteral(): number { const [x] = makeIntLiteral(); if (x === undefined) { return 100; } return x; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(destructuring_call_return::gotFloat(&ctx), 7.5);
    assert_eq!(destructuring_call_return::gotIntLiteral(&ctx), 7.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "a call to a declared `number[]` function is an array, so `const [x] = makeFloat()` must \
         compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_create::<f64>").count(),
        2,
        "the declared `number[]` return type must reach the returned literal's element type, \
         otherwise the function's `Vec<f64>` signature and its `Vec<i64>` body do not build; \
         got:\n{normalized}"
    );
    assert!(
        !normalized.contains("__local0"),
        "a destructuring whose right-hand side is a typed call must bind its names, not emit a \
         dangling local; got:\n{normalized}"
    );
    check_generated_crate("destructuring_call_return", &rust);
    let stdout = run_generated_crate("destructuring_call_return", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");

    let refused = Driver::new().compile_source(
        "test.ts",
        "function make(): number[] { return [7.5]; } function got(): number { let x: number = 0; [x] = make(); return x; }",
        &CompileOptions {
            module: false,
            ..CompileOptions::default()
        },
    );
    assert!(
        refused
            .diagnostics
            .errors()
            .any(|d| d.code.as_str() == "P0018"),
        "an element read from a call result is not provably present, so the assignment form must \
         be refused with P0018 instead of refusing the whole shape with E0500 or emitting a crate \
         that does not build; got {:?}",
        refused.diagnostics
    );
    assert!(
        refused.rust_source.is_none(),
        "a refused source must not emit Rust; got:\n{}",
        refused.rust_source.unwrap_or_default()
    );
}

#[test]
fn an_element_declared_in_an_enclosing_block_is_provable() {
    const SOURCE: &str = r"
export function got(c: boolean): number {
    let x: number = 0;
    const arr: number[] = [10];
    if (c) { [x] = arr; }
    return x;
}
export function nested(c: boolean): number {
    let x: number = 0;
    const arr: number[] = [10];
    if (c) { if (c) { [x] = arr; } }
    return x;
}
export function shadowed(c: boolean): number {
    let x: number = 0;
    const arr: number[] = [10];
    if (c) { let arr: number[] = [5]; [x] = arr; }
    return x;
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
    assert_eq!(destructuring_enclosing_element::got(&ctx, true), 10.0);
    assert_eq!(destructuring_enclosing_element::got(&ctx, false), 0.0);
    assert_eq!(destructuring_enclosing_element::nested(&ctx, true), 10.0);
    assert_eq!(destructuring_enclosing_element::nested(&ctx, false), 0.0);
    assert_eq!(destructuring_enclosing_element::shadowed(&ctx, true), 5.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "node reads the element from the array the enclosing block declared, so the nested \
         destructuring must be accepted; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized
            .matches("__ts_aot_array_get_or_default::<")
            .count(),
        3,
        "all three destructured elements are provably present, so they must use the `Default` \
         accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_enclosing_element", &rust);
    let stdout = run_generated_crate("destructuring_enclosing_element", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_enclosing_declaration_that_is_unprovable_stays_refused() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    for source in [
        "function f(xs: number[]): number { let x: number = 0; [x] = xs; return x; }",
        "function f(c: boolean): number { let x: number = 0; let arr: number[] = [10]; arr = [20]; if (c) { [x] = arr; } return x; }",
        "function f(c: boolean, xs: number[]): number { let x: number = 0; const arr: number[] = [10]; if (c) { let arr: number[] = xs; [x] = arr; } return x; }",
        "function f(c: boolean): number { let x: number = 0; if (c) var arr: number[] = [10]; [x] = arr; return x; }",
        "function f(c: boolean): number { let x: number = 0; const arr: number[] = [10]; while (c) { [x] = arr; } return x; }",
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{source}` cannot prove the element is present at the nested use, so it must stay a \
             P0018 refusal; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_top_level_destructuring_assignment_is_lowered_like_a_statement() {
    const SOURCE: &str = r"
let x: number = 0;
let y: number = 0;
const arr: number[] = [10, 20];
[x, y] = arr;
let z: number = x + y;
export function got(): number { return x * 100 + y; }
export function after(): number { return z; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_top_level::__ts_aot_init();
    assert_eq!(destructuring_top_level::got(&ctx), 1020.0);
    assert_eq!(destructuring_top_level::after(&ctx), 30.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "node writes both elements into the top-level bindings, so the top-level destructuring \
         must lower like the same statement inside a function; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized
            .matches("__ts_aot_array_get_or_default::<")
            .count(),
        2,
        "both top-level elements are provably present, so they must use the `Default` accessor; \
         got:\n{normalized}"
    );
    check_generated_crate("destructuring_top_level", &rust);
    let stdout = run_generated_crate("destructuring_top_level", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_top_level_destructuring_of_an_unprovable_array_is_refused() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    for source in [
        "function make(): number[] { return [10]; }\nlet x: number = 0;\nconst arr: number[] = make();\n[x] = arr;\nexport function got(): number { return x; }",
        "function make(): number[] { return [10]; }\nlet x: number = 0;\n[x] = make();\nexport function got(): number { return x; }",
        "let x: number = 0;\nlet arr: number[] = [10];\narr = [20, 30];\n[x] = arr;\nexport function got(): number { return x; }",
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{source}` cannot prove the element is present at the top-level use, so it must stay \
             a P0018 refusal; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_mutation_inside_a_destructuring_rhs_invalidates_a_provable_array() {
    const SOURCE: &str = r"
export function f(): number {
  let arr: number[] = [10];
  const empty: number[] = [];
  function id(a: number[]): number { return a.length; }
  let y: number | undefined = 0;
  [y] = [id(arr = empty)];
  let x: number | undefined = 99;
  { [x] = arr; }
  if (x === undefined) { return -1; }
  return x;
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
    assert_eq!(destructuring_rhs_mutation::f(&ctx), -1.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "node reassigns `arr` while evaluating the destructuring RHS, so the later `[x] = arr` \
         must read an absent element; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_rhs_mutation", &rust);
    let stdout = run_generated_crate("destructuring_rhs_mutation", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_mutation_inside_a_destructuring_rhs_makes_a_non_optional_target_a_hard_error() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        r"
export function f(): number {
  let arr: number[] = [10];
  const empty: number[] = [];
  function id(a: number[]): number { return a.length; }
  let y: number | undefined = 0;
  [y] = [id(arr = empty)];
  let x: number = 99;
  { [x] = arr; }
  return x;
}
",
        &opts,
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
        "the RHS reassignment empties `arr`, so `[x] = arr` into a `number` target must be refused \
         with P0018 instead of silently reading the default; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn a_same_body_destructuring_rhs_mutation_is_seen_without_the_fact_cache() {
    const SOURCE: &str = r"
export function g(): number {
  const arr: number[] = [10];
  const empty: number[] = [];
  function id(a: number[]): number { return a.length; }
  let t: number[] = arr;
  let y: number | undefined = 0;
  [y] = [id(t = empty)];
  let x: number | undefined = 99;
  [x] = t;
  if (x === undefined) { return -1; }
  return x;
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
    assert_eq!(destructuring_rhs_mutation_body::g(&ctx), -1.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "the alias `t` is reassigned while evaluating the destructuring RHS, so the later \
         `[x] = t` must read an absent element; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_rhs_mutation_body", &rust);
    let stdout = run_generated_crate("destructuring_rhs_mutation_body", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_function_write_invalidates_a_global_array_presence_proof() {
    const SOURCE: &str = r"
export let garr: number[] = [10];
export const gempty: number[] = [];
function shrink(): void { garr = gempty; }
shrink();
let x: number | undefined = 99;
[x] = garr;
export function observed(): number { if (x === undefined) { return -1; } return 1; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_global_function::__ts_aot_init();
    assert_eq!(destructuring_global_function::observed(&ctx), -1.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "`shrink` empties the global array before the read, so the read must fall back to the \
         optional accessor; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_global_function", &rust);
    let stdout = run_generated_crate("destructuring_global_function", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_read_and_a_push_keep_a_provable_element() {
    const SOURCE: &str = r"
export function read(): number {
  const arr: number[] = [10];
  const seen: number = arr[0];
  let x: number = 0;
  [x] = arr;
  return x * 100 + seen;
}
export function readLength(): number {
  const arr: number[] = [10];
  const n: number = arr.length;
  let x: number = 0;
  [x] = arr;
  return n > 0 ? x : 0;
}
export function push(): number {
  const arr: number[] = [10];
  arr.push(20);
  let x: number = 0;
  [x] = arr;
  return x;
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
    assert_eq!(destructuring_preserving_read::read(&ctx), 1010.0);
    assert_eq!(destructuring_preserving_read::readLength(&ctx), 10.0);
    assert_eq!(destructuring_preserving_read::push(&ctx), 10.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "an element read and a `push` cannot remove index 0, so the destructured element stays \
         provably present and the non-optional targets must be accepted; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_preserving_read", &rust);
    let stdout = run_generated_crate("destructuring_preserving_read", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_global_array_read_keeps_a_provable_element() {
    const SOURCE: &str = r"
export let garr: number[] = [10];
let seen: number = 0;
seen = garr.length;
let x: number = 0;
[x] = garr;
export function got(): number { return x * 10 + seen; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_global_read::__ts_aot_init();
    assert_eq!(destructuring_global_read::got(&ctx), 101.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "`garr.length` only reads the global array, so index 0 stays provably present and the \
         non-optional target must be accepted; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    check_generated_crate("destructuring_global_read", &rust);
    let stdout = run_generated_crate("destructuring_global_read", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_array_mutation_that_can_remove_index_zero_stays_refused() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    for source in [
        "export function f(): number { const arr: number[] = [10]; arr.pop(); let x: number = 0; [x] = arr; return x; }",
        "export function f(): number { const arr: number[] = [10]; arr.shift(); let x: number = 0; [x] = arr; return x; }",
        "export function f(): number { const arr: number[] = [10]; arr.splice(0, 1); let x: number = 0; [x] = arr; return x; }",
        "export function f(): number { const arr: number[] = [10]; arr.sort(); let x: number = 0; [x] = arr; return x; }",
        "export function f(): number { const arr: number[] = [10]; arr.length = 0; let x: number = 0; [x] = arr; return x; }",
        "export function f(): number { const arr: number[] = [10]; arr[0] = 5; let x: number = 0; [x] = arr; return x; }",
        "export function f(): number { let arr: number[] = [10]; arr = []; let x: number = 0; [x] = arr; return x; }",
        "function touch(a: number[]): void { a.pop(); } export function f(): number { const arr: number[] = [10]; touch(arr); let x: number = 0; [x] = arr; return x; }",
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{source}` can remove or replace index 0 before the destructuring, so the read must \
             stay a P0018 refusal; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_module_function_alias_mutation_invalidates_a_global_array_presence_proof() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let source = r"
export let garr: number[] = [10];
let z: number = 0;
[z] = garr;
function shrink(): void { const x: number[] = garr; x.pop(); }
shrink();
let y: number = 0;
[y] = garr;
export function observed(): number { return y; }
";
    let out = Driver::new().compile_source("test.ts", source, &opts);
    let first_site = source.find("[z] = garr").expect("first read") + 1;
    let second_site = source.find("[y] = garr").expect("second read") + 1;
    let sites: Vec<usize> = out
        .diagnostics
        .errors()
        .filter(|d| d.code.as_str() == "P0018")
        .map(|d| d.span.start as usize)
        .collect();
    assert!(
        sites.contains(&second_site),
        "`shrink` is called and can empty the global array through the alias `x` before the \
         second read, so that read into a non-optional target must be refused at its own site; \
         got sites {sites:?} (first read at {first_site}, second read at {second_site})"
    );
    assert!(
        sites
            .iter()
            .all(|site| *site == first_site || *site == second_site),
        "every presence refusal must point at one of the two `garr` destructuring reads; got \
         sites {sites:?} (first read at {first_site}, second read at {second_site})"
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn a_mutation_hiding_in_a_declaration_throw_or_new_position_stays_refused() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    for source in [
        "export function f(): number { let arr: number[] = [10]; const empty: number[] = []; const [y] = [arr = empty]; let x: number = 0; [x] = arr; return x + y.length; }",
        "export function f(): number { let arr: number[] = [10]; const empty: number[] = []; let x: number = 0; try { if (arr.length === 99) { throw (arr = empty).length; } } catch {} [x] = arr; return x; }",
        "export function f(): number { let arr: number[] = [10]; const empty: number[] = []; let x: number = 0; try { if (arr.length === 99) { return (arr = empty).length; } } finally { [x] = arr; } return x; }",
        "class Holder { n: number; constructor(a: number[]) { this.n = a.length; } } export function f(): number { let arr: number[] = [10]; const empty: number[] = []; const h = new Holder(arr = empty); let x: number = 0; [x] = arr; return x + h.n; }",
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            !out.diagnostics
                .errors()
                .any(|d| d.code.as_str().starts_with('E')),
            "`{source}` is otherwise supported, so it must fail only on the presence proof; got \
             {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{source}` mutates the array in a nested position, so the later non-optional \
             destructuring must stay a P0018 refusal; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{source}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_computed_member_call_on_a_proofed_array_is_an_unknown_write() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    for (what, call) in [
        ("a string-key `pop`", r#"arr["pop"]();"#),
        ("a string-key `shift`", r#"arr["shift"]();"#),
        ("a string-key `splice`", r#"arr["splice"](0, 1);"#),
        ("a dynamic-key call", "arr[m]();"),
        (
            "a computed call hiding in a declaration",
            r#"const a: number = arr["pop"]();"#,
        ),
    ] {
        let source = format!(
            "function got(): number {{ const arr: number[] = [1, 2]; const m: string = \"pop\"; \
             let x: number = 0; {call} [x] = arr; return x + m.length; }}"
        );
        let out = Driver::new().compile_source("test.ts", &source, &opts);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{what}` is an unknown method call on the proofed array, so it can remove index 0 \
             before the destructuring and the read into the non-optional `x` must be a P0018 \
             refusal; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{what}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn array_operations_that_preserve_the_index_keep_the_element_proof() {
    const SOURCE: &str = r"
export function got(): number {
    const arr: number[] = [1, 2];
    arr.push(3);
    const first: number = arr[0];
    const len: number = arr.length;
    let x: number = 0;
    [x] = arr;
    return first * 100 + x + len;
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
    assert_eq!(destructuring_preserving_ops::got(&ctx), 104.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "`push`, an index read and `length` preserve index 0, so the non-optional destructuring \
         target must stay provable; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        1,
        "a presence-preserving operation must not force the fallible accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_preserving_ops", &rust);
    let stdout = run_generated_crate("destructuring_preserving_ops", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_hoisted_module_write_declared_after_the_use_invalidates_the_element_proof() {
    const SOURCE: &str = r"
let arr: number[] = [1, 2];
let other: number[] = [];
let a: number | undefined = 0;
let b: number | undefined = 0;
kill();
[a, b] = arr;
function kill(): number { arr = other; return 0; }
export function p(): number { return (a === undefined ? 0 : 100) + (b === undefined ? 0 : 1); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_hoisted_write::__ts_aot_init();
    assert_eq!(destructuring_hoisted_write::p(&ctx), 0.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "both targets accept `undefined`, so the module must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        0,
        "`kill` is hoisted and can run before the read, so the element must not use the \
         `Default` accessor; got:\n{normalized}"
    );
    assert_eq!(
        normalized.matches("__ts_aot_array_get::<f64>").count(),
        2,
        "both elements of an array a later-declared function can empty must be read through the \
         fallible accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_hoisted_write", &rust);
    let stdout = run_generated_crate("destructuring_hoisted_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_nested_module_function_write_declared_after_the_use_invalidates_the_element_proof() {
    const SOURCE: &str = r"
let arr: number[] = [1, 2];
let other: number[] = [];
let a: number | undefined = 0;
let b: number | undefined = 0;
outer();
[a, b] = arr;
function outer(): number {
    function inner(): number { arr = other; return 0; }
    return inner();
}
export function p(): number { return (a === undefined ? 0 : 100) + (b === undefined ? 0 : 1); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_nested_write::__ts_aot_init();
    assert_eq!(destructuring_nested_write::p(&ctx), 0.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "both targets accept `undefined`, so the module must compile; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        0,
        "the write sits in a nested function of a hoisted function, so the element must not use \
         the `Default` accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_nested_write", &rust);
    let stdout = run_generated_crate("destructuring_nested_write", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_never_called_module_function_write_keeps_the_element_proof() {
    const SOURCE: &str = r"
let arr: number[] = [1, 2];
let other: number[] = [];
let a: number = 0;
let b: number = 0;
function kill(): number { arr = other; return 0; }
[a, b] = arr;
export function p(): number { return a * 100 + b; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_never_called::__ts_aot_init();
    assert_eq!(destructuring_never_called::p(&ctx), 102.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "`kill` is never referenced, so it cannot run before the read and the non-optional \
         targets must stay provable; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        2,
        "an unreachable function must not force the fallible accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_never_called", &rust);
    let stdout = run_generated_crate("destructuring_never_called", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_read_only_module_function_destructuring_the_global_keeps_the_element_proof() {
    const SOURCE: &str = r"
let arr: number[] = [1, 2];
let a: number = 0;
let b: number = 0;
function rd(): number { const [c] = arr; return c === undefined ? 90 : 1; }
[a, b] = arr;
export function p(): number { return a * 100 + b + rd(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_read_only::__ts_aot_init();
    assert_eq!(destructuring_read_only::p(&ctx), 103.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "`rd` only reads the global through a destructuring, so the non-optional targets must \
         stay provable; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        2,
        "a read-only function must not force the fallible accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_read_only", &rust);
    let stdout = run_generated_crate("destructuring_read_only", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_class_method_write_invalidates_the_element_proof_in_any_declaration_order() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let source = "let arr: number[] = [1, 2]; let other: number[] = [];\n\
                  class C { m(): number { arr = other; return 0; } }\n\
                  let a: number = 0; let b: number = 0;\n\
                  new C().m();\n\
                  [a, b] = arr;\n\
                  export function p(): number { return a * 100 + b; }";
    let out = Driver::new().compile_source("test.ts", source, &opts);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
        "a method of a class declared before the read is invoked before it can empty the global \
         array, so the read into the non-optional targets must stay a P0018 refusal; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "the refused source must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn a_module_class_method_write_after_the_read_keeps_the_element_proof() {
    const SOURCE: &str = r"
let arr: number[] = [1, 2];
let other: number[] = [];
let a: number = 0;
let b: number = 0;
[a, b] = arr;
class C { m(): number { arr = other; return 0; } }
new C().m();
export function p(): number { return a * 100 + b; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_write_after_read::__ts_aot_init();
    assert_eq!(destructuring_write_after_read::p(&ctx), 102.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "`[a, b] = arr` copies both elements into `a` and `b` at top-level statement 4, while \
         `arr = other` inside `C.m` can only run at top-level statement 6; node's `p()` returns \
         102 and the module must compile. An order-insensitive module-write veto refuses with two \
         P0018 errors here, so this assertion fails without an order-aware veto; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        2,
        "a module write that provably happens after the read cannot empty the array before it, so \
         both elements must keep the `Default` accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_write_after_read", &rust);
    let stdout = run_generated_crate("destructuring_write_after_read", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_module_class_expression_method_write_invalidates_the_element_proof() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    for (what, source) in [
        (
            "a class expression bound to a module variable",
            "let arr: number[] = [1];\n\
             let other: number[] = [];\n\
             const o = class { m(): void { arr = other; } };\n\
             new o().m();\n\
             let a: number = 0;\n\
             [a] = arr;\n\
             export function probe(): number { return a; }",
        ),
        (
            "an exported class expression bound to a module variable",
            "let arr: number[] = [1];\n\
             let other: number[] = [];\n\
             export const o = class { m(): void { arr = other; } };\n\
             new o().m();\n\
             let a: number = 0;\n\
             [a] = arr;\n\
             export function probe(): number { return a; }",
        ),
    ] {
        let out = Driver::new().compile_source("test.ts", source, &opts);
        assert!(
            out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{what}` has a method that empties the global array before the read; the class body \
             lives in `HirDecl::Class` and is invisible to the body write scan, so without the \
             module scan the read into the non-optional `a` silently uses the default 0 while node \
             reads `undefined`; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "`{what}` must not emit Rust; got:\n{}",
            out.rust_source.unwrap_or_default()
        );
    }
}

#[test]
fn a_class_expression_static_block_cannot_keep_the_element_proof() {
    let opts = CompileOptions {
        module: true,
        ..CompileOptions::default()
    };
    let out = Driver::new().compile_source(
        "test.ts",
        "let arr: number[] = [1];\n\
         let other: number[] = [];\n\
         const o = class { static { arr = other; } };\n\
         let a: number = 0;\n\
         [a] = arr;\n\
         export function probe(): number { return a; }",
        &opts,
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
        "node runs the static block while the class expression is evaluated, so it empties `arr` \
         before the read; the frontend drops the block, so the presence proof must refuse instead \
         of reading the default 0; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a class expression whose static block is dropped must not keep the proof; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

#[test]
fn a_write_reachable_only_from_a_never_called_function_keeps_the_element_proof() {
    const SOURCE: &str = r"
let arr: number[] = [1, 2];
let other: number[] = [];
function never(): number { return kill(); }
function kill(): number { arr = other; return 0; }
let a: number = 0;
let b: number = 0;
[a, b] = arr;
export function p(): number { return a * 100 + b; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_unreached_callee::__ts_aot_init();
    assert_eq!(destructuring_unreached_callee::p(&ctx), 102.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "`kill` is referenced only from the never-called `never`, so no reachable path can empty \
         the array before the read; node reads 1 and 2 and the non-optional targets must stay \
         provable; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        2,
        "a writer behind a never-called function must not force the fallible accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_unreached_callee", &rust);
    let stdout = run_generated_crate("destructuring_unreached_callee", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_never_instantiated_class_method_write_keeps_the_element_proof() {
    const SOURCE: &str = r"
let arr: number[] = [1, 2];
let other: number[] = [];
class C { m(): number { arr = other; return 0; } }
class D { get x(): number { arr = other; return 0; } }
let a: number = 0;
let b: number = 0;
[a, b] = arr;
export function p(): number { return a * 100 + b; }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    destructuring_uninstantiated_class::__ts_aot_init();
    assert_eq!(destructuring_uninstantiated_class::p(&ctx), 102.0);
    println!("ok");
}
"#;
    let opts = CompileOptions {
        emit: EmitStage::Crate,
        module: true,
    };
    let out = Driver::new().compile_source("test.ts", SOURCE, &opts);
    assert!(
        !out.has_errors(),
        "neither `C` nor `D` is ever referenced, so no instance can exist and no method or getter \
         can run before the read; node reads 1 and 2 and the non-optional targets must stay \
         provable; got {:?}",
        out.diagnostics
    );
    let rust = out.rust_source.expect("rust_source");
    let normalized = common::normalize_rust(&rust);
    assert_eq!(
        normalized.matches("__ts_aot_array_get_or_default").count(),
        2,
        "a method of a never-instantiated class must not force the fallible accessor; got:\n{normalized}"
    );
    check_generated_crate("destructuring_uninstantiated_class", &rust);
    let stdout = run_generated_crate("destructuring_uninstantiated_class", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_computed_preserving_call_keeps_the_element_proof() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    for (what, call) in [
        ("a string-key `push`", r#"arr["push"](3);"#),
        (
            "a string-key `unshift` on the proved index",
            r#"arr["unshift"](0);"#,
        ),
    ] {
        let source = format!(
            "function got(): number {{ const arr: number[] = [1, 2]; let x: number = 0; {call} \
             [x] = arr; return x; }}"
        );
        let out = Driver::new().compile_source("test.ts", &source, &opts);
        assert!(
            !out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
            "`{what}` cannot remove index 0, so the computed form must preserve the proof exactly \
             like the non-computed form; got {:?}",
            out.diagnostics
        );
        assert!(
            !out.has_errors(),
            "the presence proof is the only refusal this source may hit; got {:?}",
            out.diagnostics
        );
        let rust = out.rust_source.expect("rust_source");
        let normalized = common::normalize_rust(&rust);
        assert_eq!(
            normalized.matches("__ts_aot_array_get_or_default").count(),
            1,
            "`{what}` must keep the `Default` accessor for the proved element; got:\n{normalized}"
        );
    }
}

#[test]
fn a_computed_unshift_outside_the_proved_index_stays_refused() {
    let opts = CompileOptions {
        module: false,
        ..CompileOptions::default()
    };
    let source = "function got(): number { const arr: number[] = [1, 2]; arr[\"unshift\"](0); \
                  let y: number = 0; [, y] = arr; return y; }";
    let out = Driver::new().compile_source("test.ts", source, &opts);
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "P0018"),
        "a computed `unshift` only preserves the proved index 0, so the index-1 read into the \
         non-optional `y` must stay a P0018 refusal; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused source must not emit Rust; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}
