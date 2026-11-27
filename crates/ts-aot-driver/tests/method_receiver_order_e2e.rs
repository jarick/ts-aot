use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::run_generated_crate;

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

fn normalized(rust: &str) -> String {
    rust.chars().filter(|c| !c.is_whitespace()).collect()
}

fn codes(out: &ts_aot_driver::DriverOutput) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn compile_hir(source: &str) -> ts_aot_driver::DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Hir,
            module: true,
        },
    )
}

fn function_body(rust: &str, name: &str) -> String {
    let norm = normalized(rust);
    let signature = format!("pubfn{name}<'a>(");
    let start = norm
        .find(&signature)
        .unwrap_or_else(|| panic!("no `{name}` in generated code:\n{norm}"));
    let tail = &norm[start..];
    let end = tail[1..].find("pubfn").map_or(tail.len(), |i| i + 1);
    tail[..end].to_owned()
}

const ORDER_SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
     let order: number = 0; \
     function make(): Counter { order = order * 10 + 1; return new Counter(); } \
     function arg(): number | null { order = order * 10 + 2; return 5; } \
     export function got(): number { make().add(arg() ?? 0); return order; }";

#[test]
fn a_call_receiver_is_materialized_before_the_argument_statements() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(receiver_order::got(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = rust_of(ORDER_SOURCE);
    let body = function_body(&rust, "got");
    let receiver_at = body
        .find("make(__ctx)")
        .unwrap_or_else(|| panic!("the receiver call must survive; got:\n{body}"));
    let argument_at = body
        .find("arg(__ctx)")
        .unwrap_or_else(|| panic!("the argument call must survive; got:\n{body}"));
    assert!(
        receiver_at < argument_at,
        "the side effects of the receiver must be materialized ahead of the argument's \
         statements; got:\n{body}"
    );
    assert!(
        !body.contains("&mut(make(__ctx))"),
        "the receiver call must not be borrowed inline, because that moves its evaluation \
         behind the argument statements; got:\n{body}"
    );
    let stdout = run_generated_crate("receiver_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_captured_receiver_binding_is_released_before_the_call() {
    const SOURCE: &str = "class Counter { n: number = 0; \
             add(f: () => number): void { this.n = 10; this.n = this.n + f(); } } \
         export function got(): number { \
             const orig = new Counter(); \
             let c: Counter = orig; \
             const read = (): number => c.n; \
             c.add(read); \
             return orig.n; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(receiver_binding::got(&ctx), 20.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "got");
    assert!(
        body.contains(".borrow_mut()).as_mut().unwrap()).clone();"),
        "the shared receiver handle must be cloned in a statement of its own, so the `RefCell` \
         borrow of the environment slot ends before the method call; got:\n{body}"
    );
    let stdout = run_generated_crate("receiver_binding", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_place_receiver_keeps_the_method_mutations() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let hits: number = 0; \
         function arg(): number { hits = hits + 1; return 5; } \
         export function got(): number { const c = new Counter(); c.add(arg()); return c.n * 10 + hits; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(receiver_place::got(&ctx), 51.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("receiver_place", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_index_receiver_with_a_call_index_keeps_the_method_mutations() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let other: number = 0; \
         function idx(): number { return 0; } \
         function arg(): number | null { other = other + 1; return 5; } \
         export function got(): number { \
             const arr: Counter[] = [new Counter()]; \
             arr[idx()].add(arg() ?? 0); \
             return arr[0].n * 100 + other; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(index_receiver::got(&ctx), 501.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("index_receiver", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn an_index_receiver_computes_the_index_before_the_argument_statements() {
    const SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let order: number = 0; \
         function idx(): number { order = order * 10 + 1; return 0; } \
         function arg(): number | null { order = order * 10 + 2; return 5; } \
         export function got(): number { \
             const arr: Counter[] = [new Counter()]; \
             arr[idx()].add(arg() ?? 0); \
             return arr[0].n * 100 + order; \
         }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(index_order::got(&ctx), 512.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "got");
    let index_at = body
        .find("idx(__ctx)")
        .unwrap_or_else(|| panic!("the index call must survive; got:\n{body}"));
    let argument_at = body
        .find("arg(__ctx)")
        .unwrap_or_else(|| panic!("the argument call must survive; got:\n{body}"));
    assert!(
        index_at < argument_at,
        "the index of a place receiver must be materialized ahead of the argument's statements; \
         got:\n{body}"
    );
    assert!(
        body.contains("&(arr[") && !body.contains("&mut(arr["),
        "the receiver must stay a shared place of the original element; got:\n{body}"
    );
    let stdout = run_generated_crate("index_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_reassigned_alias_target_is_not_resolved_to_one_function() {
    const REASSIGNED_BY_A_FUNCTION: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function bump(x: number): number { counter = new Counter(); return x; } \
         function quiet(x: number): number { return x; } \
         let h: (x: number) => number; \
         function arm(): void { h = bump; } \
         h = quiet; \
         arm(); \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    const REASSIGNED_DIRECTLY: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function bump(x: number): number { counter = new Counter(); return x; } \
         function quiet(x: number): number { return x; } \
         let h: (x: number) => number; \
         h = bump; \
         h = quiet; \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    const REASSIGNED_DIRECTLY_REVERSED: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function bump(x: number): number { counter = new Counter(); return x; } \
         function quiet(x: number): number { return x; } \
         let h: (x: number) => number; \
         h = quiet; \
         h = bump; \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    const UNAMBIGUOUS: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         let counter: Counter = new Counter(); \
         function quiet(x: number): number { return x; } \
         const h: (x: number) => number = quiet; \
         export function got(): number { counter.add(h(1)); return counter.n; }";
    let ambiguous = [
        (
            "`h` is reassigned inside a function body",
            REASSIGNED_BY_A_FUNCTION,
        ),
        ("`h` is reassigned directly", REASSIGNED_DIRECTLY),
        (
            "`h` is reassigned directly, with the harmless target written first",
            REASSIGNED_DIRECTLY_REVERSED,
        ),
    ];
    for (what, source) in ambiguous {
        let out = compile_hir(source);
        assert!(
            codes(&out).contains(&"E0419"),
            "{what} and ends up holding another target, so a call through it may still be the one \
             that reassigns the module binding `counter`; the receiver order cannot be proven, so \
             the call must be refused; got {:?}",
            out.diagnostics
        );
    }

    let out = compile_hir(UNAMBIGUOUS);
    assert!(
        !out.has_errors(),
        "a single alias target must still resolve to its function, so this call stays accepted; \
         got {:?}",
        out.diagnostics
    );
}

const REFUSED_PLACE_SOURCES: [(&str, &str); 7] = [
    (
        "the argument call replaces the field the receiver was read from",
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         class Holder { f: Counter = new Counter(); bump(): number { this.f = new Counter(); \
         return 5; } } \
         export function got(): number { const h = new Holder(); h.f.add(h.bump()); \
         return h.f.n; }",
    ),
    (
        "the argument call replaces the field of `this` the receiver was read from",
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         class Holder { f: Counter; constructor() { this.f = new Counter(); } \
         bump(): number { this.f = new Counter(); return 5; } \
         run(): number { this.f.add(this.bump()); return this.f.n; } } \
         export function got(): number { const h = new Holder(); return h.run(); }",
    ),
    (
        "the argument call reassigns the local the receiver was read from",
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         export function got(): number { \
             let c = new Counter(); \
             const bump = (): number => { c = new Counter(); return 5; }; \
             c.add(bump()); \
             return c.n; \
         }",
    ),
    (
        "the argument call reassigns a loop binding the receiver was read from",
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         export function got(): number { \
             const xs: Counter[] = [new Counter()]; \
             let total = 0; \
             for (let c: Counter of xs) { \
                 const bump = (): number => { c = new Counter(); return 5; }; \
                 c.add(bump()); \
                 total = total + c.n; \
             } \
             return total; \
         }",
    ),
    (
        "the argument call reassigns the root through a nested function declaration",
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         class Holder { f: Counter = new Counter(); } \
         export function got(): number { \
             let h = new Holder(); \
             function replace(): number { h = new Holder(); return 5; } \
             h.f.add(replace()); \
             return h.f.n; \
         }",
    ),
    (
        "the argument call replaces the array element the receiver was read from",
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         export function got(): number { \
             const arr: Counter[] = [new Counter()]; \
             const reset = (): number => { arr[0] = new Counter(); return 5; }; \
             arr[0].add(reset()); \
             return arr[0].n; \
         }",
    ),
    (
        "the argument call reassigns a parameter that has a default value",
        "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
         export function got(): number { \
             function f(c: Counter = new Counter()): number { \
                 c.add(c = new Counter()); \
                 return c.n; \
             } \
             return f(); \
         }",
    ),
];

#[test]
fn a_mutable_place_receiver_reassigned_by_the_arguments_is_refused() {
    for (what, source) in REFUSED_PLACE_SOURCES {
        let out = compile_module(source);
        assert!(
            codes(&out).contains(&"E0419"),
            "{what}: a receiver read from a mutable place before the arguments run is stale when \
             the arguments replace that place, so the call must be refused; got {:?}",
            out.diagnostics
        );
        assert!(
            out.rust_source.is_none(),
            "{what}: a refused call must not emit Rust; got {:?}",
            out.rust_source
        );
    }
}

const PLACE_SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } \
     self(): number { return this.n; } } \
     class Holder { \
         f: Counter; \
         g: Counter; \
         items: number[] = []; \
         constructor() { this.f = new Counter(); this.g = new Counter(); } \
         bump(): number { this.f = new Counter(); return 5; } \
         quiet(): number { return 7; } \
         compute(): number { this.g.n = this.g.n + 100; return 2; } \
         other(): number { this.f = new Counter(); return 3; } \
         read(): number { return this.f.n; } \
         pushComputed(): number { this.items.push(this.compute()); return this.g.n; } \
     } \
     export function got(): number { \
         const h = new Holder(); \
         const other = new Holder(); \
         h.f.add(1); \
         h.f.add(h.g.self()); \
         h.f.add(h.quiet()); \
         h.f.add(other.other()); \
         h.f.add(h.read()); \
         h.f.add(h.compute()); \
         const c = new Counter(); \
         c.add(c.self()); \
         const kept = h.f.n; \
         const seen = h.g.n; \
         const pushed = h.pushComputed(); \
         return kept * 1000 + seen + c.n + pushed; \
     }";

#[test]
fn a_mutable_place_receiver_survives_arguments_that_do_not_write_it() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(place_receiver_safe::got(&ctx), 24300.0);
    println!("ok");
}
"#;
    let rust = rust_of(PLACE_SOURCE);
    let stdout = run_generated_crate("place_receiver_safe", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const INDEX_PLACE_SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
     class Maker { make(): number { return 2.0; } } \
     export function got(): number { \
         const arr: Counter[] = [new Counter()]; \
         const m = new Maker(); \
         arr[0].add(m.make()); \
         arr[0].add(new Counter().n); \
         return arr[0].n; \
     }";

#[test]
fn an_index_place_receiver_accepts_arguments_that_do_not_write_it() {
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(index_place_safe::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(INDEX_PLACE_SOURCE);
    let stdout = run_generated_crate("index_place_safe", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const INDEX_SHARED_SOURCE: &str = "class Counter { n: number = 0; add(x: number): void { this.n = this.n + x; } } \
     export function got(): number { \
         const arr: Counter[] = [new Counter(), new Counter()]; \
         const reset = (): number => { arr[1] = new Counter(); return 5; }; \
         arr[0].add(reset()); \
         return arr[0].n; \
     }";

#[test]
fn an_index_receiver_on_a_shared_array_is_refused() {
    let out = compile_module(INDEX_SHARED_SOURCE);
    assert!(
        codes(&out).contains(&"E0419"),
        "an element receiver of a shared array is emitted as a clone, so a mutating call would lose \
         its write; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "a refused call must not emit Rust; got {:?}",
        out.rust_source
    );
}

#[test]
fn an_element_receiver_releases_the_container_borrow_before_the_call() {
    const SOURCE: &str = "class C { arr: E[] = []; } \
         class E { c: C; constructor(c: C) { this.c = c; } m(): void { this.c.arr.push(new E(this.c)); } } \
         export function got(): number { let c = new C(); c.arr.push(new E(c)); c.arr[0].m(); return c.arr.length; }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(element_receiver::got(&ctx), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let body = function_body(&rust, "got");
    let receiver_at = body.find("__ts_recv").unwrap_or_else(|| {
        panic!(
            "an indexed receiver must be captured into a local, which releases the \
                 `RefCell` borrow of the array before the call; got:\n{body}"
        )
    });
    assert!(
        !body.contains("E__m(&((*c.arr.borrow())"),
        "the call must not hold a `Ref` of the receiver's container while the method runs, because \
         the method reaches the same `RefCell` and would panic; got:\n{body}"
    );
    assert!(
        receiver_at < body.find("E__m").unwrap_or(usize::MAX),
        "the captured receiver must be bound before the call; got:\n{body}"
    );
    let stdout = run_generated_crate("element_receiver", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
