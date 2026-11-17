use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

#[test]
fn a_module_level_array_literal_keeps_its_element_type() {
    const SOURCE: &str = r"
class Box { id: number = 0; add(x: number): number { return this.id + x; } }
const arr: Box[] = [new Box()];
const nums: number[] = [1, 2];
const empty: Box[] = [];
function idx(): number { return 0; }
function arg(): number | null { return 512; }
export function got(): number { return arr[idx()].add(arg() ?? 0); }
export function numeric(): number { return nums[0] + nums[1]; }
export function empty_len(): number { return empty.length; }
export function local(): number { const l: Box[] = [new Box()]; return l[0].add(7); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    module_arrays::__ts_aot_init();
    assert_eq!(module_arrays::got(&ctx), 512.0);
    assert_eq!(module_arrays::numeric(&ctx), 3.0);
    assert_eq!(module_arrays::empty_len(&ctx), 0.0);
    assert_eq!(module_arrays::local(&ctx), 7.0);
    println!("ok");
}
"#;
    let out = Driver::new().compile_source(
        "test.ts",
        SOURCE,
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
    let rust = out
        .rust_source
        .expect("emit-crate must populate rust_source");
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("__ts_aot_array_create::<Box>"),
        "a module-level array of object literals must keep `Box` as its element type; got:\n{rust}"
    );
    assert!(
        flat.contains("__ts_aot_array_create::<f64>"),
        "a module-level `number[]` literal must keep `f64` as its element type; got:\n{rust}"
    );
    check_generated_crate("module_arrays", &rust);
    let stdout = run_generated_crate("module_arrays", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
