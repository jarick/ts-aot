use ts_aot_driver::{CompileOptions, Driver, DriverOutput, EmitStage};

mod common;

fn compile(source: &str) -> DriverOutput {
    Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: false,
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
fn generator_with_throw_keeps_generator_return_type_and_panics() {
    let out = compile("function* gen(): i64 { throw 7; }");
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("ts_aot_runtime::Generator<i64>"),
        "a throwing generator must still return a plain Generator, got:\n{r}"
    );
    assert!(
        !r.contains("Result<ts_aot_runtime::Generator"),
        "a throwing generator must not be wrapped in Result (throw happens in next()), got:\n{r}"
    );
    assert!(
        r.contains("__ts_aot_throw"),
        "throw inside a generator must lower to a panic that propagates from next(), got:\n{r}"
    );
}

#[test]
fn async_generator_emits_async_generator_wrapper() {
    let out = compile("async function* gen(): i64 { yield 1; }");
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("ts_aot_runtime::AsyncGenerator<i64>"),
        "async generator must return AsyncGenerator, got:\n{r}"
    );
    assert!(
        r.contains("__ts_aot_async_generator_new"),
        "async generator must use the async generator wrapper, got:\n{r}"
    );
    assert!(
        !r.contains("async fn gen"),
        "an async generator must not be emitted as an async fn, got:\n{r}"
    );
    assert!(
        r.contains("AsyncGenYield"),
        "async generator yields must be wrapped in AsyncGenYield, got:\n{r}"
    );
}

#[test]
fn sync_for_of_over_async_generator_is_rejected() {
    let out = compile(
        "async function* g(): i64 { yield 1; } \
         function run(): i64 { for (const x of g()) { } return 0; }",
    );
    let codes: Vec<&str> = out.diagnostics.errors().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E0413"),
        "for...of over an async generator must be rejected with E0413, got {codes:?}"
    );
}

#[test]
fn async_generator_next_result_value_and_done_are_lowered() {
    let out = compile(
        "async function* g(): i64 { yield 1; } \
         async function run(): bool { const y = await g().next(); return y.done; }",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("AsyncGenYield"),
        "the awaited next() result must be typed as AsyncGenYield, got:\n{r}"
    );
    assert!(
        r.contains("done"),
        "`.done` must lower to an AsyncGenYield field access, got:\n{r}"
    );
}

#[test]
fn for_await_over_named_async_generator_borrows_it() {
    let out = compile(
        "async function* g(): i64 { yield 1; } \
         async function run(): i64 { const it = g(); for await (const x of it) { } return 0; }",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("& mut it") || r.contains("&mut it"),
        "a named async generator iterable must be mutably borrowed (not moved) \
         so it stays accessible after iteration; got:\n{r}"
    );
}

#[test]
fn for_await_over_async_generator_uses_async_loop() {
    let out = compile(
        "async function* g(): i64 { yield 1; } \
         async function run(): i64 { let s: i64 = 0; for await (const x of g()) { s = s + x; } return s; }",
    );
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("__for_await_iter"),
        "for await over an async generator must emit the async iteration loop, got:\n{r}"
    );
    assert!(
        r.contains("__ts_aot_await_value"),
        "for await must await each next() promise, got:\n{r}"
    );
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

#[test]
fn compiled_generator_crate_passes_cargo_check() {
    use std::fs;
    use std::process::Command;
    let out = compile(
        r"
        function* gen(): i64 { yield 1; yield 2; return 3; }
        function main(): i64 {
            const g = gen();
            g.next();
            g.next();
            return 0;
        }
        ",
    );
    assert!(
        !out.has_errors(),
        "compile must succeed, got: {:?}",
        out.diagnostics
    );
    let r = rust(&out);
    assert!(r.contains("yield"), "yield must survive, got:\n{r}");
    assert!(r.contains("next ()"), "next() must survive, got:\n{r}");

    if std::env::var_os("TS_AOT_SKIP_E2E_CARGO").is_some() {
        eprintln!(
            "skipping compiled_generator_crate_passes_cargo_check: TS_AOT_SKIP_E2E_CARGO is set \
             (offline target/ unavailable or cargo not on PATH in this env)"
        );
        return;
    }

    let unique = format!(
        "ts_aot_gen_e2e_{}_{}",
        std::process::id(),
        match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_nanos(),
            Err(_) => 0,
        }
    );
    let tmp = std::env::temp_dir().join(&unique);
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create temp dir");
    fs::create_dir_all(tmp.join("src")).expect("create src dir");

    let shared_target = std::env::temp_dir().join(format!("{unique}_target"));
    let _ = fs::remove_dir_all(&shared_target);
    fs::create_dir_all(&shared_target).expect("create shared target dir");

    let runtime_manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("ts-aot-runtime");
    let runtime_manifest_str = runtime_manifest.to_string_lossy().replace('\\', "/");
    let target_str = shared_target.to_string_lossy().replace('\\', "/");

    fs::write(
        tmp.join("Cargo.toml"),
        format!(
            r#"[package]
name = "ts_aot_gen_e2e"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "ts_aot_gen_e2e"
path = "src/main.rs"

[dependencies]
ts-aot-runtime = {{ path = "{runtime_manifest_str}" }}
"#
        ),
    )
    .expect("write Cargo.toml");

    fs::write(tmp.join("src/lib.rs"), r).expect("write lib.rs");
    fs::write(
        tmp.join("src/main.rs"),
        "fn main() { ts_aot_gen_e2e::__ts_aot_dispatch_main(&[]); }\n",
    )
    .expect("write main.rs");

    let mut cmd = Command::new("cargo");
    cmd.current_dir(&tmp)
        .env("CARGO_TARGET_DIR", &target_str)
        .args(["check", "--quiet", "--offline"]);
    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            let _ = fs::remove_dir_all(&shared_target);
            eprintln!(
                "skipping compiled_generator_crate_passes_cargo_check: cargo invocation failed: \
                 {e} (set TS_AOT_SKIP_E2E_CARGO=1 to silence this in offline envs)"
            );
            return;
        }
    };
    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_dir_all(&shared_target);
    assert!(
        status.success(),
        "cargo check must succeed for the compiled generator crate, exit: {status:?}"
    );
}

#[test]
fn async_generator_await_uses_generator_await_helper() {
    let out =
        compile("async function* g(p: Promise<i64>): i64 { const v: i64 = await p; yield v; }");
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("__ts_aot_await_value_gen"),
        "async generator await must use the generator await helper so a rejected await rejects next(), got:\n{r}"
    );
}

#[test]
fn plain_async_function_await_does_not_use_generator_await_helper() {
    let out = compile("async function f(p: Promise<i64>): Promise<i64> { return await p; }");
    assert!(!out.has_errors(), "got: {:?}", out.diagnostics);
    let r = rust(&out);
    assert!(
        r.contains("__ts_aot_await_value") && !r.contains("__ts_aot_await_value_gen"),
        "a plain async function await must not use the generator await helper, got:\n{r}"
    );
}
