use ts_aot_driver::{CompileOptions, Driver, EmitStage, Severity};
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

const CTX_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_field_class_expr::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_field_holding_a_class_expression_keeps_its_static() {
    const SOURCE: &str = r"
export function got(): number {
  class H {
    C = class {
      static k: number = 5;
    };
  }
  const h = new H();
  return h.C.k;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_field_class_expr", &rust, CTX_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const BASIC_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_basic::got(&ctx), 1.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_instance_field_is_constructed_and_read() {
    const SOURCE: &str = r"
export function got(): number {
  class H {
    v: number = 1;
  }
  const h = new H();
  return h.v;
}
";
    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    assert_eq!(
        flat.matches("pubstruct").count(),
        1,
        "exactly one struct must be emitted for one local class; got:\n{rust}"
    );
    let stdout = run_generated_crate("local_classes_basic", &rust, BASIC_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const BLOCK_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_block_scope::got(&ctx), 7.0);
    println!("ok");
}
"#;

#[test]
fn a_class_declared_in_a_block_is_usable_inside_it() {
    const SOURCE: &str = r"
export function got(): number {
  let r: number = 0;
  {
    class H {
      v: number = 7;
    }
    const h = new H();
    r = h.v;
  }
  return r;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_block_scope", &rust, BLOCK_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_class_declared_in_a_block_is_not_visible_after_it() {
    const SOURCE: &str = r"
export function got(): number {
  {
    class H {
      v: number = 7;
    }
  }
  const h = new H();
  return h.v;
}
";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "the block-scoped class name must not resolve after the block; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0414"),
        "an out-of-block use of a block-scoped class must be a hard E0414 error, got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0500"
                && d.message.contains("statement form is not supported")),
        "a class declaration statement must be walked, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_class_declared_in_one_branch_is_not_visible_in_the_other() {
    const SOURCE: &str = r"
export function got(flag: boolean): number {
  if (flag) {
    class H {
      v: number = 3;
    }
    return new H().v;
  }
  const h = new H();
  return h.v;
}
";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a class declared in one branch must not resolve after the branch; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0414"),
        "the dangling use must be a hard E0414 error, got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E0500"
                && d.message.contains("statement form is not supported")),
        "the class declaration itself must be walked, got {:?}",
        out.diagnostics
    );
}

const MEMBERS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_members::got(&ctx), 21.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_supports_statics_getters_methods_and_new_arguments() {
    const SOURCE: &str = r"
export function got(): number {
  class Counter {
    static step: number = 3;
    v: number = 1;
    constructor(start: number) {
      this.v = start;
    }
    add(): number {
      this.v += Counter.step;
      return this.v;
    }
    get doubled(): number {
      return this.v * 2;
    }
  }
  const c = new Counter(4);
  c.add();
  return c.v + c.doubled;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_members", &rust, MEMBERS_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const EXTENDS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_extends::got(&ctx), 12.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_can_extend_another_local_class() {
    const SOURCE: &str = r"
export function got(): number {
  class Base {
    scale(n: number): number {
      return n * 3;
    }
  }
  class Derived extends Base {
    static bonus: number = 5;
    w: number = 1;
    total(): number {
      return this.scale(2) + this.w + Derived.bonus;
    }
  }
  const d = new Derived();
  return d.total();
}
";
    let rust = rust_of(SOURCE);
    assert_eq!(
        normalized(&rust).matches("pubstruct").count(),
        2,
        "both local classes must get their own struct; got:\n{rust}"
    );
    let stdout = run_generated_crate("local_classes_extends", &rust, EXTENDS_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SWITCH_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_switch::pick(&ctx, 1.0), 11.0);
    assert_eq!(local_classes_switch::pick(&ctx, 7.0), 22.0);
    println!("ok");
}
"#;

#[test]
fn a_class_declared_in_a_switch_case_builds_its_own_case_class() {
    const SOURCE: &str = r"
export function pick(n: number): number {
  switch (n) {
    case 1: {
      class H {
        v: number = 11;
      }
      return new H().v;
    }
    default: {
      class H {
        v: number = 22;
      }
      return new H().v;
    }
  }
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_switch", &rust, SWITCH_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SAME_NAME_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_same_name::got(&ctx), 12.0);
    println!("ok");
}
"#;

#[test]
fn two_local_classes_with_the_same_name_in_different_functions_stay_distinct() {
    const SOURCE: &str = r"
function a(): number {
  class H {
    v: number = 1;
  }
  return new H().v;
}

function b(): number {
  class H {
    v: number = 2;
  }
  return new H().v;
}

export function got(): number {
  return a() * 10 + b();
}
";
    let rust = rust_of(SOURCE);
    let flat = normalized(&rust);
    assert_eq!(
        flat.matches("pubstruct").count(),
        2,
        "the two same-named classes must emit two distinct structs; got:\n{rust}"
    );
    let stdout = run_generated_crate("local_classes_same_name", &rust, SAME_NAME_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const RETURN_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_return::stat(&ctx), 6.0);
    assert_eq!(local_classes_return::got(&ctx), 10.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_instance_can_be_returned_and_its_static_read_through_the_class_value() {
    const SOURCE: &str = r"
export function stat(): number {
  class H {
    static k: number = 6;
  }
  return H.k;
}

export function make() {
  class H {
    v: number = 4;
  }
  return new H();
}

export function got(): number {
  const h = make();
  return h.v + stat();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_return", &rust, RETURN_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const ANNOTATION_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_annotation::got(&ctx), 3.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_name_is_usable_as_a_type_annotation() {
    const SOURCE: &str = r"
export function got(): number {
  class Point {
    x: number = 0;
  }
  const p: Point = new Point();
  p.x = 3;
  return p.x;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_annotation", &rust, ANNOTATION_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const CLOSURE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_closure::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_is_constructible_from_a_closure_that_captures_it() {
    const SOURCE: &str = r"
export function got(): number {
  class H {
    v: number = 5;
  }
  const f = (): number => new H().v;
  return f();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_closure", &rust, CLOSURE_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SHADOW_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_shadow::got(&ctx), 2.0);
    assert_eq!(local_classes_shadow::module_value(&ctx), 1.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_shadows_a_module_level_class_inside_its_function() {
    const SOURCE: &str = r"
class H {
  v: number = 1;
}

export function got(): number {
  class H {
    v: number = 2;
  }
  const h = new H();
  return h.v;
}

export function module_value(): number {
  const h = new H();
  return h.v;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_shadow", &rust, SHADOW_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SIBLING_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_siblings::got(&ctx), 12.0);
    println!("ok");
}
"#;

#[test]
fn same_named_classes_in_sibling_blocks_do_not_mix() {
    const SOURCE: &str = r"
export function got(): number {
  let r: number = 0;
  {
    class H {
      v: number = 1;
    }
    r = new H().v;
  }
  {
    class H {
      v: number = 2;
    }
    r = r * 10 + new H().v;
  }
  return r;
}
";
    let rust = rust_of(SOURCE);
    assert_eq!(
        normalized(&rust).matches("pubstruct").count(),
        2,
        "sibling classes must not share one struct; got:\n{rust}"
    );
    let stdout = run_generated_crate("local_classes_siblings", &rust, SIBLING_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_class_inside_a_try_block_is_supported() {
    const SOURCE: &str = r"
export function got(): number {
  try {
    class H {
      v: number = 9;
    }
    const h = new H();
    return h.v;
  } catch (e) {
    return 0;
  }
}
";
    const TRY_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_try::got(&ctx), 9.0);
    println!("ok");
}
"#;
    let out = compile_module(SOURCE);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0500"),
        "a class statement in a try block must not be reported as unwalked, got {:?}",
        out.diagnostics
    );
    assert!(
        !out.has_errors(),
        "a local class in a try block must compile, got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert_eq!(
        normalized(&rust).matches("pubstruct").count(),
        1,
        "got:\n{rust}"
    );
    let stdout = run_generated_crate("local_classes_try", &rust, TRY_MAIN);
    assert!(
        stdout.contains("ok"),
        "the generated crate must build and `got()` must take the expected path (9) rather than \
         the catch path; got stdout:\n{stdout}"
    );
}

#[test]
fn a_class_in_a_branch_does_not_shadow_a_later_declaration_in_the_function() {
    const SOURCE: &str = r"
export function got(): number {
  if (true) {
    class H {
      v: number = 1;
    }
  }
  class H {
    w: number = 2;
  }
  const h = new H();
  return h.w;
}
";
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "the later declaration in the function scope must be the one in scope; got {:?}",
        out.diagnostics
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    assert_eq!(
        normalized(&rust).matches("pubstruct").count(),
        2,
        "both declarations must still get their own struct; got:\n{rust}"
    );
}

#[test]
fn a_local_class_declaration_is_not_an_unwalked_statement() {
    const SOURCE: &str = r"
export function got(): number {
  class H {
    v: number = 1;
  }
  const h = new H();
  return h.v;
}
";
    let out = compile_module(SOURCE);
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0500"),
        "a class declaration must be walked without E0500, got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "a plain local class must compile without errors, got {:?}",
        out.diagnostics
    );
}

const STATIC_EXPR_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_static_expr::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_expression_exposes_its_static_method_through_its_value() {
    const SOURCE: &str = r"
export function got(): number {
  const H = class {
    static make(): number {
      return 5;
    }
  };
  return H.make();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_static_expr", &rust, STATIC_EXPR_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const STATIC_DECL_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_static_decl::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_declaration_exposes_its_static_method_through_its_value() {
    const SOURCE: &str = r"
export function got(): number {
  class H {
    static make(): number {
      return 5;
    }
  }
  return H.make();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_static_decl", &rust, STATIC_DECL_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const CAPTURE_CTOR_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_capture_ctor::got(&ctx), 4.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_constructor_reads_an_enclosing_function_local() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 4;
  class H {
    constructor() {
      this.v = k;
    }
    v: number = 0;
  }
  return new H().v;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_capture_ctor", &rust, CAPTURE_CTOR_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const CAPTURE_METHOD_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_capture_method::got(&ctx), 4.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_method_reads_an_enclosing_function_local() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 4;
  class H {
    v: number = 0;
    read(): number {
      this.v = k;
      return this.v;
    }
  }
  return new H().read();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_capture_method", &rust, CAPTURE_METHOD_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const FIELD_INIT_LOOP_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_field_init_loop::got(&ctx), 3.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_field_initializer_reads_a_loop_local() {
    const SOURCE: &str = r"
export function got(): number {
  let r: number = 0;
  for (let i = 3; i < 4; i = i + 1) {
    class H {
      v: number = i;
    }
    r = new H().v;
  }
  return r;
}
";
    let out = compile_module(SOURCE);
    assert!(
        !out.has_errors(),
        "must compile through the full pipeline; got {:?}",
        out.diagnostics
    );
    assert!(
        !out.diagnostics.iter().any(|d| d.code.as_str() == "E0406"),
        "a local class field initializer reading a local must not be reported as unsupported"
    );
    let rust = out
        .rust_source
        .expect("emit-rust must populate rust_source");
    let stdout = run_generated_crate("local_classes_field_init_loop", &rust, FIELD_INIT_LOOP_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const NESTED_ANNOTATION_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_nested_annotation::got(&ctx), 4.0);
    println!("ok");
}
"#;

#[test]
fn a_method_closure_can_annotate_an_enclosing_local_class() {
    const SOURCE: &str = r"
export function got(): number {
  class H {
    v: number = 4;
  }
  class G {
    m(): number {
      const f = (): H => new H();
      return f().v;
    }
  }
  return new G().m();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "local_classes_nested_annotation",
        &rust,
        NESTED_ANNOTATION_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const NESTED_EXTENDS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_nested_extends::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_nested_closure_can_extend_an_enclosing_local_class() {
    const SOURCE: &str = r"
export function got(): number {
  class H {
    v: number = 3;
  }
  const f = (): number => {
    class G extends H {
      w: number = 2;
    }
    return new G().v + new G().w;
  };
  return f();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_nested_extends", &rust, NESTED_EXTENDS_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SUPER_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_super::got(&ctx), 3.0);
    println!("ok");
}
"#;

#[test]
fn a_derived_local_class_applies_its_parent_field_initializers() {
    const SOURCE: &str = r"
export function got(): number {
  class A {
    v: number = 1;
  }
  class B extends A {
    w: number = 2;
    constructor() {
      super();
    }
  }
  return new B().v + new B().w;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_super", &rust, SUPER_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const MODULE_SUPER_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(module_classes_super::got(&ctx), 3.0);
    println!("ok");
}
"#;

#[test]
fn a_derived_module_class_applies_its_parent_field_initializers() {
    const SOURCE: &str = r"
class A {
  v: number = 1;
}
class B extends A {
  w: number = 2;
  constructor() {
    super();
  }
}
export function got(): number {
  return new B().v + new B().w;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("module_classes_super", &rust, MODULE_SUPER_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const CONSTRUCTOR_INIT_ORDER_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_init_order::got(&ctx), 1234.0);
    println!("ok");
}
"#;

#[test]
fn a_derived_constructor_runs_its_field_initializers_after_the_base_constructor() {
    const SOURCE: &str = r"
let order: number = 0;

function side(n: number): number {
  order = order * 10 + n;
  return n;
}

export function got(): number {
  class A {
    x: number = side(1);
    constructor() {
      side(2);
    }
  }
  class B extends A {
    y: number = side(3);
    constructor() {
      super();
      side(4);
    }
  }
  new B();
  return order;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "local_classes_init_order",
        &rust,
        CONSTRUCTOR_INIT_ORDER_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const STATIC_CAPTURE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_static_capture::got(&ctx), 7.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_static_method_reads_an_enclosing_function_local() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 7;
  class H {
    static m(): number {
      return k;
    }
  }
  return H.m();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_static_capture", &rust, STATIC_CAPTURE_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const STATIC_CAPTURE_ARGS_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_static_capture_args::got(&ctx, 3.0, 4.0), 13.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_static_method_keeps_its_arguments_before_the_captures() {
    const SOURCE: &str = r"
export function got(base: number, extra: number): number {
  const k: number = 2;
  const bump: number = 3;
  class H {
    static m(a: number, b: number): number {
      return k * a + b + bump;
    }
  }
  return H.m(base, extra);
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "local_classes_static_capture_args",
        &rust,
        STATIC_CAPTURE_ARGS_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_class_static_method_that_captures_is_not_a_value() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 7;
  class H {
    static m(): number {
      return k;
    }
  }
  const f = H.m;
  return f();
}
";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "taking the value of a capturing static method must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.rust_source.is_none(),
        "no Rust may be emitted for a capturing static method used as a value; got:\n{}",
        out.rust_source.unwrap_or_default()
    );
}

const CLOSURE_CAPTURE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_closure_capture::got(&ctx), 6.0);
    println!("ok");
}
"#;

#[test]
fn a_closure_inside_a_local_class_method_reads_an_enclosing_function_local() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 6;
  class H {
    read(): number {
      const f = (): number => k;
      return f();
    }
  }
  return new H().read();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_closure_capture", &rust, CLOSURE_CAPTURE_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const CLOSURE_STATIC_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_closure_static::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_closure_inside_a_local_class_static_method_reads_an_enclosing_function_local() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 4;
  class H {
    static m(): number {
      const f = (): number => k + 1;
      return f();
    }
  }
  return H.m();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_closure_static", &rust, CLOSURE_STATIC_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const CAPTURE_CONST_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_capture_const::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_reads_a_const_local() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 5;
  class H {
    read(): number {
      return k;
    }
  }
  const h = new H();
  return h.read();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("local_classes_capture_const", &rust, CAPTURE_CONST_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const CAPTURE_LET_NOWRITE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_capture_let_nowrite::got(&ctx), 1.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_reads_a_let_local_that_is_never_assigned() {
    const SOURCE: &str = r"
export function got(): number {
  let k: number = 1;
  class H {
    read(): number {
      return k;
    }
  }
  const h = new H();
  return h.read();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "local_classes_capture_let_nowrite",
        &rust,
        CAPTURE_LET_NOWRITE_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_class_capture_is_refused_when_the_local_is_assigned_later() {
    const SOURCE: &str = r"
export function got(): number {
  let k: number = 1;
  class H {
    read(): number {
      return k;
    }
  }
  const h = new H();
  k = 5;
  return h.read();
}
";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a captured `let` that is assigned after the class declaration must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0419"),
        "the refusal must be E0419, got {:?}",
        out.diagnostics
    );
}

#[test]
fn a_local_class_closure_capture_is_refused_when_the_local_is_assigned_later() {
    const SOURCE: &str = r"
export function got(): number {
  let k: number = 1;
  class H {
    read(): number {
      const f = (): number => k;
      return f();
    }
  }
  const h = new H();
  k = 5;
  return h.read();
}
";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a closure that reads an assigned `let` inside a local class method must be refused; \
         got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0419"),
        "the refusal must be E0419, got {:?}",
        out.diagnostics
    );
}

const CAPTURE_STATIC_LATER_WRITE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_capture_static_later::got(&ctx), 5.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_static_method_reads_the_current_value_of_a_reassigned_local() {
    const SOURCE: &str = r"
export function got(): number {
  let k: number = 1;
  class H {
    static m(): number {
      return k;
    }
  }
  k = 5;
  return H.m();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "local_classes_capture_static_later",
        &rust,
        CAPTURE_STATIC_LATER_WRITE_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const FOR_LOOP_CAPTURE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(local_classes_for_loop_capture::got(&ctx), 6.0);
    println!("ok");
}
"#;

#[test]
fn a_local_class_inside_a_for_loop_reads_the_per_iteration_binding() {
    const SOURCE: &str = r"
export function got(): number {
  let total: number = 0;
  for (let i = 0; i < 3; i++) {
    class H {
      read(): number {
        return i * 2;
      }
    }
    const h = new H();
    total = total + h.read();
  }
  return total;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "local_classes_for_loop_capture",
        &rust,
        FOR_LOOP_CAPTURE_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_local_class_inside_a_for_loop_is_refused_when_the_body_assigns_the_local() {
    const SOURCE: &str = r"
export function got(): number {
  let total: number = 0;
  for (let i = 0; i < 3; i++) {
    class H {
      read(): number {
        return i * 2;
      }
    }
    const h = new H();
    i = i + 1;
    total = total + h.read();
  }
  return total;
}
";
    let out = compile_module(SOURCE);
    assert!(
        out.has_errors(),
        "a captured `for (let ...)` binding that the loop body assigns must be refused; got {:?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics.errors().any(|d| d.code.as_str() == "E0419"),
        "the refusal must be E0419, got {:?}",
        out.diagnostics
    );
}

const INHERITED_STATIC_COLLISION_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(inherited_static_collision::got(&ctx), 7);
    println!("ok");
}
"#;

#[test]
fn a_subclass_keeps_an_inherited_instance_method_next_to_a_static_one() {
    const SOURCE: &str = r"
class Base {
  drop(x: i32): i32 { return x; }
  static drop(x: i32): i32 { return x + 100; }
}
class Sub extends Base {}
export function got(): i32 {
  const s = new Sub();
  return s.drop(7);
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "inherited_static_collision",
        &rust,
        INHERITED_STATIC_COLLISION_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SUPER_INSTANCE_CLONE_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_instance_clone::got(&ctx), 7);
    println!("ok");
}
"#;

#[test]
fn a_super_call_reaches_the_instance_method_not_a_static_one_declared_first() {
    const SOURCE: &str = r"
class Base {
  static m(x: i32): i32 { return x + 100; }
  m(x: i32): i32 { return x; }
}
class Sub extends Base {
  m(x: i32): i32 { return super.m(x); }
}
export function got(): i32 {
  const s = new Sub();
  return s.m(7);
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("super_instance_clone", &rust, SUPER_INSTANCE_CLONE_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SUPER_DECOY_NAME_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_decoy_name::got(&ctx), 11.0);
    println!("ok");
}
"#;

#[test]
fn a_super_clone_ignores_a_decoy_class_whose_name_extends_the_owner_name() {
    const SOURCE: &str = r"
class Base {
  Sub_m(): number { return 1; }
}
class Base_Sub {
  m(): number { return 2; }
}
class Leaf extends Base {
  Sub_m(): number { return super.Sub_m() + 10; }
}
export function got(): number {
  return new Leaf().Sub_m();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("super_decoy_name", &rust, SUPER_DECOY_NAME_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const SUPER_COMPUTED_LITERAL_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(super_computed_literal::got(&ctx), 32.0);
    println!("ok");
}
"#;

#[test]
fn a_computed_super_call_accepts_parenthesized_and_template_literal_keys() {
    const SOURCE: &str = r"
class A {
  m(): number { return 1; }
}
class B extends A {
  m(): number { return 2; }
  viaParen(): number { return super[('m')]() + 10; }
  viaTemplate(): number { return super[`m`]() + 20; }
}
export function got(): number {
  const b = new B();
  return b.viaParen() + b.viaTemplate();
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("super_computed_literal", &rust, SUPER_COMPUTED_LITERAL_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const HOIST_DOUBLE_INIT_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoist_double_init::got(&ctx), 71.0);
    println!("ok");
}
"#;

#[test]
fn a_constructor_hoisted_init_is_not_reapplied_at_a_two_level_allocation() {
    const SOURCE: &str = r"
let bumpCalls: number = 0;
function bump(): number { bumpCalls = bumpCalls + 1; return 7; }
export function got(): number {
  const k: number = 3;
  class A {}
  class B extends A {
    x: number = bump();
    w: number = k;
    constructor() { super(); }
  }
  class D extends B { constructor() { super(); } }
  const d = new D();
  return d.x * 10 + bumpCalls;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate("hoist_double_init", &rust, HOIST_DOUBLE_INIT_MAIN);
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

const HOIST_CHAIN_REDEFINITION_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(hoist_chain_redefinition::got(&ctx), 2.0);
    println!("ok");
}
"#;

const SPLICED_INIT_SINGLE_RUN_MAIN: &str = r#"
use ts_aot_runtime::{Bump, Ctx, Host};

struct NoHost;
impl Host for NoHost {}

fn main() {
    let host = NoHost;
    let bump = Bump::new();
    let ctx = Ctx { arena: &bump, host: &host };
    assert_eq!(spliced_init_single_run::got(&ctx), 71.0);
    println!("ok");
}
"#;

#[test]
fn a_class_whose_only_field_init_was_spliced_keeps_no_stale_allocation_init() {
    const SOURCE: &str = r"
let calls: number = 0;
function bump(): number { calls = calls + 1; return 7; }
export function got(): number {
  class A {}
  class B extends A { x: number = bump(); constructor() { super(); } }
  return new B().x * 10 + calls;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "spliced_init_single_run",
        &rust,
        SPLICED_INIT_SINGLE_RUN_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}

#[test]
fn a_four_class_chain_keeps_the_most_derived_field_initializer() {
    const SOURCE: &str = r"
export function got(): number {
  const k: number = 9;
  class A { constructor() {} }
  class B extends A { z: number = 1; constructor() { super(); } }
  class D extends B { z: number = 2; y: number = 3; constructor() { super(); } }
  class E extends D { y: number = k; constructor() { super(); } }
  return new E().z;
}
";
    let rust = rust_of(SOURCE);
    let stdout = run_generated_crate(
        "hoist_chain_redefinition",
        &rust,
        HOIST_CHAIN_REDEFINITION_MAIN,
    );
    assert!(stdout.contains("ok"), "got stdout:\n{stdout}");
}
