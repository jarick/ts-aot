use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

mod common;

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
        },
    )
}

fn rust(out: &DriverOutput) -> String {
    common::normalize_rust(&out.rust_source.clone().expect("rust must be set"))
}

#[test]
fn simple_generator_compiles_and_yields() {
    let out = compile("function* gen(): i64 { yield 1; return 2; }");
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(r.contains("yield"), "yield must survive, got:\n{r}");
    assert!(r.contains("ts_aot_runtime::Generator<i64>"), "got:\n{r}");
}

#[test]
fn multi_yield_compiles() {
    let out = compile(
        r"
        function* gen(): i64 {
            yield 1;
            yield 2;
            return 3;
        }
        function main(): i64 {
            const g = gen();
            g.next();
            return 0;
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(r.matches("yield").count() >= 2, "two yields, got:\n{r}");
}

#[test]
fn gen_call_returns_generator_value() {
    let out = compile(
        r"
        function* gen(): i64 { yield 1; return 2; }
        function main() {
            const g = gen();
            g.next();
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(r.contains("__ts_aot_generator_new"), "got:\n{r}");
    assert!(r.contains("async move"), "got:\n{r}");
    assert!(r.contains("yield_"), "got:\n{r}");
}

#[test]
fn g_next_emits_direct_method_call() {
    let out = compile(
        r"
        function* gen(): i64 { yield 1; return 0; }
        function main() {
            const g = gen();
            g.next();
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(r.contains("next ()"), "got:\n{r}");
    assert!(
        r.contains("let mut g"),
        "mut binding for g.next(), got:\n{r}"
    );
}

#[test]
fn g_return_and_g_throw_emit_e0502() {
    let out = compile(
        r"
        function* gen(): i64 { yield 1; }
        function main(): i64 {
            const g = gen();
            g.return(5);
            g.throw(new Error());
            return 0;
        }
        ",
    );
    let e0502: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "E0502")
        .collect();
    assert_eq!(
        e0502.len(),
        2,
        "expected exactly two E0502 diagnostics (one for .return, one for .throw), got: {out_diagnostics:?}",
        out_diagnostics = out.diagnostics
    );
    assert_eq!(
        e0502
            .iter()
            .filter(|d| d.message.contains(".return"))
            .count(),
        1,
        "exactly one E0502 must mention .return, got: {e0502:?}"
    );
    assert_eq!(
        e0502
            .iter()
            .filter(|d| d.message.contains(".throw"))
            .count(),
        1,
        "exactly one E0502 must mention .throw, got: {e0502:?}"
    );
}

#[test]
fn for_of_over_generator_borrows_mutably() {
    let out = compile(
        r"
        function* gen(): i64 { yield 1; yield 2; return 3; }
        function main(): i64 {
            let sum: i64 = 0;
            for (const x of gen()) {
                sum = sum + x;
            }
            return sum;
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("let mut sum : i64 = 0"),
        "for-of iterable binding `sum` must be declared as `let mut sum : i64 = 0` \
         (for-of body reassigns `sum`, so locals_mut must promote it to `mut`); got:\n{r}"
    );
    assert!(
        r.contains("& mut (gen_ ())") || r.contains("& mut gen_ ()"),
        "for-of over gen must borrow mutably, got:\n{r}"
    );
    assert!(
        r.contains("for __for_of_"),
        "for-of header must use the stable __for_of_ synth prefix, got:\n{r}"
    );
}

#[test]
fn for_of_then_next_on_same_generator_local() {
    let out = compile(
        r"
        function* gen(): i64 { yield 1; yield 2; return 3; }
        function main(): i64 {
            const g = gen();
            for (const x of g) { x; }
            g.next();
            return 0;
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("let mut g"),
        "the `g` local used by for-of must be declared as `let mut g` \
         (the for-of body borrows `g` mutably, so g must be declared mutable); got:\n{r}"
    );
    assert!(
        r.contains("& mut (g)") || r.contains("& mut g"),
        "for-of borrow, got:\n{r}"
    );
    assert!(r.contains("next ()"), "g.next() after for-of, got:\n{r}");
}

#[test]
fn const_generator_local_for_of_borrows_mutably() {
    let out = compile(
        r"
        function* gen(): i64 { yield 1; yield 2; }
        function main(): i64 {
            const g = gen();
            for (const x of g) { x; }
            return 0;
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("let mut g"),
        "the `g` const local used by for-of must be promoted to `let mut g` \
         (the for-of body borrows `g` mutably); got:\n{r}"
    );
    assert!(
        r.contains("& mut (g)") || r.contains("& mut g"),
        "got:\n{r}"
    );
}

#[test]
fn generator_body_local_kept_across_yield() {
    let out = compile(
        r"
        function* gen(): i64 {
            const x: i64 = 1;
            yield x;
            return 2;
        }
        function main() {
            const g = gen();
            g.next();
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("let x : i64 = 1"),
        "x must keep its binding, got:\n{r}"
    );
    assert!(r.contains("yield_ (x)"), "yield x, got:\n{r}");
}

#[test]
fn generator_params_captured_by_producer() {
    let out = compile(
        r"
        function* range(n: i64): i64 {
            yield n;
            yield n + 1;
        }
        function main() {
            const g = range(10);
            g.next();
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(r.contains("fn range (n : i64)"), "range fn sig, got:\n{r}");
    assert!(r.contains("ts_aot_runtime::Generator<i64>"), "got:\n{r}");
    assert!(r.contains("yield_ (n)"), "yield n, got:\n{r}");
}

#[test]
fn yield_inside_while_loop() {
    let out = compile(
        r"
        function* gen(): i64 {
            let i: i64 = 0;
            while (i < 2) {
                yield i;
                i = i + 1;
            }
        }
        function main() {
            const g = gen();
            g.next();
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(r.contains("yield_ (i)"), "yield i inside while, got:\n{r}");
    assert!(r.contains("while"), "while loop, got:\n{r}");
}

#[test]
fn cross_yield_local_keeps_immutable_binding() {
    let out = compile(
        r"
        function* gen(): i64 {
            let x: i64 = 1;
            yield x;
            yield x + 1;
        }
        function main() {
            const g = gen();
            g.next();
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    let immutable_count = r.matches("let x : i64 = 1").count();
    let mutable_count = r.matches("let mut x : i64 = 1").count();
    assert_eq!(
        immutable_count, 1,
        "x must stay immutable across yield, got:\n{r}"
    );
    assert_eq!(
        mutable_count, 0,
        "x must NOT be mut (no reassignment), got:\n{r}"
    );
    assert!(r.contains("yield_ (x)"), "first yield, got:\n{r}");
    assert!(
        r.contains("yield_ ((x + 1))"),
        "second yield x+1, got:\n{r}"
    );
}

#[test]
fn void_generator_with_bare_yield() {
    let out = compile(
        r"
        function* gen(): void {
            yield;
            yield;
        }
        function main() {
            const g = gen();
            g.next();
        }
        ",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("ts_aot_runtime::Generator<()>"),
        "void generator, got:\n{r}"
    );
    assert!(r.contains("yield_ (())"), "bare yield, got:\n{r}");
    assert!(r.contains("return None"), "void completion, got:\n{r}");
}
