use super::Dumper;
use super::dump_sym;
use super::expr;

use crate::decl::{MirDecl, MirFunctionDecl, MirGlobalDecl, MirParam, MirStructDecl};

pub(crate) fn dump_decl(decl: &MirDecl, d: &mut Dumper) {
    match decl {
        MirDecl::Function(f) => dump_function(f, d),
        MirDecl::Struct(s) => dump_struct(s, d),
        MirDecl::Global(g) => dump_global(g, d),
    }
}

fn dump_function(f: &MirFunctionDecl, d: &mut Dumper) {
    d.write(&format!(
        "fn #{} {}({}) -> {}",
        f.id.raw(),
        dump_sym(&f.name, d),
        f.params
            .iter()
            .map(|p| dump_param(p, d))
            .collect::<Vec<_>>()
            .join(", "),
        f.ret.raw(),
    ));
    if let Some(throws) = f.throws {
        d.write(&format!(" throws {}", throws.raw()));
    }
    d.write(&format!(" {:?}", f.kind));
    if f.effects.can_throw || f.effects.is_async {
        d.write(&format!(" [{:?}]", f.effects));
    }
    d.write("\n");
    expr::dump_body(&f.body, d);
}

fn dump_param(p: &MirParam, d: &Dumper) -> String {
    format!("{}: {}", dump_sym(&p.name, d), p.ty.raw())
}

fn dump_struct(s: &MirStructDecl, d: &mut Dumper) {
    d.write(&format!("struct {} {{", dump_sym(&s.name, d)));
    if !s.fields.is_empty() || !s.methods.is_empty() {
        d.write("\n");
        d.push();
        for field in &s.fields {
            d.line(&format!(
                "{}{}: {},",
                if field.mutable { "var " } else { "" },
                dump_sym(&field.name, d),
                field.ty.raw(),
            ));
        }
        for method in &s.methods {
            dump_function(method, d);
        }
        d.pop();
    }
    d.line("}");
}

fn dump_global(g: &MirGlobalDecl, d: &mut Dumper) {
    d.line(&format!(
        "global {}: {} (mut: {})",
        dump_sym(&g.name, d),
        g.ty.raw(),
        g.mutable,
    ));
    if let Some(init) = &g.init {
        let mut tmp = Dumper::new();
        super::expr::dump_expr_inline(init, &mut tmp);
        d.line(&format!("  = {}", tmp.buf.trim_end()));
    }
}
