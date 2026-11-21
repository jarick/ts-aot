use ts_aot_driver::{CompileOptions, Driver, EmitStage};
use ts_aot_test_support::{check_generated_crate, run_generated_crate};

fn rust_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.rust_source
        .expect("emit-rust must populate rust_source")
}

fn mir_of(source: &str) -> String {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Mir,
            module: true,
        },
    );
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    out.mir_text.expect("emit-mir must populate mir_text")
}

fn diagnostics_of(source: &str) -> Vec<(String, String)> {
    let out = Driver::new().compile_source(
        "test.ts",
        source,
        &CompileOptions {
            emit: EmitStage::Rust,
            module: true,
        },
    );
    out.diagnostics
        .iter()
        .map(|d| (d.code.as_str().to_owned(), d.message.clone()))
        .collect()
}

#[test]
fn number_to_string_radix_matches_js() {
    const SOURCE: &str = r"
export function hex(): string { return (255).toString(16); }
export function bin(): string { return (10).toString(2); }
export function dec(): string { return (7).toString(); }
export function zero(): string { return (0).toString(36); }
export function neg(): string { return (-255).toString(16); }
export function big(): string { return (1e21).toString(); }
export function small(): string { return (1e-7).toString(); }
export function frac(): string { return (0.5).toString(2); }
export function hexFrac(): string { return (0.1).toString(16); }
export function fromLocal(): string { const x: number = 255; return x.toString(16); }
export function rounded(): string { return (255.9).toString(16); }
export function radixTenth7(): string { return (0.1).toString(7); }
export function radixHalf3(): string { return (0.5).toString(3); }
export function radixThird7(): string { return (0.3333333333333333).toString(7); }
export function radixTiny36(): string { return (5e-324).toString(36); }
export function radixTiny7(): string { return (5e-324).toString(7); }
export function hexFraction(): string { return (16.5).toString(16); }
export function shortestTieEven(): string { return (785068460487425.25).toString(); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn text(value: ts_aot_runtime::JsString<'_>) -> String {
    value.as_valid().expect("ascii result").to_owned()
}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(text(number_radix::hex(&ctx)), "ff");
    assert_eq!(text(number_radix::bin(&ctx)), "1010");
    assert_eq!(text(number_radix::dec(&ctx)), "7");
    assert_eq!(text(number_radix::zero(&ctx)), "0");
    assert_eq!(text(number_radix::neg(&ctx)), "-ff");
    assert_eq!(text(number_radix::big(&ctx)), "1e+21");
    assert_eq!(text(number_radix::small(&ctx)), "1e-7");
    assert_eq!(text(number_radix::frac(&ctx)), "0.1");
    assert_eq!(text(number_radix::hexFrac(&ctx)), "0.1999999999999a");
    assert_eq!(text(number_radix::fromLocal(&ctx)), "ff");
    assert_eq!(text(number_radix::rounded(&ctx)), "ff.e66666666668");
    assert_eq!(text(number_radix::radixTenth7(&ctx)), "0.04620462046204620463");
    assert_eq!(text(number_radix::radixHalf3(&ctx)), "0.1111111111111111111111111111111112");
    assert_eq!(text(number_radix::radixThird7(&ctx)), "0.2222222222222222222");
    assert_eq!(text(number_radix::radixTiny36(&ctx)), "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000003");
    assert_eq!(text(number_radix::radixTiny7(&ctx)), "0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002");
    assert_eq!(text(number_radix::hexFraction(&ctx)), "10.8");
    assert_eq!(text(number_radix::shortestTieEven(&ctx)), "785068460487425.2");
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("__ts_aot_number_to_string"),
        "number.toString must lower to the runtime helper; got:\n{rust}"
    );
    check_generated_crate("number_radix", &rust);
    let stdout = run_generated_crate("number_radix", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn number_to_fixed_exponential_precision_match_js() {
    const SOURCE: &str = r"
export function fixed1(): string { return (255.5).toFixed(1); }
export function fixedTie(): string { return (2.5).toFixed(0); }
export function fixedNegTie(): string { return (-2.5).toFixed(0); }
export function fixedLong(): string { return (1.005).toFixed(2); }
export function fixedZero(): string { return (0).toFixed(2); }
export function fixedHuge(): string { return (1e21).toFixed(2); }
export function expDigits(): string { return (255).toExponential(2); }
export function expDefault(): string { return (255).toExponential(); }
export function expZero(): string { return (0).toExponential(2); }
export function expNeg(): string { return (-0.5).toExponential(3); }
export function precHuge(): string { return (255).toPrecision(5); }
export function precSmall(): string { return (0.000123).toPrecision(2); }
export function precExp(): string { return (12345).toPrecision(2); }
export function precZero(): string { return (0).toPrecision(3); }
export function precDefault(): string { return (255).toPrecision(); }
export function expTieHalf(): string { return (2.5).toExponential(0); }
export function precTieQuarter(): string { return (0.25).toPrecision(1); }
export function expTieLong(): string { return (1352.5).toExponential(3); }
export function precTieLong(): string { return (2864.125).toPrecision(6); }
export function expTieEighth(): string { return (0.125).toExponential(1); }
export function expTieLarge(): string { return (766.5).toExponential(2); }
";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn text(value: ts_aot_runtime::JsString<'_>) -> String {
    value.as_valid().expect("ascii result").to_owned()
}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(text(number_format::fixed1(&ctx)), "255.5");
    assert_eq!(text(number_format::fixedTie(&ctx)), "3");
    assert_eq!(text(number_format::fixedNegTie(&ctx)), "-3");
    assert_eq!(text(number_format::fixedLong(&ctx)), "1.00");
    assert_eq!(text(number_format::fixedZero(&ctx)), "0.00");
    assert_eq!(text(number_format::fixedHuge(&ctx)), "1e+21");
    assert_eq!(text(number_format::expDigits(&ctx)), "2.55e+2");
    assert_eq!(text(number_format::expDefault(&ctx)), "2.55e+2");
    assert_eq!(text(number_format::expZero(&ctx)), "0.00e+0");
    assert_eq!(text(number_format::expNeg(&ctx)), "-5.000e-1");
    assert_eq!(text(number_format::precHuge(&ctx)), "255.00");
    assert_eq!(text(number_format::precSmall(&ctx)), "0.00012");
    assert_eq!(text(number_format::precExp(&ctx)), "1.2e+4");
    assert_eq!(text(number_format::precZero(&ctx)), "0.00");
    assert_eq!(text(number_format::precDefault(&ctx)), "255");
    assert_eq!(text(number_format::expTieHalf(&ctx)), "3e+0");
    assert_eq!(text(number_format::precTieQuarter(&ctx)), "0.3");
    assert_eq!(text(number_format::expTieLong(&ctx)), "1.353e+3");
    assert_eq!(text(number_format::precTieLong(&ctx)), "2864.13");
    assert_eq!(text(number_format::expTieEighth(&ctx)), "1.3e-1");
    assert_eq!(text(number_format::expTieLarge(&ctx)), "7.67e+2");
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("__ts_aot_number_to_fixed")
            && flat.contains("__ts_aot_number_to_exponential")
            && flat.contains("__ts_aot_number_to_precision"),
        "toFixed/toExponential/toPrecision must lower to the runtime helpers; got:\n{rust}"
    );
    check_generated_crate("number_format", &rust);
    let stdout = run_generated_crate("number_format", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_slice_matches_js() {
    const SOURCE: &str = r#"
export function tail(): number[] {
    const a: number[] = [1, 2, 3, 4];
    return a.slice(1);
}
export function negative(): number[] {
    const a: number[] = [1, 2, 3, 4];
    return a.slice(-2);
}
export function window(): number[] {
    const a: number[] = [1, 2, 3, 4];
    return a.slice(1, 3);
}
export function negativeWindow(): number[] {
    const a: number[] = [1, 2, 3, 4];
    return a.slice(-3, -1);
}
export function whole(): number[] {
    const a: number[] = [1, 2, 3];
    return a.slice();
}
export function empty(): number[] {
    const a: number[] = [1, 2, 3];
    return a.slice(5);
}
export function strings(): string[] {
    const a: string[] = ["a", "b", "c"];
    return a.slice(1);
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
    let tail = array_slice::tail(&ctx);
    assert_eq!(tail.len(), 3);
    assert_eq!(tail[0], 2.0);
    assert_eq!(tail[1], 3.0);
    assert_eq!(tail[2], 4.0);
    let negative = array_slice::negative(&ctx);
    assert_eq!(negative.len(), 2);
    assert_eq!(negative[0], 3.0);
    assert_eq!(negative[1], 4.0);
    let window = array_slice::window(&ctx);
    assert_eq!(window.len(), 2);
    assert_eq!(window[0], 2.0);
    assert_eq!(window[1], 3.0);
    let negative_window = array_slice::negativeWindow(&ctx);
    assert_eq!(negative_window.len(), 2);
    assert_eq!(negative_window[0], 2.0);
    assert_eq!(negative_window[1], 3.0);
    let whole = array_slice::whole(&ctx);
    assert_eq!(whole.len(), 3);
    assert_eq!(whole[2], 3.0);
    assert_eq!(array_slice::empty(&ctx).len(), 0);
    let strings = array_slice::strings(&ctx);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].as_valid(), Some("b"));
    assert_eq!(strings[1].as_valid(), Some("c"));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("__ts_aot_array_slice"),
        "Array.prototype.slice must lower to the runtime helper; got:\n{rust}"
    );
    check_generated_crate("array_slice", &rust);
    let stdout = run_generated_crate("array_slice", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_index_of_and_concat_match_js() {
    const SOURCE: &str = r"
export function first(): number {
    const a: number[] = [1, 2, 3, 2];
    return a.indexOf(2);
}
export function fromIndex(): number {
    const a: number[] = [1, 2, 3, 2];
    return a.indexOf(2, 2);
}
export function missing(): number {
    const a: number[] = [1, 2, 3];
    return a.indexOf(9);
}
export function negativeFrom(): number {
    const a: number[] = [1, 2, 3];
    return a.indexOf(3, -1);
}
export function joined(): number[] {
    const a: number[] = [1, 2];
    const b: number[] = [3, 4];
    return a.concat(b);
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
    assert_eq!(array_builtins::first(&ctx), 1.0);
    assert_eq!(array_builtins::fromIndex(&ctx), 3.0);
    assert_eq!(array_builtins::missing(&ctx), -1.0);
    assert_eq!(array_builtins::negativeFrom(&ctx), 2.0);
    let joined = array_builtins::joined(&ctx);
    assert_eq!(joined.len(), 4);
    assert_eq!(joined[0], 1.0);
    assert_eq!(joined[2], 3.0);
    assert_eq!(joined[3], 4.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("__ts_aot_array_index_of"),
        "Array.prototype.indexOf must lower to the runtime helper; got:\n{rust}"
    );
    check_generated_crate("array_builtins", &rust);
    let stdout = run_generated_crate("array_builtins", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_index_of_never_matches_a_fractional_needle_in_an_integer_array() {
    const SOURCE: &str = r"
export function fractional(): number {
    const a: i64[] = [1, 2, 3];
    return a.indexOf(1.5);
}
export function wholeLiteral(): number {
    const a: i64[] = [1, 2, 3];
    return a.indexOf(1);
}
export function wholeFloat(): number {
    const a: i64[] = [1, 2, 3];
    return a.indexOf(1.0);
}
export function fromIndexIsStillEvaluated(): number {
    const a: i64[] = [1, 2, 3];
    let seen: number = 0;
    const bump = (): number => { seen = 5; return 0; };
    a.indexOf(1.5, bump());
    return seen;
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
    assert_eq!(index_of_fraction::fractional(&ctx), -1.0);
    assert_eq!(index_of_fraction::wholeLiteral(&ctx), 0.0);
    assert_eq!(index_of_fraction::wholeFloat(&ctx), 0.0);
    assert_eq!(index_of_fraction::fromIndexIsStillEvaluated(&ctx), 5.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("index_of_fraction", &rust);
    let stdout = run_generated_crate("index_of_fraction", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_index_of_evaluates_the_needle_before_the_from_index() {
    const SOURCE: &str = r"
let log: number = 0;
function needle(): number { log = log * 10 + 1; return 1.5; }
function fromIdx(): number { log = log * 10 + 2; return 0; }
export function got0(): number {
    const a: i64[] = [1, 2, 3];
    a.indexOf(needle(), fromIdx());
    return log;
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
    assert_eq!(index_of_order::got0(&ctx), 12.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    let needle_at = flat
        .find("needle(__ctx)")
        .expect("the needle call must survive");
    let from_at = flat
        .find("fromIdx(__ctx)")
        .expect("the fromIndex call must survive");
    assert!(
        needle_at < from_at,
        "the needle must be evaluated before fromIndex; got:\n{rust}"
    );
    check_generated_crate("index_of_order", &rust);
    let stdout = run_generated_crate("index_of_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_index_of_handles_a_string_receiver() {
    const SOURCE: &str = r#"
export function present(): number {
    const a: string[] = ["a", "b", "c"];
    return a.indexOf("b");
}
export function absent(): number {
    const a: string[] = ["a", "b", "c"];
    return a.indexOf("z");
}
export function fromPresent(): number {
    const a: string[] = ["a", "b", "a"];
    return a.indexOf("a", 1);
}
export function fromOptional(from: number | undefined): number {
    const a: string[] = ["a", "b", "a"];
    return a.indexOf("a", from);
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
    assert_eq!(string_index::present(&ctx), 1.0);
    assert_eq!(string_index::absent(&ctx), -1.0);
    assert_eq!(string_index::fromPresent(&ctx), 2.0);
    assert_eq!(string_index::fromOptional(&ctx, None), 0.0);
    assert_eq!(string_index::fromOptional(&ctx, Some(2.0)), 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("__ts_aot_array_index_of::<ts_aot_runtime::JsString"),
        "a string[] indexOf must be monomorphized for the string element type; got:\n{rust}"
    );
    check_generated_crate("string_index", &rust);
    let stdout = run_generated_crate("string_index", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn prototype_call_and_apply_rewrite_to_direct_calls() {
    const SOURCE: &str = r#"
class Point { x: number = 2; y: number = 3; }
let extraCalls: number = 0;
function bump(): number { extraCalls = extraCalls + 1; return 7; }
export function ownViaCall(): boolean {
    const p = new Point();
    return Object.prototype.hasOwnProperty.call(p, "x");
}
export function absentViaCall(): boolean {
    const p = new Point();
    return Object.prototype.hasOwnProperty.call(p, "z");
}
export function ownViaApply(): boolean {
    const p = new Point();
    return Object.prototype.hasOwnProperty.apply(p, ["y"]);
}
export function directOwn(): boolean {
    const p = new Point();
    return p.hasOwnProperty("x");
}
export function applyExtra(): boolean {
    const p = new Point();
    return Object.prototype.hasOwnProperty.apply(p, ["x"], bump(), bump());
}
export function getExtraCalls(): number { return extraCalls; }
export function numberViaCall(): string {
    const x: number = 255;
    return Number.prototype.toString.call(x, 16);
}
export function arrayViaCall(): number[] {
    const a: number[] = [1, 2, 3, 4];
    return Array.prototype.slice.call(a, 1);
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
    assert!(prototype_call::ownViaCall(&ctx));
    assert!(!prototype_call::absentViaCall(&ctx));
    assert!(prototype_call::ownViaApply(&ctx));
    assert!(prototype_call::directOwn(&ctx));
    assert!(prototype_call::applyExtra(&ctx));
    assert_eq!(prototype_call::getExtraCalls(&ctx), 2.0);
    assert_eq!(
        prototype_call::numberViaCall(&ctx)
            .as_valid()
            .expect("ascii result"),
        "ff"
    );
    let sliced = prototype_call::arrayViaCall(&ctx);
    assert_eq!(sliced.len(), 3);
    assert_eq!(sliced[0], 2.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(
        !rust.contains("hasOwnProperty"),
        "the prototype `.call`/`.apply` forms must be rewritten into direct calls, never a \
         method lookup; got:\n{rust}"
    );
    check_generated_crate("prototype_call", &rust);
    let stdout = run_generated_crate("prototype_call", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn class_value_has_own_property_matches_js() {
    const SOURCE: &str = r##"
class C {
    static foo: number = 1;
    bar: number = 2;
    static sv: number = 0;
    iv: number = 0;
    static m(): number { return 3; }
    n(): number { return 4; }
    static set ss(value: number) { C.sv = value; }
    set is(value: number) { this.iv = value; }
}
export function staticField(): boolean { return Object.prototype.hasOwnProperty.call(C, "foo"); }
export function staticMethod(): boolean { return Object.prototype.hasOwnProperty.call(C, "m"); }
export function prototypeObject(): boolean { return Object.prototype.hasOwnProperty.call(C, "prototype"); }
export function privateBrand(): boolean { return Object.prototype.hasOwnProperty.call(C, "#gen"); }
export function protoInstanceField(): boolean { return Object.prototype.hasOwnProperty.call(C.prototype, "bar"); }
export function protoInstanceMethod(): boolean { return Object.prototype.hasOwnProperty.call(C.prototype, "n"); }
export function protoStaticField(): boolean { return Object.prototype.hasOwnProperty.call(C.prototype, "foo"); }
export function protoConstructor(): boolean { return Object.prototype.hasOwnProperty.call(C.prototype, "constructor"); }
export function instancePrivate(): boolean { return Object.prototype.hasOwnProperty.call(new C(), "#x"); }
export function setterOnlyStatic(): boolean { return Object.prototype.hasOwnProperty.call(C, "ss"); }
export function setterOnlyProto(): boolean { return Object.prototype.hasOwnProperty.call(C.prototype, "is"); }
"##;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(class_own::staticField(&ctx));
    assert!(class_own::staticMethod(&ctx));
    assert!(class_own::prototypeObject(&ctx));
    assert!(!class_own::privateBrand(&ctx));
    assert!(!class_own::protoInstanceField(&ctx));
    assert!(class_own::protoInstanceMethod(&ctx));
    assert!(!class_own::protoStaticField(&ctx));
    assert!(class_own::protoConstructor(&ctx));
    assert!(!class_own::instancePrivate(&ctx));
    assert!(class_own::setterOnlyStatic(&ctx));
    assert!(class_own::setterOnlyProto(&ctx));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(
        !rust.contains("hasOwnProperty"),
        "every literal-key hasOwnProperty check must fold to a constant; got:\n{rust}"
    );
    check_generated_crate("class_own", &rust);
    let stdout = run_generated_crate("class_own", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn null_has_own_property_receiver_throws_type_error() {
    const SOURCE: &str = r##"
export function onNull(): boolean { return Object.prototype.hasOwnProperty.call(null, "x"); }
export function onUndefined(): boolean { return Object.prototype.hasOwnProperty.call(undefined, "y"); }
export function onNullPrivate(): boolean { return Object.prototype.hasOwnProperty.call(null, "#x"); }
export function onReal(): boolean {
    const p: { x: number } = { x: 1 };
    return Object.prototype.hasOwnProperty.call(p, "x");
}
"##;
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert!(null_hastype::onReal(&ctx));
    let ops: [(&str, fn(&ts_aot_runtime::Ctx) -> bool); 3] = [
        ("onNull", null_hastype::onNull as fn(&ts_aot_runtime::Ctx) -> bool),
        (
            "onUndefined",
            null_hastype::onUndefined as fn(&ts_aot_runtime::Ctx) -> bool,
        ),
        (
            "onNullPrivate",
            null_hastype::onNullPrivate as fn(&ts_aot_runtime::Ctx) -> bool,
        ),
    ];
    for (name, op) in ops {
        let thrown =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(&ctx)));
        let err = thrown.expect_err(name);
        let payload = err
            .downcast_ref::<ts_aot_runtime::TypeErrorPayload>()
            .expect("the TypeError payload kind must be preserved");
        assert_eq!(
            payload.to_string(),
            "TypeError: Object.prototype.hasOwnProperty called on null or undefined"
        );
        let caught = ts_aot_runtime::__ts_aot_catch_bind(&*err);
        assert_eq!(
            caught.typeof_str(),
            "object",
            "a caught TypeError must report `object` like a real error object"
        );
    }
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    assert!(
        rust.contains("__ts_aot_throw_type_error"),
        "a null receiver must fold to a runtime TypeError instead of a fold or a method lookup; \
         got:\n{rust}"
    );
    check_generated_crate("null_hastype", &rust);
    let stdout = run_generated_crate("null_hastype", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn unsupported_prototype_method_is_rejected_with_e0406() {
    let diagnostics = diagnostics_of(
        "export function run(): number { \
             const a: number[] = [1, 2, 3]; \
             return Array.prototype.map.call(a, a).length; \
         }",
    );
    assert!(
        diagnostics
            .iter()
            .any(|(code, message)| code == "E0406" && message.contains("X.prototype.map.call")),
        "an unimplemented prototype method must be rejected with a readable E0406, got {diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|(code, _)| code == "E0300"),
        "the rejection must not be the backend `not implemented` error, got {diagnostics:?}"
    );
}

#[test]
fn number_radix_out_of_range_is_rejected_at_runtime() {
    const SOURCE: &str = "export function run(): string { return (5).toString(1); }";
    const MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        number_radix_error::run(&ctx)
    }));
    assert!(caught.is_err(), "an invalid radix must throw like JS does");
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ts_aot_runtime::__ts_aot_number_to_string(&ctx, value, Some(37))
        }));
        assert!(
            caught.is_err(),
            "a non-finite value with an out-of-range radix must throw"
        );
    }
    assert!(ts_aot_runtime::__ts_aot_number_to_string(&ctx, f64::NAN, Some(36))
        .as_valid()
        .is_some_and(|text| text == "NaN"));
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("number_radix_error", &rust);
    let stdout = run_generated_crate("number_radix_error", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_index_of_types_a_hoisted_needle_from_its_argument() {
    const SOURCE: &str = r"
export function fractionalLocal(): number {
    const a: i64[] = [1, 2, 3];
    const y = 1.5;
    return a.indexOf(y, 0);
}
export function wholeLocal(): number {
    const a: i64[] = [1, 2, 3];
    const y = 2.0;
    return a.indexOf(y, 0);
}
export function stringLocal(): number {
    const a: string[] = ['a', 'b', 'c'];
    const s = 'b';
    return a.indexOf(s, 1);
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
    assert_eq!(needle_static_ty::fractionalLocal(&ctx), -1.0);
    assert_eq!(needle_static_ty::wholeLocal(&ctx), 1.0);
    assert_eq!(needle_static_ty::stringLocal(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("needle_static_ty", &rust);
    let stdout = run_generated_crate("needle_static_ty", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_index_of_snapshots_the_needle_after_the_receiver() {
    const SOURCE: &str = r"
export function got(): number {
    let n = 1;
    function getArr(): number[] { n = 2; const out: number[] = [1, 2, 3]; return out; }
    return getArr().indexOf(n, 0);
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
    assert_eq!(needle_eval_order::got(&ctx), 1.0);
    println!("ok");
}
"#;
    let rust = rust_of(SOURCE);
    check_generated_crate("needle_eval_order", &rust);
    let stdout = run_generated_crate("needle_eval_order", &rust, MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn array_index_of_does_not_stage_a_needle_with_an_unknown_static_type() {
    const SOURCE: &str = r"
export function got(a: number[]): number {
  try {
    return 0;
  } catch (e) {
    return a.indexOf(e, 0);
  }
}
";
    let mir = mir_of(SOURCE);
    assert!(
        !mir.contains(": 0 = "),
        "a needle whose static type is unknown must not be staged into a temp typed with the \
         raw-0 sentinel, which resolves to the first interned type; got:\n{mir}"
    );
    assert!(
        mir.contains("array_index_of(local(0), local(1)"),
        "the catch binding must reach the runtime helper directly instead of through a foreign \
         typed temp; got:\n{mir}"
    );
}
