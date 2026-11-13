use std::collections::HashSet;

use ts_aot_driver::{CompileOptions, Driver, EmitStage};

const REPEATED_SOURCE: &str = "export function got(): number { \
     const alpha = (): number => 1; \
     const beta = (): number => 2; \
     const gamma = (): number => 3; \
     const delta = beta; \
     const epsilon = gamma; \
     const zeta = alpha; \
     return delta() + epsilon() + zeta(); \
 }";

const REPEATS: usize = 10;

fn compile_repeatedly(emit: EmitStage) -> (HashSet<String>, HashSet<Vec<String>>) {
    let mut artifacts: HashSet<String> = HashSet::new();
    let mut code_sets: HashSet<Vec<String>> = HashSet::new();
    for _ in 0..REPEATS {
        let out = Driver::new().compile_source(
            "test.ts",
            REPEATED_SOURCE,
            &CompileOptions { emit, module: true },
        );
        let codes = out
            .diagnostics
            .iter()
            .map(|d| d.code.as_str().to_owned())
            .collect();
        let artifact = match emit {
            EmitStage::Hir => out.hir_text,
            _ => out.rust_source,
        }
        .expect("the requested emit stage must populate its artifact");
        artifacts.insert(artifact);
        code_sets.insert(codes);
    }
    (artifacts, code_sets)
}

#[test]
fn repeated_compilation_of_the_same_module_emits_identical_hir() {
    let (artifacts, code_sets) = compile_repeatedly(EmitStage::Hir);
    assert_eq!(
        code_sets.len(),
        1,
        "the same module must produce the same diagnostic codes on every compilation"
    );
    assert_eq!(
        artifacts.len(),
        1,
        "the same module must produce byte-identical HIR on every compilation, got {} distinct \
         dumps out of {REPEATS}; the frontend assigns LocalIds while iterating a HashMap of visible \
         names, so repeated compilations disagree",
        artifacts.len()
    );
}

#[test]
fn repeated_compilation_of_the_same_module_emits_identical_rust() {
    let (artifacts, code_sets) = compile_repeatedly(EmitStage::Rust);
    assert_eq!(
        code_sets.len(),
        1,
        "the same module must produce the same diagnostic codes on every compilation"
    );
    assert_eq!(
        artifacts.len(),
        1,
        "the same module must produce byte-identical Rust on every compilation, got {} distinct \
         sources out of {REPEATS}",
        artifacts.len()
    );
}
