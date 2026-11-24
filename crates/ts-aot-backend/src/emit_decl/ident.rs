use proc_macro2::{Ident, Span};

use std::collections::HashSet;

use ts_aot_core::{Atom, LocalId};

pub(super) fn ident_from(atom: &Atom) -> Ident {
    let raw = atom.as_str();
    let sanitized = sanitize_ident(raw);
    Ident::new(&sanitized, Span::call_site())
}

pub(super) fn unique_local_ident(
    atom: &Atom,
    forbidden: &HashSet<Ident>,
    taken: &mut HashSet<Ident>,
) -> Ident {
    let mut candidate = ident_from(atom);
    if candidate == "_" {
        return candidate;
    }
    while forbidden.contains(&candidate) || !taken.insert(candidate.clone()) {
        candidate = Ident::new(&format!("{candidate}_"), Span::call_site());
    }
    candidate
}

pub(super) fn bound_local_ident(
    atom: &Atom,
    id: LocalId,
    forbidden: &HashSet<Ident>,
    taken: &mut HashSet<Ident>,
) -> Ident {
    let candidate = unique_local_ident(atom, forbidden, taken);
    if candidate != "_" {
        return candidate;
    }
    let mut candidate = Ident::new(&format!("__tmp{}", id.raw()), Span::call_site());
    while forbidden.contains(&candidate) || !taken.insert(candidate.clone()) {
        candidate = Ident::new(&format!("{candidate}_"), Span::call_site());
    }
    candidate
}

pub(super) fn sanitize_ident(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for (i, ch) in raw.chars().enumerate() {
        let valid = ch == '_' || ch.is_ascii_alphanumeric();
        if valid {
            if i == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    if is_rust_keyword(&out) {
        out.push('_');
    }
    out
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "try"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}
