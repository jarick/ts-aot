use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{Declaration, Expression, Program, Statement, TSType, TSTypeName};
use oxc_span::GetSpan;
use ts_aot_core::{
    Atom, Diagnostic, DiagnosticBag, LocalId, Span as CoreSpan, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirExport, HirExpr, HirFunction, HirParam, HirProgram, HirStmt, Visitor,
    walk_expr, walk_stmt,
};

use crate::scope::{BodyScope, ClosureBody};
use crate::type_resolver::TypeParamMap;
use crate::util::core_span_from_oxc;

const ALIAS_CYCLE_CODE: &str = "E0401";
const UNSUPPORTED_BODY_CODE: &str = "E0500";
pub(crate) const NARROWED_UPDATE_MESSAGE: &str = "a compound update (`x += v`, `x++`) of a narrowed optional variable is only \
     supported as the whole expression of a statement or of a `let`/`const` initializer: inside \
     a larger expression the new value could not be written back to the variable's storage";
pub(crate) struct NarrowedUpdate {
    pub(crate) narrowed_id: LocalId,
    pub(crate) span: oxc_span::Span,
}
const RESERVED_NAME_MAIN_CODE: &str = "E0510";
const RESERVED_NAME_TLA_MAIN_CODE: &str = "E0511";
const RESERVED_NAME_TLA_GENERATED_CODE: &str = "E0512";
pub(crate) const ESCAPING_THIS_CAPTURE_CODE: &str = "E0513";
pub(crate) const ESCAPING_THIS_CAPTURE_MESSAGE: &str = "an arrow function that captures `this` escapes its method here, so it would \
     observe a snapshot copy of the receiver taken when the method was entered instead of the \
     caller's live receiver; call the arrow inside the method, or pass the values it needs in as \
     parameters/arguments";

fn reserved_name_reason(name: &str) -> Option<(&'static str, &'static str)> {
    if name == "main" {
        return Some((RESERVED_NAME_MAIN_CODE, "synthesized entry point"));
    }
    if name == "__ts_aot_tla_main" {
        return Some((RESERVED_NAME_TLA_MAIN_CODE, "mangled TLA main entry name"));
    }
    if is_tla_main_reserved_name(name) {
        return Some((
            RESERVED_NAME_TLA_GENERATED_CODE,
            "generated TLA main name namespace",
        ));
    }
    None
}
const TLA_MAIN_RESERVED_PREFIX: &str = "__tla_main_";
const TLA_MAIN_FIXED_NAME: &str = "__tla_main_0";

pub(crate) fn is_tla_main_reserved_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix(TLA_MAIN_RESERVED_PREFIX) else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
}

fn fixed_tla_main_name() -> Atom {
    Atom::from(TLA_MAIN_FIXED_NAME)
}

struct AwaitFinder {
    found: bool,
}

impl Visitor for AwaitFinder {
    fn visit_expr(&mut self, expr: &HirExpr) {
        if self.found {
            return;
        }
        if matches!(expr, HirExpr::Await { .. }) {
            self.found = true;
            return;
        }
        walk_expr(self, expr);
    }
}

fn body_contains_await(body: &[HirStmt]) -> bool {
    let mut finder = AwaitFinder { found: false };
    for stmt in body {
        if finder.found {
            break;
        }
        finder.visit_stmt(stmt);
    }
    finder.found
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum ClassValueFieldBase {
    Local(u32, LocalId),
    Global(Atom),
}

pub(crate) type PendingClassValueFieldRead = (
    TypeId,
    Atom,
    Vec<TypeId>,
    oxc_span::Span,
    Option<ClassValueFieldBase>,
);

pub(crate) type ClassCaptureFields = HashMap<TypeId, Vec<(Atom, Atom, LocalId, TypeId, CoreSpan)>>;

#[derive(Clone, Copy, Default)]
pub(crate) struct ClassValuePoisonFacts {
    pub(crate) definitely_falsy: bool,
    pub(crate) definitely_nullish: bool,
}

pub(crate) const MERGED_CLASS_VALUE_BRANCH: u32 = u32::MAX;

#[derive(Clone, Default)]
pub(crate) struct ClassValuePoisonBases {
    pub(crate) bases: HashMap<ClassValueFieldBase, ClassValuePoisonFacts>,
    pub(crate) fact_depths: HashMap<ClassValueFieldBase, u32>,
    pub(crate) fact_branches: HashMap<ClassValueFieldBase, Option<u32>>,
    pub(crate) unknown_base: bool,
}

#[derive(Clone, Default)]
pub(crate) struct ClassValueFieldState {
    pub(crate) classes: HashMap<TypeId, HashMap<Atom, Vec<TypeId>>>,
    pub(crate) unknown: HashMap<TypeId, HashSet<Atom>>,
    pub(crate) bases: HashMap<TypeId, HashMap<Atom, ClassValuePoisonBases>>,
}

#[derive(Clone, Copy)]
pub(crate) struct ClassValueExit {
    pub(crate) position: usize,
    pub(crate) depth: u32,
    pub(crate) leaves_a_loop: bool,
}

#[derive(Default)]
pub(crate) struct ClassValueLoopFrame {
    pub(crate) breakable: u32,
    pub(crate) exits: Vec<ClassValueExit>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwitchCaseEventKind {
    Use,
    Decl,
    VarDecl,
}

#[derive(Clone)]
pub(crate) struct SwitchCaseEvent {
    pub(crate) local: LocalId,
    pub(crate) kind: SwitchCaseEventKind,
    pub(crate) name: Option<Atom>,
    pub(crate) shadowed: Option<LocalId>,
}

#[derive(Default)]
pub(crate) struct SwitchCaseFrame {
    pub(crate) scope_depth: usize,
    pub(crate) current: usize,
    pub(crate) outer: HashMap<Atom, LocalId>,
    pub(crate) events: Vec<Vec<SwitchCaseEvent>>,
}

#[derive(Default)]
pub(crate) struct ClassValueBranchFrame {
    pub(crate) start: usize,
    pub(crate) breakable: u32,
    pub(crate) exits: Vec<ClassValueExit>,
}

#[derive(Default)]
pub(crate) struct ClassValueBranchRecord {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) exits: Vec<ClassValueExit>,
}

pub(crate) fn state_holds_class_value(
    state: &ClassValueFieldState,
    owner_ty: TypeId,
    field: &Atom,
    base: &ClassValueFieldBase,
) -> bool {
    if state
        .unknown
        .get(&owner_ty)
        .is_some_and(|fields| fields.contains(field))
    {
        return false;
    }
    if state
        .classes
        .get(&owner_ty)
        .and_then(|fields| fields.get(field))
        .is_none_or(Vec::is_empty)
    {
        return false;
    }
    state
        .bases
        .get(&owner_ty)
        .and_then(|fields| fields.get(field))
        .is_none_or(|bases| !bases.unknown_base && !bases.bases.contains_key(base))
}

impl ClassValuePoisonBases {
    pub(crate) fn merge(&mut self, other: &Self) {
        self.unknown_base |= other.unknown_base;
        for (base, facts) in &other.bases {
            if let Some(existing) = self.bases.get_mut(base) {
                existing.definitely_falsy &= facts.definitely_falsy;
                existing.definitely_nullish &= facts.definitely_nullish;
            } else {
                self.bases.insert(base.clone(), *facts);
            }
        }
        for (base, depth) in &other.fact_depths {
            if let Some(existing) = self.fact_depths.get_mut(base) {
                *existing = (*existing).min(*depth);
            } else {
                self.fact_depths.insert(base.clone(), *depth);
            }
        }
        for (base, branch) in &other.fact_branches {
            if let Some(existing) = self.fact_branches.get_mut(base) {
                if *existing != *branch {
                    *existing = Some(MERGED_CLASS_VALUE_BRANCH);
                }
            } else {
                self.fact_branches
                    .insert(base.clone(), Some(MERGED_CLASS_VALUE_BRANCH));
            }
        }
    }

    fn established_branch_reaches(&self, base: &ClassValueFieldBase, active: &[u32]) -> bool {
        match self.fact_branches.get(base).copied().flatten() {
            None => true,
            Some(branch) => active.is_empty() || active.contains(&branch),
        }
    }

    pub(crate) fn restorable(
        &self,
        constraint: ClassValuePoisonConstraint,
        depth: u32,
        active_branches: &[u32],
    ) -> bool {
        if self.unknown_base {
            return constraint == ClassValuePoisonConstraint::Always;
        }
        let requires_dominance = constraint == ClassValuePoisonConstraint::Always;
        self.bases.iter().all(|(base, facts)| {
            let established = self.fact_depths.get(base).copied().unwrap_or(0);
            depth <= established
                && (!requires_dominance || self.established_branch_reaches(base, active_branches))
                && constraint.holds(*facts)
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassValuePoisonConstraint {
    Always,
    DefinitelyFalsy,
    DefinitelyNullish,
}

impl ClassValuePoisonConstraint {
    fn holds(self, facts: ClassValuePoisonFacts) -> bool {
        match self {
            Self::Always => true,
            Self::DefinitelyFalsy => facts.definitely_falsy,
            Self::DefinitelyNullish => facts.definitely_nullish,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ClassValuePoisonWrite {
    pub(crate) owner_ty: TypeId,
    pub(crate) field: Atom,
    pub(crate) base: ClassValueFieldBase,
    pub(crate) facts: ClassValuePoisonFacts,
    pub(crate) plain_assign: bool,
    pub(crate) depth: u32,
    pub(crate) holds_class_value: bool,
    pub(crate) branch: Option<u32>,
}

#[derive(Clone)]
pub(crate) struct StaticMethodCapture {
    pub(crate) source: Atom,
    pub(crate) local: LocalId,
    pub(crate) body_id: u32,
    pub(crate) ty: TypeId,
}

#[derive(Clone)]
pub(crate) struct PrivateMember {
    pub(crate) lowered: Atom,
    pub(crate) is_static: bool,
    pub(crate) brand_is_field: bool,
}

pub(crate) struct PrivateClassFrame {
    pub(crate) ty: TypeId,
    pub(crate) class_name: Atom,
    pub(crate) members: HashMap<Atom, PrivateMember>,
}

pub(crate) fn rust_ident_escape(raw: &str) -> String {
    ts_aot_core::escape_rust_ident(raw)
}

pub(crate) fn private_member_hir_name(class_ty: TypeId, name: &str) -> Atom {
    Atom::from(format!(
        "__ps_{}_{}",
        class_ty.raw(),
        rust_ident_escape(name)
    ))
}

#[derive(Clone)]
pub(crate) struct LocalGeneratorAlias {
    pub(crate) lifted: Atom,
    pub(crate) call_ty: TypeId,
    pub(crate) fn_ty: TypeId,
}

#[derive(Clone)]
pub(crate) struct CaptureRecord {
    pub(crate) binding: LocalId,
    pub(crate) source: Atom,
    pub(crate) span: CoreSpan,
    pub(crate) construction_only: bool,
    pub(crate) mutable_reference: bool,
}

#[derive(Default)]
pub(crate) struct CaptureCheckFrame {
    pub(crate) captures: Vec<CaptureRecord>,
    pub(crate) inherited: Vec<(Atom, CoreSpan)>,
}

pub(crate) struct PendingClosureCaptures {
    pub(crate) enclosing_body_id: u32,
    pub(crate) closure_depth: u32,
    pub(crate) enclosing_bindings: HashMap<String, (LocalId, TypeId)>,
    pub(crate) required: Vec<(LocalId, TypeId, CoreSpan)>,
}

impl PendingClosureCaptures {
    pub(crate) fn require(&mut self, local: LocalId, ty: TypeId, span: CoreSpan) {
        if self.required.iter().any(|(id, _, _)| *id == local) {
            return;
        }
        self.required.push((local, ty, span));
    }
}

impl CaptureCheckFrame {
    pub(crate) fn record(&mut self, record: CaptureRecord) {
        if let Some(entry) = self
            .captures
            .iter_mut()
            .find(|entry| entry.binding.raw() == record.binding.raw())
        {
            entry.construction_only = entry.construction_only && record.construction_only;
            return;
        }
        self.captures.push(record);
    }

    pub(crate) fn inherit(&mut self, source: Atom, span: CoreSpan) {
        if self
            .inherited
            .iter()
            .any(|(name, _)| name.as_str() == source.as_str())
        {
            return;
        }
        self.inherited.push((source, span));
    }
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SkeletonBuilder {
    pub(crate) source_len: u32,
    pub(crate) next_generic_param: u32,
    pub(crate) next_anon_class_id: u32,
    pub(crate) next_closure_id: u32,
    pub(crate) next_class_struct_id: u32,
    pub(crate) resolved_aliases: HashMap<String, TypeId>,
    pub(crate) class_types: HashMap<String, TypeId>,
    pub(crate) class_value_globals: HashMap<String, Vec<TypeId>>,
    pub(crate) module_lets: HashSet<String>,
    pub(crate) module_vars: HashSet<String>,
    pub(super) mutable_locals: HashSet<(u32, LocalId)>,
    pub(super) pending_class_value_global_reads: Vec<PendingGlobalRead>,
    pub(super) pending_global_receiver_calls: Vec<PendingGlobalReceiverCall>,
    pub(super) unknown_class_value_globals: HashSet<String>,
    pub(super) unknown_class_value_locals: HashMap<LocalId, String>,
    pub(super) capture_class_values: Vec<HashMap<String, CaptureClassValue>>,
    pub(super) pending_capture_class_value_reads: Vec<Vec<PendingCaptureRead>>,
    pub(super) loop_frames: Vec<Vec<PendingLoopRead>>,
    pub(super) module_init_depth: u32,
    pub(crate) class_value_locals: HashMap<ts_aot_core::LocalId, Vec<TypeId>>,
    pub(crate) class_value_fields: HashMap<TypeId, HashMap<Atom, Vec<TypeId>>>,
    pub(crate) class_value_field_initials: HashMap<TypeId, HashMap<Atom, Vec<TypeId>>>,
    pub(crate) unknown_class_value_fields: HashMap<TypeId, HashSet<Atom>>,
    pub(crate) fresh_field_bases: HashSet<ClassValueFieldBase>,
    pub(crate) fresh_field_classes: HashMap<(ClassValueFieldBase, TypeId, Atom), Vec<TypeId>>,
    pub(crate) class_value_field_poison_bases:
        HashMap<TypeId, HashMap<Atom, ClassValuePoisonBases>>,
    pub(crate) class_value_poison_writes: Vec<ClassValuePoisonWrite>,
    pub(crate) class_value_exits: Vec<ClassValueExit>,
    pub(crate) class_value_loop_frames: Vec<ClassValueLoopFrame>,
    pub(super) constant_int_locals: HashMap<LocalId, i64>,
    pub(crate) switch_case_frames: Vec<SwitchCaseFrame>,
    pub(crate) class_value_branch_frames: Vec<ClassValueBranchFrame>,
    pub(crate) class_value_branch_records: Vec<ClassValueBranchRecord>,
    pub(crate) class_value_branch_ids: Vec<u32>,
    pub(crate) class_value_breakable_depth: u32,
    pub(crate) next_class_value_branch_id: u32,
    pub(crate) class_value_poisoned_locals: HashMap<(u32, LocalId), ClassValuePoisonFacts>,
    pub(crate) class_value_poisoned_globals: HashMap<String, ClassValuePoisonFacts>,
    pub(super) pending_class_value_field_reads: Vec<PendingClassValueFieldRead>,
    pub(crate) class_value_tags: HashMap<TypeId, i64>,
    pub(crate) is_generator_stack: Vec<bool>,
    pub(crate) type_param_stack: Vec<TypeParamMap>,
    pub(crate) in_static_method: bool,
    pub(crate) current_super_class: Option<Atom>,
    pub(crate) module: bool,
    pub(crate) static_field_globals: HashMap<TypeId, HashMap<Atom, (Atom, TypeId)>>,
    pub(crate) static_method_globals: HashMap<TypeId, HashMap<Atom, (Atom, TypeId, TypeId)>>,
    pub(crate) static_method_captures: HashMap<Atom, Vec<StaticMethodCapture>>,
    pub(crate) pending_static_captures: HashMap<(TypeId, Atom), Vec<(Atom, StaticMethodCapture)>>,
    pub(crate) capture_check_frames: Vec<CaptureCheckFrame>,
    pub(crate) class_capture_fields: ClassCaptureFields,
    pub(crate) class_decl_body: HashMap<TypeId, u32>,
    pub(crate) pending_closure_captures: Vec<PendingClosureCaptures>,
    pub(crate) body_depth: u32,
    pub(super) in_callee_position: bool,
    pub(crate) strict: bool,
    pub(crate) tla_main_body: Vec<TlaMainItem>,
    pub(crate) tla_main_name: Atom,
    pub(crate) tla_top_level_scope: BodyScope,
    pub(super) top_level_function_spans: HashMap<Atom, CoreSpan>,
    pub(super) narrowed_stack: Vec<HashMap<LocalId, (LocalId, TypeId)>>,
    pub(super) captured_stack: Vec<HashSet<LocalId>>,
    pub(super) declared_names_stack: Vec<HashSet<String>>,
    pub(super) generator_alias_stack: Vec<HashMap<String, LocalGeneratorAlias>>,
    pub(super) suppress_narrowing: u32,
    pub(super) conditional_depth: u32,
    pub(super) try_body_skips: Option<bool>,
    pub(super) own_statement_expr: bool,
    pub(super) pending_narrowed_updates: Vec<NarrowedUpdate>,
    pub(crate) array_literal_hints: HashMap<u32, TypeId>,
    pub(crate) class_method_rets: HashMap<(TypeId, Atom, bool), TypeId>,
    pub(crate) private_class_stack: Vec<PrivateClassFrame>,
    pub(super) hoisted_fn_decls: Vec<Vec<HirStmt>>,
    pub(super) current_class_ty: Option<TypeId>,
    pub(super) current_class_own_methods: Vec<Atom>,
    pub(super) this_local_stack: Vec<Option<LocalId>>,
    pub(super) this_capture_locals: Vec<HashSet<LocalId>>,
}

pub(crate) struct SkeletonEnv<'b> {
    pub(crate) types: &'b mut TypeTable,
    pub(crate) diagnostics: &'b mut DiagnosticBag,
    pub(crate) program: &'b mut HirProgram,
    pub(crate) next_destructured_id: Cell<u32>,
    pub(crate) capture_types: Vec<HashMap<String, TypeId>>,
}

impl SkeletonEnv<'_> {
    pub(crate) fn capture_type(&self, name: &str) -> Option<TypeId> {
        self.capture_types
            .iter()
            .rev()
            .find_map(|frame| frame.get(name).copied())
    }
}

impl SkeletonEnv<'_> {
    pub(crate) fn error_ty(&mut self) -> TypeId {
        self.types.intern(&Type::Error)
    }

    pub(crate) fn bigint_ty(&mut self) -> TypeId {
        self.types.intern(&Type::BigInt)
    }

    pub(crate) fn record_export(&mut self, name: &str) {
        self.program.exports.push(HirExport {
            name: Atom::from(name),
            alias: None,
        });
    }

    pub(crate) fn report_unwalked(&mut self, message: &str, span: oxc_span::Span) {
        self.diagnostics.push(Diagnostic::warning(
            UNSUPPORTED_BODY_CODE,
            message,
            core_span_from_oxc(span),
        ));
    }

    pub(crate) fn report_narrowed_update(&mut self, span: oxc_span::Span) {
        self.report_unsupported(UNSUPPORTED_BODY_CODE, NARROWED_UPDATE_MESSAGE, span);
    }
}

pub(crate) enum TlaMainItem {
    Expr(HirExpr),
}

pub(super) struct PendingGlobalRead {
    pub(super) name: String,
    pub(super) seen: Vec<TypeId>,
    pub(super) span: oxc_span::Span,
}

pub(super) const INDEX_PATH_COMPONENT: &str = "[]";

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum ReceiverRef {
    Global(String),
    StaticField {
        class: String,
        field: String,
    },
    Path {
        root: String,
        root_id: LocalId,
        body_id: u32,
        fields: Vec<String>,
    },
}

impl ReceiverRef {
    pub(super) fn describe(&self) -> String {
        match self {
            ReceiverRef::Global(name) => name.clone(),
            ReceiverRef::StaticField { class, field } => format!("{class}.{field}"),
            ReceiverRef::Path { root, fields, .. } if fields.is_empty() => root.clone(),
            ReceiverRef::Path { root, fields, .. } => format!("{root}.{}", fields.join(".")),
        }
    }

    fn root_local(&self) -> Option<LocalId> {
        match self {
            ReceiverRef::Path { root_id, .. } => Some(*root_id),
            ReceiverRef::Global(_) | ReceiverRef::StaticField { .. } => None,
        }
    }

    fn path_fields(&self) -> Option<&[String]> {
        match self {
            ReceiverRef::Path { fields, .. } => Some(fields.as_slice()),
            ReceiverRef::Global(_) | ReceiverRef::StaticField { .. } => None,
        }
    }
}

pub(super) struct PendingGlobalReceiverCall {
    pub(super) receiver: ReceiverRef,
    pub(super) owner_ty: TypeId,
    pub(super) method: String,
    pub(super) direct_write: bool,
    pub(super) opaque: bool,
    pub(super) call_sites: Vec<ReceiverCallSite>,
    pub(super) arg_bindings: Vec<ArgumentBody>,
    pub(super) span: oxc_span::Span,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum ReceiverCallTarget {
    Function(String),
    Method(TypeId, String),
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum ArgumentBody {
    Closures(Vec<ClosureBody>),
    Function(String),
    Method(TypeId, String),
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct ReceiverCallSite {
    pub(super) target: ReceiverCallTarget,
    pub(super) args: Vec<ArgumentBody>,
    pub(super) owner_path: Option<Vec<String>>,
}

pub(crate) fn may_hold_a_callable(init: &HirExpr) -> bool {
    !matches!(
        init,
        HirExpr::Unit(_)
            | HirExpr::Bool(..)
            | HirExpr::Int(..)
            | HirExpr::Float(..)
            | HirExpr::String(..)
            | HirExpr::Null(_)
            | HirExpr::Undefined(_)
            | HirExpr::BigInt { .. }
            | HirExpr::RegExp { .. }
            | HirExpr::ArrayLiteral { .. }
            | HirExpr::ObjectLiteral { .. }
            | HirExpr::StructLiteral { .. }
            | HirExpr::New { .. }
            | HirExpr::DefaultConstruct { .. }
    )
}

pub(crate) fn closure_bodies_of(expr: &HirExpr) -> Option<Vec<ClosureBody>> {
    fn collect(expr: &HirExpr, out: &mut Vec<ClosureBody>) -> bool {
        match expr {
            HirExpr::Closure { params, body, .. } => {
                out.push(ClosureBody {
                    params: params.clone(),
                    body: body.clone(),
                });
                true
            }
            HirExpr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                let then_ok = collect(then_branch, out);
                let else_ok = collect(else_branch, out);
                then_ok && else_ok
            }
            _ => false,
        }
    }
    let mut out = Vec::new();
    collect(expr, &mut out).then_some(out)
}

pub(super) struct ReceiverWriteScan<'c> {
    pub(super) receiver: ReceiverRef,
    pub(super) direct_write: bool,
    pub(super) opaque: bool,
    pub(super) call_sites: Vec<ReceiverCallSite>,
    local_closures: HashMap<LocalId, Vec<ClosureBody>>,
    local_aliases: HashMap<LocalId, Vec<ReceiverCallTarget>>,
    unknown_locals: HashSet<LocalId>,
    expanded_locals: HashSet<(LocalId, Vec<ArgumentBody>)>,
    class_types: &'c HashMap<String, TypeId>,
    shadowed_locals: HashMap<String, Option<TypeId>>,
    this_local: Option<LocalId>,
    this_place: Option<Vec<String>>,
    root_in_scope: bool,
}

impl<'c> ReceiverWriteScan<'c> {
    pub(super) fn new(
        receiver: ReceiverRef,
        local_closures: HashMap<LocalId, Vec<ClosureBody>>,
        class_types: &'c HashMap<String, TypeId>,
        shadowed_locals: HashMap<String, Option<TypeId>>,
    ) -> Self {
        Self {
            receiver,
            direct_write: false,
            opaque: false,
            call_sites: Vec::new(),
            local_closures,
            local_aliases: HashMap::new(),
            unknown_locals: HashSet::new(),
            expanded_locals: HashSet::new(),
            class_types,
            shadowed_locals,
            this_local: None,
            this_place: None,
            root_in_scope: false,
        }
    }

    pub(super) fn enter_receiver_scope(
        &mut self,
        this_local: Option<LocalId>,
        this_place: Option<Vec<String>>,
        root_in_scope: bool,
    ) {
        self.this_local = this_local;
        self.this_place = this_place;
        self.root_in_scope = root_in_scope;
    }

    pub(super) fn enter_method(&mut self, params: &[HirParam], place: Option<Vec<String>>) {
        let this_local = params
            .iter()
            .find(|param| param.name.as_str() == "this")
            .map(|param| param.id);
        self.enter_receiver_scope(this_local, place, false);
    }

    fn forget_expansions(&mut self, id: LocalId) {
        self.expanded_locals.retain(|(local, _)| *local != id);
    }

    fn reset_local(&mut self, id: LocalId) {
        self.local_closures.remove(&id);
        self.local_aliases.remove(&id);
        self.forget_expansions(id);
        self.unknown_locals.remove(&id);
    }

    fn poison_local(&mut self, id: LocalId) {
        self.reset_local(id);
        self.unknown_locals.insert(id);
    }

    fn add_local_closure(&mut self, id: LocalId, body: ClosureBody) {
        if self.unknown_locals.contains(&id) {
            return;
        }
        let bodies = self.local_closures.entry(id).or_default();
        if !bodies.contains(&body) {
            bodies.push(body);
        }
        self.forget_expansions(id);
    }

    fn add_local_alias(&mut self, id: LocalId, target: ReceiverCallTarget) {
        if self.unknown_locals.contains(&id) {
            return;
        }
        let aliases = self.local_aliases.entry(id).or_default();
        if !aliases.contains(&target) {
            aliases.push(target);
        }
        self.forget_expansions(id);
    }

    fn bind_local(&mut self, id: LocalId, arg: &ArgumentBody) {
        match arg {
            ArgumentBody::Closures(bodies) => {
                for body in bodies {
                    self.add_local_closure(id, body.clone());
                }
            }
            ArgumentBody::Function(name) => {
                self.add_local_alias(id, ReceiverCallTarget::Function(name.clone()));
            }
            ArgumentBody::Method(ty, name) => {
                self.add_local_alias(id, ReceiverCallTarget::Method(*ty, name.clone()));
            }
            ArgumentBody::Unknown => self.poison_local(id),
        }
        self.forget_expansions(id);
    }

    pub(super) fn bind_params(&mut self, params: &[HirParam], args: &[ArgumentBody]) {
        for (param, arg) in params
            .iter()
            .filter(|param| param.name.as_str() != "this")
            .zip(args)
        {
            self.bind_local(param.id, arg);
        }
    }

    fn is_class_type(&self, ty: TypeId) -> bool {
        self.class_types.values().any(|class_ty| *class_ty == ty)
    }

    fn owner_type(&self, owner: &HirExpr) -> Option<TypeId> {
        if let HirExpr::Global { name, .. } = owner {
            if let Some(shadowed) = self.shadowed_locals.get(name.as_str()) {
                return shadowed.filter(|ty| self.is_class_type(*ty));
            }
            if let Some(class_ty) = self.class_types.get(name.as_str()) {
                let node_ty = owner.ty();
                if node_ty != *class_ty && self.is_class_type(node_ty) {
                    return Some(node_ty);
                }
                return Some(*class_ty);
            }
        }
        Some(owner.ty())
    }

    pub(super) fn argument_body(&self, expr: &HirExpr) -> ArgumentBody {
        match expr {
            HirExpr::Closure { params, body, .. } => ArgumentBody::Closures(vec![ClosureBody {
                params: params.clone(),
                body: body.clone(),
            }]),
            HirExpr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                let then_body = self.argument_body(then_branch);
                let else_body = self.argument_body(else_branch);
                match (then_body, else_body) {
                    (
                        ArgumentBody::Closures(mut then_bodies),
                        ArgumentBody::Closures(else_bodies),
                    ) => {
                        then_bodies.extend(else_bodies);
                        ArgumentBody::Closures(then_bodies)
                    }
                    (ArgumentBody::Function(then_name), ArgumentBody::Function(else_name))
                        if then_name == else_name =>
                    {
                        ArgumentBody::Function(then_name)
                    }
                    (
                        ArgumentBody::Method(then_ty, then_name),
                        ArgumentBody::Method(else_ty, else_name),
                    ) if then_ty == else_ty && then_name == else_name => {
                        ArgumentBody::Method(then_ty, then_name)
                    }
                    _ => ArgumentBody::Unknown,
                }
            }
            HirExpr::Local { id, .. } => {
                if self.unknown_locals.contains(id) {
                    return ArgumentBody::Unknown;
                }
                let bodies = self.local_closures.get(id);
                let aliases = self.local_aliases.get(id);
                match (bodies, aliases) {
                    (Some(bodies), None) => ArgumentBody::Closures(bodies.clone()),
                    (None, Some(aliases)) if aliases.len() == 1 => match &aliases[0] {
                        ReceiverCallTarget::Function(name) => ArgumentBody::Function(name.clone()),
                        ReceiverCallTarget::Method(ty, name) => {
                            ArgumentBody::Method(*ty, name.clone())
                        }
                    },
                    _ => ArgumentBody::Unknown,
                }
            }
            HirExpr::Global { name, .. } => ArgumentBody::Function(name.as_str().to_owned()),
            HirExpr::Field {
                owner, field_name, ..
            } => match self.owner_type(owner) {
                Some(ty) => ArgumentBody::Method(ty, field_name.as_str().to_owned()),
                None => ArgumentBody::Unknown,
            },
            _ => ArgumentBody::Unknown,
        }
    }

    fn argument_bodies(&self, args: &[HirExpr]) -> Vec<ArgumentBody> {
        args.iter().map(|arg| self.argument_body(arg)).collect()
    }

    fn push_call_site(
        &mut self,
        target: ReceiverCallTarget,
        args: &[HirExpr],
        owner_path: Option<Vec<String>>,
    ) {
        let bindings = self.argument_bodies(args);
        self.call_sites.push(ReceiverCallSite {
            target,
            args: bindings,
            owner_path,
        });
    }

    fn place_fields(&self, place: &HirExpr) -> Option<Vec<String>> {
        match place {
            HirExpr::Local { id, .. }
                if self.root_in_scope && Some(*id) == self.receiver.root_local() =>
            {
                Some(Vec::new())
            }
            HirExpr::Local { id, .. } if Some(*id) == self.this_local => self.this_place.clone(),
            HirExpr::Field {
                owner, field_name, ..
            } => {
                let mut fields = self.place_fields(owner)?;
                fields.push(field_name.as_str().to_owned());
                Some(fields)
            }
            HirExpr::Index { owner, .. } => {
                let mut fields = self.place_fields(owner)?;
                fields.push(INDEX_PATH_COMPONENT.to_owned());
                Some(fields)
            }
            _ => None,
        }
    }

    fn writes_receiver(&self, target: &HirExpr) -> bool {
        match (&self.receiver, target) {
            (ReceiverRef::Global(name), HirExpr::Global { name: target, .. }) => {
                target.as_str() == name
            }
            (
                ReceiverRef::StaticField { class, field },
                HirExpr::Field {
                    owner, field_name, ..
                },
            ) => {
                field_name.as_str() == field
                    && matches!(owner.as_ref(), HirExpr::Global { name, .. } if name.as_str() == class)
            }
            (ReceiverRef::Path { fields, .. }, _) => {
                let Some(written) = self.place_fields(target) else {
                    return false;
                };
                fields.starts_with(&written)
            }
            _ => false,
        }
    }
}

impl Visitor for ReceiverWriteScan<'_> {
    fn visit_stmt(&mut self, stmt: &HirStmt) {
        if let HirStmt::Let { id, init, .. } = stmt {
            match init {
                Some(HirExpr::Local { id: source, .. }) => {
                    let bodies = self.local_closures.get(source).cloned();
                    let aliases = self.local_aliases.get(source).cloned();
                    let unknown = self.unknown_locals.contains(source);
                    self.reset_local(*id);
                    for body in bodies.unwrap_or_default() {
                        self.add_local_closure(*id, body);
                    }
                    for alias in aliases.unwrap_or_default() {
                        self.add_local_alias(*id, alias);
                    }
                    if unknown {
                        self.unknown_locals.insert(*id);
                    }
                }
                Some(init) => {
                    let arg = self.argument_body(init);
                    self.reset_local(*id);
                    self.bind_local(*id, &arg);
                }
                None => self.reset_local(*id),
            }
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpr) {
        if matches!(expr, HirExpr::Closure { .. }) {
            return;
        }
        match expr {
            HirExpr::Assignment { target, value, .. } => {
                if let HirExpr::Local { id, .. } = target.as_ref() {
                    let arg = self.argument_body(value);
                    self.bind_local(*id, &arg);
                }
                if self.writes_receiver(target) {
                    self.direct_write = true;
                }
            }
            HirExpr::CompoundUpdate { target, .. } => {
                if let HirExpr::Local { id, .. } = target.as_ref() {
                    self.poison_local(*id);
                }
                if self.writes_receiver(target) {
                    self.direct_write = true;
                }
            }
            HirExpr::Call { callee, args, .. } => match callee {
                HirCallee::Indirect(inner) => match inner.as_ref() {
                    HirExpr::Global { name, .. } => self.push_call_site(
                        ReceiverCallTarget::Function(name.as_str().to_owned()),
                        args,
                        None,
                    ),
                    HirExpr::Field {
                        owner, field_name, ..
                    } => match self.owner_type(owner) {
                        Some(ty) => {
                            let owner_path = self.place_fields(owner);
                            self.push_call_site(
                                ReceiverCallTarget::Method(ty, field_name.as_str().to_owned()),
                                args,
                                owner_path,
                            );
                        }
                        None => self.opaque = true,
                    },
                    HirExpr::Closure { params, body, .. } => {
                        let bindings = self.argument_bodies(args);
                        self.bind_params(params, &bindings);
                        for stmt in body {
                            self.visit_stmt(stmt);
                        }
                    }
                    HirExpr::Local { id, .. } => {
                        let targets = self.local_aliases.get(id).cloned();
                        let aliased = targets.as_ref().is_some_and(|targets| !targets.is_empty());
                        for target in targets.unwrap_or_default() {
                            self.push_call_site(target, args, None);
                        }
                        let bindings = self.argument_bodies(args);
                        if self.expanded_locals.insert((*id, bindings.clone())) {
                            let bodies = self.local_closures.get(id).cloned();
                            match bodies {
                                Some(bodies) => {
                                    for body in &bodies {
                                        self.bind_params(&body.params, &bindings);
                                        for stmt in &body.body {
                                            self.visit_stmt(stmt);
                                        }
                                    }
                                }
                                None => {
                                    if !aliased {
                                        self.opaque = true;
                                    }
                                }
                            }
                        }
                    }
                    _ => self.opaque = true,
                },
                HirCallee::Runtime { .. } => {}
                HirCallee::Function(_) | HirCallee::Closure(_) => self.opaque = true,
            },
            HirExpr::New { .. } => self.opaque = true,
            _ => {}
        }
        walk_expr(self, expr);
    }
}

struct ThisWriteScan {
    this_id: LocalId,
    writes: bool,
    calls_method: bool,
}

impl ThisWriteScan {
    fn roots_at_this(&self, place: &HirExpr) -> bool {
        match place {
            HirExpr::Local { id, .. } => *id == self.this_id,
            HirExpr::Field { owner, .. } | HirExpr::Index { owner, .. } => {
                self.roots_at_this(owner)
            }
            _ => false,
        }
    }
}

impl Visitor for ThisWriteScan {
    fn visit_expr(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Assignment { target, .. } | HirExpr::CompoundUpdate { target, .. } => {
                if self.roots_at_this(target) {
                    self.writes = true;
                }
            }
            HirExpr::Call {
                callee: HirCallee::Indirect(inner),
                ..
            } => {
                if let HirExpr::Field { owner, .. } = inner.as_ref()
                    && self.roots_at_this(owner)
                {
                    self.calls_method = true;
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

#[derive(Default, Clone)]
struct ClosureBinding {
    params: Vec<HirParam>,
    body: Vec<HirStmt>,
    body_dump: String,
    ambiguous: bool,
}

#[derive(Default)]
struct ReceiverBodyIndex<'p> {
    functions: HashMap<String, &'p HirFunction>,
    methods: HashMap<TypeId, HashMap<String, &'p HirFunction>>,
    bindings: HashMap<String, ClosureBinding>,
    aliases: HashMap<String, String>,
    aliases_ambiguous: HashSet<String>,
    classes: HashSet<TypeId>,
    classes_by_name: HashMap<String, TypeId>,
    parents: HashMap<TypeId, String>,
}

struct ClosureBindingScan<'i> {
    bindings: &'i mut HashMap<String, ClosureBinding>,
    aliases: &'i mut HashMap<String, String>,
    aliases_ambiguous: &'i mut HashSet<String>,
}

impl Visitor for ClosureBindingScan<'_> {
    fn visit_expr(&mut self, expr: &HirExpr) {
        if let HirExpr::Assignment { target, value, .. } = expr
            && let HirExpr::Global { name, .. } = target.as_ref()
        {
            match value.as_ref() {
                HirExpr::Closure { params, body, .. } => {
                    let name = name.as_str().to_owned();
                    if self.aliases.contains_key(name.as_str()) {
                        self.aliases_ambiguous.insert(name.clone());
                    }
                    let body_dump = ts_aot_ir_hir::dump_stmts_text(body);
                    match self.bindings.get_mut(&name) {
                        Some(binding) => {
                            if binding.body_dump != body_dump {
                                binding.ambiguous = true;
                            }
                        }
                        None => {
                            self.bindings.insert(
                                name,
                                ClosureBinding {
                                    params: params.clone(),
                                    body: body.clone(),
                                    body_dump,
                                    ambiguous: false,
                                },
                            );
                        }
                    }
                }
                HirExpr::Global { name: function, .. } => {
                    let name = name.as_str().to_owned();
                    let function = function.as_str().to_owned();
                    if self.bindings.contains_key(name.as_str()) {
                        self.aliases_ambiguous.insert(name.clone());
                    }
                    match self.aliases.get(name.as_str()) {
                        Some(existing) if *existing != function => {
                            self.aliases_ambiguous.insert(name);
                        }
                        Some(_) => {}
                        None => {
                            self.aliases.insert(name, function);
                        }
                    }
                }
                _ => {}
            }
        }
        walk_expr(self, expr);
    }
}

impl<'p> ReceiverBodyIndex<'p> {
    fn new(program: &'p HirProgram, tla_items: &[TlaMainItem]) -> Self {
        let mut index = Self::default();
        for decl in &program.declarations {
            match decl {
                HirDecl::Function(function) => {
                    index
                        .functions
                        .insert(function.name.as_str().to_owned(), function);
                    index.collect_bindings(&function.body);
                }
                HirDecl::Class(class) => {
                    index.classes.insert(class.ty);
                    index
                        .classes_by_name
                        .insert(class.name.as_str().to_owned(), class.ty);
                    if let Some(parent) = &class.extends {
                        index.parents.insert(class.ty, parent.as_str().to_owned());
                    }
                    for method in &class.methods {
                        index
                            .methods
                            .entry(class.ty)
                            .or_default()
                            .insert(method.name.as_str().to_owned(), method);
                    }
                    for method in &class.methods {
                        index.collect_bindings(&method.body);
                    }
                }
                _ => {}
            }
        }
        index.collect_tla_items(tla_items);
        index
    }

    fn collect_bindings(&mut self, body: &[HirStmt]) {
        let mut scan = ClosureBindingScan {
            bindings: &mut self.bindings,
            aliases: &mut self.aliases,
            aliases_ambiguous: &mut self.aliases_ambiguous,
        };
        for stmt in body {
            scan.visit_stmt(stmt);
        }
    }

    fn collect_tla_items(&mut self, items: &[TlaMainItem]) {
        let mut scan = ClosureBindingScan {
            bindings: &mut self.bindings,
            aliases: &mut self.aliases,
            aliases_ambiguous: &mut self.aliases_ambiguous,
        };
        for item in items {
            let TlaMainItem::Expr(expr) = item;
            scan.visit_expr(expr);
        }
    }

    fn function(&self, name: &str) -> Option<(&[HirParam], &[HirStmt])> {
        if let Some(function) = self.functions.get(name) {
            return Some((function.params.as_slice(), function.body.as_slice()));
        }
        if self.aliases_ambiguous.contains(name) {
            return None;
        }
        if let Some(binding) = self.bindings.get(name) {
            if binding.ambiguous {
                return None;
            }
            return Some((binding.params.as_slice(), binding.body.as_slice()));
        }
        let target = self.aliases.get(name)?;
        self.functions
            .get(target.as_str())
            .map(|function| (function.params.as_slice(), function.body.as_slice()))
    }

    fn method(&self, owner_ty: TypeId, name: &str) -> Option<(&[HirParam], &[HirStmt])> {
        let mut ty = owner_ty;
        for _ in 0..16 {
            if let Some(method) = self.methods.get(&ty).and_then(|methods| methods.get(name)) {
                return Some((method.params.as_slice(), method.body.as_slice()));
            }
            let parent = self.parents.get(&ty)?;
            ty = *self.classes_by_name.get(parent)?;
        }
        None
    }

    fn has_class(&self, ty: TypeId) -> bool {
        self.classes.contains(&ty)
    }
}

pub(super) struct CaptureClassValue {
    pub(super) id: LocalId,
    pub(super) classes: Option<Vec<TypeId>>,
    pub(super) pending_frame: usize,
}

pub(super) struct PendingCaptureRead {
    pub(super) name: String,
    pub(super) id: LocalId,
    pub(super) seen: Vec<TypeId>,
    pub(super) span: oxc_span::Span,
}

pub(super) struct PendingLoopRead {
    pub(super) name: Option<String>,
    pub(super) id: LocalId,
    pub(super) seen: Vec<TypeId>,
    pub(super) span: oxc_span::Span,
}

impl SkeletonBuilder {
    pub(crate) fn new(source_len: u32, module: bool) -> Self {
        let tla_main_name = if module {
            fixed_tla_main_name()
        } else {
            Atom::from("")
        };
        Self {
            source_len,
            next_generic_param: 0,
            next_anon_class_id: 0,
            next_closure_id: 0,
            next_class_struct_id: 0,
            resolved_aliases: HashMap::new(),
            class_types: HashMap::new(),
            class_value_globals: HashMap::new(),
            module_init_depth: 0,
            module_lets: HashSet::new(),
            module_vars: HashSet::new(),
            mutable_locals: HashSet::new(),
            pending_class_value_global_reads: Vec::new(),
            pending_global_receiver_calls: Vec::new(),
            unknown_class_value_globals: HashSet::new(),
            unknown_class_value_locals: HashMap::new(),
            capture_class_values: Vec::new(),
            pending_capture_class_value_reads: Vec::new(),
            loop_frames: Vec::new(),
            class_value_locals: HashMap::new(),
            class_value_fields: HashMap::new(),
            class_value_field_initials: HashMap::new(),
            unknown_class_value_fields: HashMap::new(),
            fresh_field_bases: HashSet::new(),
            fresh_field_classes: HashMap::new(),
            class_value_field_poison_bases: HashMap::new(),
            class_value_poison_writes: Vec::new(),
            class_value_exits: Vec::new(),
            class_value_loop_frames: Vec::new(),
            constant_int_locals: HashMap::new(),
            switch_case_frames: Vec::new(),
            class_value_branch_frames: Vec::new(),
            class_value_branch_records: Vec::new(),
            class_value_branch_ids: Vec::new(),
            class_value_breakable_depth: 0,
            next_class_value_branch_id: 0,
            class_value_poisoned_locals: HashMap::new(),
            class_value_poisoned_globals: HashMap::new(),
            pending_class_value_field_reads: Vec::new(),
            class_value_tags: HashMap::new(),
            is_generator_stack: Vec::new(),
            type_param_stack: Vec::new(),
            in_static_method: false,
            current_super_class: None,
            module,
            static_field_globals: HashMap::new(),
            static_method_globals: HashMap::new(),
            static_method_captures: HashMap::new(),
            pending_static_captures: HashMap::new(),
            capture_check_frames: Vec::new(),
            class_capture_fields: HashMap::new(),
            class_decl_body: HashMap::new(),
            pending_closure_captures: Vec::new(),
            body_depth: 0,
            in_callee_position: false,
            strict: module,
            tla_main_body: Vec::new(),
            tla_main_name,
            tla_top_level_scope: BodyScope::new(0),
            top_level_function_spans: HashMap::new(),
            narrowed_stack: Vec::new(),
            captured_stack: Vec::new(),
            declared_names_stack: Vec::new(),
            generator_alias_stack: Vec::new(),
            suppress_narrowing: 0,
            conditional_depth: 0,
            try_body_skips: None,
            own_statement_expr: false,
            pending_narrowed_updates: Vec::new(),
            array_literal_hints: HashMap::new(),
            class_method_rets: HashMap::new(),
            private_class_stack: Vec::new(),
            hoisted_fn_decls: Vec::new(),
            current_class_ty: None,
            current_class_own_methods: Vec::new(),
            this_local_stack: Vec::new(),
            this_capture_locals: Vec::new(),
        }
    }

    pub(crate) fn build(mut self, senv: &mut SkeletonEnv<'_>, program: &Program<'_>) {
        self.strict = self.module || crate::util::directives_have_use_strict(&program.directives);
        self.pre_register_class_types(senv, program);
        self.pre_resolve_all_aliases(senv, program);
        self.pre_register_class_fields(senv, program);
        for stmt in &program.body {
            self.walk_top_level(senv, stmt);
        }
        self.finish_class_value_global_reads(senv);
        self.finish_global_receiver_calls(senv);
        self.verify_class_value_field_reads(senv);
        self.check_reserved_names(senv);
        self.finalize_tla_main(senv);
        senv.program.next_class_struct_id = self.next_class_struct_id;
    }

    pub(super) fn finish_class_value_global_reads(&mut self, senv: &mut SkeletonEnv<'_>) {
        let pending = std::mem::take(&mut self.pending_class_value_global_reads);
        for read in pending {
            let final_set = self
                .class_value_globals
                .get(&read.name)
                .cloned()
                .unwrap_or_default();
            let complete = !self.unknown_class_value_globals.contains(&read.name)
                && final_set.len() == read.seen.len()
                && final_set
                    .iter()
                    .all(|class_ty| read.seen.contains(class_ty));
            if !complete {
                senv.report_unsupported(
                    "E0419",
                    format!(
                        "`{}` is a module-level `let` holding a class value that is reassigned \
                         later in the module, so its class is not known when this read runs; use \
                         `const`, or name the class directly",
                        read.name
                    )
                    .as_str(),
                    read.span,
                );
            }
        }
    }

    pub(super) fn finish_global_receiver_calls(&mut self, senv: &mut SkeletonEnv<'_>) {
        let pending = std::mem::take(&mut self.pending_global_receiver_calls);
        if pending.is_empty() {
            return;
        }
        let refused: Vec<(ReceiverRef, oxc_span::Span)> = {
            let index = ReceiverBodyIndex::new(senv.program, &self.tla_main_body);
            pending
                .into_iter()
                .filter(|call| {
                    self.receiver_is_mutable_place(call)
                        && Self::argument_may_reassign(&index, call)
                })
                .map(|call| (call.receiver, call.span))
                .collect()
        };
        for (receiver, span) in refused {
            let described = receiver.describe();
            let message = match &receiver {
                ReceiverRef::Path { fields, .. } if fields.is_empty() => format!(
                    "`{described}` is a mutable place, so a receiver read from it is resolved \
                     before the arguments of the call run, but the call may reassign `{described}`; \
                     assign the receiver outside the call or declare it `const`"
                ),
                ReceiverRef::Path { .. } => format!(
                    "`{described}` is a mutable place, so a receiver read from it is resolved \
                     before the arguments of the call run, but the call may reassign `{described}`; \
                     read the receiver into a local before the call"
                ),
                ReceiverRef::Global(_) | ReceiverRef::StaticField { .. } => format!(
                    "`{described}` is a mutable module binding, so a receiver read from it is \
                     resolved before the arguments of the call run, but the call may reassign \
                     `{described}`; assign `{described}` outside the call or declare it `const`"
                ),
            };
            senv.report_unsupported("E0419", message.as_str(), span);
        }
    }

    pub(crate) fn private_member(&self, name: &str) -> Option<(TypeId, Atom, PrivateMember)> {
        let key = Atom::from(name);
        self.private_class_stack.iter().rev().find_map(|frame| {
            frame
                .members
                .get(&key)
                .map(|member| (frame.ty, frame.class_name.clone(), member.clone()))
        })
    }

    pub(crate) fn lowered_class_key_hir_name(
        &self,
        class_ty: TypeId,
        key: &oxc_ast::ast::PropertyKey<'_>,
    ) -> Option<Atom> {
        if let Some(private) = key.private_name() {
            return Some(self.private_member(private.as_str()).map_or_else(
                || private_member_hir_name(class_ty, private.as_str()),
                |(_, _, member)| member.lowered.clone(),
            ));
        }
        key.static_name().map(|name| Atom::from(name.as_ref()))
    }

    fn is_static_field_global(&self, name: &str) -> bool {
        self.static_field_globals
            .values()
            .any(|fields| fields.values().any(|(global, _)| global.as_str() == name))
    }

    fn receiver_is_mutable_place(&self, call: &PendingGlobalReceiverCall) -> bool {
        match &call.receiver {
            ReceiverRef::Global(name) => {
                self.module_lets.contains(name)
                    || self.module_vars.contains(name)
                    || self.is_static_field_global(name)
            }
            ReceiverRef::StaticField { class, field } => self
                .class_types
                .get(class)
                .and_then(|ty| self.static_field_globals.get(ty))
                .is_some_and(|fields| fields.keys().any(|name| name.as_str() == field)),
            ReceiverRef::Path {
                root,
                root_id,
                body_id,
                fields,
            } => {
                if !fields.is_empty() {
                    return true;
                }
                root.as_str() != "this" && self.mutable_locals.contains(&(*body_id, *root_id))
            }
        }
    }

    fn argument_may_reassign(
        index: &ReceiverBodyIndex<'_>,
        call: &PendingGlobalReceiverCall,
    ) -> bool {
        let local_place = call.receiver.path_fields().is_some();
        if call.direct_write || (call.opaque && !local_place) {
            return true;
        }
        let mut seen: HashSet<ReceiverCallSite> = HashSet::new();
        let mut queue: Vec<ReceiverCallSite> = call.call_sites.clone();
        if let Some((params, body)) = index.method(call.owner_ty, call.method.as_str()) {
            let mut scan = ReceiverWriteScan::new(
                call.receiver.clone(),
                HashMap::new(),
                &index.classes_by_name,
                HashMap::new(),
            );
            scan.enter_method(params, call.receiver.path_fields().map(<[String]>::to_vec));
            scan.bind_params(params, &call.arg_bindings);
            for stmt in body {
                scan.visit_stmt(stmt);
            }
            if scan.direct_write || (scan.opaque && !local_place) {
                return true;
            }
            queue.extend(scan.call_sites);
        }
        while let Some(site) = queue.pop() {
            if !seen.insert(site.clone()) {
                continue;
            }
            let body = match &site.target {
                ReceiverCallTarget::Function(name) => index.function(name.as_str()),
                ReceiverCallTarget::Method(ty, name) => {
                    if !index.has_class(*ty) {
                        if local_place {
                            continue;
                        }
                        return true;
                    }
                    index.method(*ty, name.as_str())
                }
            };
            let Some((params, body)) = body else {
                if local_place {
                    continue;
                }
                return true;
            };
            let mut scan = ReceiverWriteScan::new(
                call.receiver.clone(),
                HashMap::new(),
                &index.classes_by_name,
                HashMap::new(),
            );
            scan.enter_method(params, site.owner_path.clone());
            scan.bind_params(params, &site.args);
            for stmt in body {
                scan.visit_stmt(stmt);
            }
            if scan.direct_write || (scan.opaque && !local_place) {
                return true;
            }
            queue.extend(scan.call_sites);
        }
        false
    }

    pub(super) fn method_writes_this(
        senv: &SkeletonEnv<'_>,
        owner_ty: TypeId,
        method: &str,
    ) -> bool {
        let Some(class) = senv
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                HirDecl::Class(class) if class.ty == owner_ty => Some(class),
                _ => None,
            })
        else {
            return false;
        };
        let Some(decl) = class.methods.iter().find(|m| m.name.as_str() == method) else {
            return true;
        };
        let Some(this) = decl.params.first().filter(|p| p.name.as_str() == "this") else {
            return false;
        };
        let mut scan = ThisWriteScan {
            this_id: this.id,
            writes: false,
            calls_method: false,
        };
        for stmt in &decl.body {
            scan.visit_stmt(stmt);
        }
        scan.writes || scan.calls_method
    }

    pub(super) fn captured_local_id(&self, name: &str) -> Option<LocalId> {
        self.capture_class_values
            .iter()
            .rev()
            .find_map(|frame| frame.get(name))
            .map(|captured| captured.id)
    }

    pub(super) fn captured_class_value_local(&self, name: &str) -> Option<LocalId> {
        self.capture_class_values
            .iter()
            .rev()
            .find_map(|frame| frame.get(name))
            .filter(|captured| captured.classes.is_some())
            .map(|captured| captured.id)
    }

    pub(super) fn capture_class_value_map(
        &self,
        scope: &crate::scope::BodyScope,
        narrowed: &HashMap<LocalId, (LocalId, TypeId)>,
    ) -> HashMap<String, CaptureClassValue> {
        let mut map = HashMap::new();
        let Some(pending_frame) = self.pending_capture_class_value_reads.len().checked_sub(1)
        else {
            return map;
        };
        for name in scope.names() {
            let id = scope
                .visible_binding(&name)
                .unwrap_or_else(|| LocalId::from_raw(u32::MAX));
            let storage = narrowed.get(&id).map_or(id, |(storage, _)| *storage);
            map.insert(
                name,
                CaptureClassValue {
                    id: storage,
                    classes: self.class_value_locals.get(&storage).cloned(),
                    pending_frame,
                },
            );
        }
        map
    }

    pub(super) fn captured_class_values(
        &mut self,
        name: &str,
        span: oxc_span::Span,
    ) -> Option<Vec<TypeId>> {
        let (id, captured_classes, pending_frame) = self
            .capture_class_values
            .iter()
            .rev()
            .find_map(|frame| frame.get(name))
            .map(|captured| {
                (
                    captured.id,
                    captured.classes.clone(),
                    captured.pending_frame,
                )
            })?;
        let captured_classes = captured_classes?;
        let classes = self
            .class_value_locals
            .get(&id)
            .cloned()
            .unwrap_or(captured_classes);
        if let Some(pending) = self
            .pending_capture_class_value_reads
            .get_mut(pending_frame)
        {
            pending.push(PendingCaptureRead {
                name: name.to_owned(),
                id,
                seen: classes.clone(),
                span,
            });
        }
        Some(classes)
    }

    pub(super) fn record_loop_class_value_copies(
        &mut self,
        sources: &[(LocalId, Vec<TypeId>)],
        span: oxc_span::Span,
    ) {
        if self.loop_frames.is_empty() {
            return;
        }
        for (id, classes) in sources {
            for frame in &mut self.loop_frames {
                frame.push(PendingLoopRead {
                    name: None,
                    id: *id,
                    seen: classes.clone(),
                    span,
                });
            }
        }
    }

    pub(super) fn record_class_value_global_copy(&mut self, names: &[&str], span: oxc_span::Span) {
        for name in names {
            let seen = self
                .class_value_globals
                .get(*name)
                .cloned()
                .unwrap_or_default();
            self.pending_class_value_global_reads
                .push(PendingGlobalRead {
                    name: (*name).to_owned(),
                    seen,
                    span,
                });
        }
    }

    pub(super) fn verify_capture_class_value_reads(&mut self, senv: &mut SkeletonEnv<'_>) {
        let Some(pending) = self.pending_capture_class_value_reads.pop() else {
            return;
        };
        for read in pending {
            let known = !self.unknown_class_value_locals.contains_key(&read.id);
            if !known || self.class_value_locals.get(&read.id) != Some(&read.seen) {
                senv.report_unsupported(
                    "E0419",
                    format!(
                        "`{}` holds a class value captured by a closure, and the binding is \
                         rebound to another class or to a plain value afterwards, so the closure \
                         cannot know which class it holds; read the static in the enclosing \
                         function, or keep the binding unchanged",
                        read.name
                    )
                    .as_str(),
                    read.span,
                );
            }
        }
    }

    fn verify_class_value_field_reads(&mut self, senv: &mut SkeletonEnv<'_>) {
        let pending = std::mem::take(&mut self.pending_class_value_field_reads);
        for (owner_ty, field_name, seen, span, base) in pending {
            let poisoned = self
                .unknown_class_value_fields
                .get(&owner_ty)
                .is_some_and(|fields| fields.contains(&field_name));
            let fresh = base
                .as_ref()
                .is_some_and(|base| self.class_value_field_base_is_fresh(base));
            let complete = if fresh {
                let final_set = match &base {
                    Some(base) => self.fresh_field_base_classes(base, owner_ty, &field_name),
                    None => self.initial_class_value_field_classes(owner_ty, &field_name),
                };
                !poisoned && final_set == seen
            } else {
                !poisoned
                    && self
                        .class_value_fields
                        .get(&owner_ty)
                        .and_then(|fields| fields.get(&field_name))
                        == Some(&seen)
            };
            if !complete {
                senv.report_unsupported(
                    "E0419",
                    format!(
                        "`{field_name}` is a class-value field that is assigned another class or \
                         another value later, so its class is not known when this read runs; read \
                         the static through a `const` binding, or name the class directly"
                    )
                    .as_str(),
                    span,
                );
            }
        }
    }

    pub(super) fn snapshot_class_value_field_state(&self) -> ClassValueFieldState {
        ClassValueFieldState {
            classes: self.class_value_fields.clone(),
            unknown: self.unknown_class_value_fields.clone(),
            bases: self.class_value_field_poison_bases.clone(),
        }
    }

    pub(super) fn class_value_field_base_is_fresh(&self, base: &ClassValueFieldBase) -> bool {
        self.fresh_field_bases.contains(base)
    }

    pub(super) fn note_fresh_field_base(&mut self, base: ClassValueFieldBase) {
        self.fresh_field_classes
            .retain(|(other, _, _), _| other != &base);
        self.fresh_field_bases.insert(base);
    }

    pub(super) fn forget_fresh_field_base(&mut self, base: &ClassValueFieldBase) {
        self.fresh_field_bases.remove(base);
    }

    pub(super) fn fresh_field_base_classes(
        &self,
        base: &ClassValueFieldBase,
        owner_ty: TypeId,
        field: &Atom,
    ) -> Vec<TypeId> {
        let mut classes = self
            .fresh_field_classes
            .get(&(base.clone(), owner_ty, field.clone()))
            .cloned()
            .unwrap_or_else(|| self.initial_class_value_field_classes(owner_ty, field));
        for class_ty in self.escaping_field_extra_classes(owner_ty, field) {
            if !classes.contains(&class_ty) {
                classes.push(class_ty);
            }
        }
        classes
    }

    pub(super) fn escaping_field_extra_classes(
        &self,
        owner_ty: TypeId,
        field: &Atom,
    ) -> Vec<TypeId> {
        let initials = self.initial_class_value_field_classes(owner_ty, field);
        self.class_value_fields
            .get(&owner_ty)
            .and_then(|fields| fields.get(field))
            .map_or_else(Vec::new, |classes| {
                classes
                    .iter()
                    .filter(|class_ty| !initials.contains(class_ty))
                    .copied()
                    .collect()
            })
    }

    pub(super) fn set_fresh_field_base_classes(
        &mut self,
        base: &ClassValueFieldBase,
        owner_ty: TypeId,
        field: &Atom,
        classes: Vec<TypeId>,
    ) {
        self.fresh_field_classes
            .insert((base.clone(), owner_ty, field.clone()), classes);
    }

    pub(super) fn fresh_field_base_classes_or_initials(
        &self,
        base: &ClassValueFieldBase,
        owner_ty: TypeId,
        field: &Atom,
    ) -> Vec<TypeId> {
        self.fresh_field_classes
            .get(&(base.clone(), owner_ty, field.clone()))
            .cloned()
            .unwrap_or_else(|| self.initial_class_value_field_classes(owner_ty, field))
    }

    pub(super) fn restore_class_value_field_state(&mut self, state: &ClassValueFieldState) {
        self.class_value_fields.clone_from(&state.classes);
        self.unknown_class_value_fields.clone_from(&state.unknown);
        self.class_value_field_poison_bases.clone_from(&state.bases);
    }

    pub(super) fn class_value_field_names(&self, owner_ty: TypeId) -> Vec<Atom> {
        self.class_value_field_initials
            .get(&owner_ty)
            .map_or_else(Vec::new, |fields| fields.keys().cloned().collect())
    }

    pub(super) fn initial_class_value_field_classes(
        &self,
        owner_ty: TypeId,
        field: &Atom,
    ) -> Vec<TypeId> {
        self.class_value_field_initials
            .get(&owner_ty)
            .and_then(|fields| fields.get(field))
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn clear_class_value_field_base(
        &mut self,
        owner_ty: TypeId,
        field: &Atom,
        base: &ClassValueFieldBase,
    ) -> bool {
        if let Some(bases) = self
            .class_value_field_poison_bases
            .get_mut(&owner_ty)
            .and_then(|fields| fields.get_mut(field))
        {
            bases.bases.remove(base);
            bases.fact_depths.remove(base);
            bases.fact_branches.remove(base);
            if !bases.bases.is_empty() || bases.unknown_base {
                return false;
            }
            if let Some(fields) = self.class_value_field_poison_bases.get_mut(&owner_ty) {
                fields.remove(field);
            }
        }
        if let Some(unknown) = self.unknown_class_value_fields.get_mut(&owner_ty) {
            return unknown.remove(field);
        }
        false
    }

    pub(super) fn join_class_value_field_states(&mut self, branches: &[ClassValueFieldState]) {
        if branches.is_empty() {
            return;
        }
        let mut classes: HashMap<TypeId, HashMap<Atom, Vec<TypeId>>> = HashMap::new();
        let mut unknown: HashMap<TypeId, HashSet<Atom>> = HashMap::new();
        let mut bases: HashMap<TypeId, HashMap<Atom, ClassValuePoisonBases>> = HashMap::new();
        for branch in branches {
            for (owner_ty, fields) in &branch.classes {
                let owner = classes.entry(*owner_ty).or_default();
                for (field, branch_classes) in fields {
                    let target = owner.entry(field.clone()).or_default();
                    for class_ty in branch_classes {
                        if !target.contains(class_ty) {
                            target.push(*class_ty);
                        }
                    }
                }
            }
            for (owner_ty, fields) in &branch.unknown {
                unknown
                    .entry(*owner_ty)
                    .or_default()
                    .extend(fields.iter().cloned());
            }
            for (owner_ty, fields) in &branch.bases {
                let owner = bases.entry(*owner_ty).or_default();
                for (field, branch_bases) in fields {
                    if let Some(existing) = owner.get_mut(field) {
                        existing.merge(branch_bases);
                    } else {
                        owner.insert(field.clone(), branch_bases.clone());
                    }
                }
            }
        }
        self.class_value_fields = classes;
        self.unknown_class_value_fields = unknown;
        self.class_value_field_poison_bases = bases;
    }

    pub(super) fn verify_loop_class_value_reads(&mut self, senv: &mut SkeletonEnv<'_>) {
        let Some(pending) = self.loop_frames.pop() else {
            return;
        };
        for read in pending {
            let complete = !self.unknown_class_value_locals.contains_key(&read.id)
                && self.class_value_locals.get(&read.id) == Some(&read.seen);
            if !complete {
                let what = read
                    .name
                    .as_deref()
                    .map_or_else(|| "a class value".to_owned(), |name| format!("`{name}`"));
                senv.report_unsupported(
                    "E0419",
                    format!(
                        "{what} is read inside a loop and reassigned later in the loop body, so a \
                         later iteration holds a class this read was not lowered for; read the \
                         static before the loop, or keep the binding unchanged inside it"
                    )
                    .as_str(),
                    read.span,
                );
            }
        }
    }

    pub(super) fn merge_unknown_class_value_locals(&mut self, saved: HashMap<LocalId, String>) {
        for (id, name) in saved {
            self.unknown_class_value_locals.entry(id).or_insert(name);
        }
    }

    pub(super) fn note_switch_case_local_read(&mut self, id: LocalId) {
        for frame in &mut self.switch_case_frames {
            if let Some(events) = frame.events.get_mut(frame.current) {
                events.push(SwitchCaseEvent {
                    local: id,
                    kind: SwitchCaseEventKind::Use,
                    name: None,
                    shadowed: None,
                });
            }
        }
    }

    pub(super) fn note_switch_case_hoisted_decl(
        &mut self,
        id: LocalId,
        name: &Atom,
        scope_depth: usize,
    ) {
        let Some(frame) = self.switch_case_frames.last_mut() else {
            return;
        };
        if frame.scope_depth != scope_depth {
            return;
        }
        let shadowed = frame.outer.get(name).copied().filter(|outer| *outer != id);
        if let Some(events) = frame.events.get_mut(frame.current) {
            events.insert(
                0,
                SwitchCaseEvent {
                    local: id,
                    kind: SwitchCaseEventKind::Decl,
                    name: Some(name.clone()),
                    shadowed,
                },
            );
        }
    }

    pub(super) fn note_switch_case_declaration(
        &mut self,
        id: LocalId,
        name: &Atom,
        scope_depth: usize,
        kind: SwitchCaseEventKind,
    ) {
        let Some(frame) = self.switch_case_frames.last_mut() else {
            return;
        };
        if frame.scope_depth != scope_depth {
            return;
        }
        let shadowed = if kind == SwitchCaseEventKind::Decl {
            frame.outer.get(name).copied().filter(|outer| *outer != id)
        } else {
            None
        };
        if let Some(events) = frame.events.get_mut(frame.current) {
            events.push(SwitchCaseEvent {
                local: id,
                kind,
                name: Some(name.clone()),
                shadowed,
            });
        }
    }

    pub(super) fn snapshot_constant_int_locals(&self) -> HashMap<LocalId, i64> {
        self.constant_int_locals.clone()
    }

    pub(super) fn restore_constant_int_locals(&mut self, state: &HashMap<LocalId, i64>) {
        self.constant_int_locals.clone_from(state);
    }

    pub(super) fn intersect_constant_int_locals(&mut self, other: &HashMap<LocalId, i64>) {
        self.constant_int_locals
            .retain(|id, value| other.get(id) == Some(value));
    }

    pub(super) fn push_switch_case_frame(
        &mut self,
        scope_depth: usize,
        outer: HashMap<Atom, LocalId>,
    ) {
        self.switch_case_frames.push(SwitchCaseFrame {
            scope_depth,
            current: 0,
            outer,
            events: Vec::new(),
        });
    }

    pub(super) fn start_switch_case(&mut self) {
        let Some(frame) = self.switch_case_frames.last_mut() else {
            return;
        };
        frame.current = frame.events.len();
        frame.events.push(Vec::new());
    }

    pub(super) fn pop_switch_case_frame(&mut self) -> Option<SwitchCaseFrame> {
        let frame = self.switch_case_frames.pop()?;
        for frame in &mut self.switch_case_frames {
            frame.current = frame.current.min(frame.events.len().saturating_sub(1));
        }
        Some(frame)
    }

    pub(crate) fn class_value_tag(&mut self, ty: TypeId) -> i64 {
        let next = match i64::try_from(self.class_value_tags.len()) {
            Ok(len) => len.saturating_add(1),
            Err(_) => i64::MAX,
        };
        *self.class_value_tags.entry(ty).or_insert(next)
    }

    fn pre_register_class_types(&mut self, senv: &mut SkeletonEnv, program: &Program<'_>) {
        for stmt in &program.body {
            let class_opt: Option<&oxc_ast::ast::Class<'_>> =
                if let Some(decl) = stmt.as_declaration() {
                    if let Declaration::ClassDeclaration(c) = decl {
                        Some(c)
                    } else {
                        None
                    }
                } else if let Some(m) = stmt.as_module_declaration() {
                    if let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                        && let Some(Declaration::ClassDeclaration(c)) = e.declaration.as_ref()
                    {
                        Some(c)
                    } else {
                        None
                    }
                } else {
                    None
                };
            if let Some(class) = class_opt
                && let Some(id) = class.id.as_ref()
            {
                let name = id.name.as_str().to_owned();
                if !self.class_types.contains_key(&name) {
                    let struct_id = ts_aot_core::StructId::from_raw(self.next_class_struct_id);
                    self.next_class_struct_id = self.next_class_struct_id.saturating_add(1);
                    let ty = senv.types.intern(&Type::Struct { id: struct_id });
                    self.class_types.insert(name, ty);
                }
            }
        }
    }

    fn check_reserved_names(&mut self, senv: &mut SkeletonEnv) {
        if !self.module {
            return;
        }
        let end = self.source_len;
        let fallback_span = CoreSpan::new(0, end);
        for decl in &senv.program.declarations {
            let (name, kind, span) = match decl {
                HirDecl::Function(f) => {
                    let span = self
                        .top_level_function_spans
                        .get(&f.name)
                        .copied()
                        .unwrap_or(fallback_span);
                    (f.name.as_str(), "function", span)
                }
                HirDecl::Global { name, .. } => (name.as_str(), "global", fallback_span),
                _ => continue,
            };
            if let Some((code, reason)) = reserved_name_reason(name) {
                senv.diagnostics.push(Diagnostic::error(
                    code,
                    format!(
                        "{kind} `{name}` is reserved in module mode (collides with {reason}); rename or remove"
                    ),
                    span,
                ));
            }
        }
    }

    fn finalize_tla_main(&mut self, senv: &mut SkeletonEnv) {
        if !self.module {
            return;
        }
        let items = std::mem::take(&mut self.tla_main_body);
        let unit_ty = senv.types.intern(&Type::Void);
        let mut body: Vec<HirStmt> = Vec::with_capacity(items.len());
        for item in items {
            let TlaMainItem::Expr(expr) = item;
            body.push(HirStmt::Expr { expr });
        }
        if body.is_empty() {
            return;
        }
        let is_async = body_contains_await(&body);
        let tla_main = HirDecl::Function(HirFunction {
            name: self.tla_main_name.clone(),
            params: Vec::<HirParam>::new(),
            ret: unit_ty,
            throws: None,
            body,
            is_async,
            is_generator: false,
            is_exported: false,
            type_params: Vec::new(),
            async_info: None,
        });
        senv.program.declarations.push(tla_main);
        senv.program.tla_main_name = Some(self.tla_main_name.clone());
    }

    fn pre_resolve_all_aliases(&mut self, senv: &mut SkeletonEnv, program: &Program<'_>) {
        let names = Self::collect_alias_names(&program.body);
        let alias_set: HashSet<String> = names.iter().cloned().collect();
        let mut visiting: HashSet<String> = HashSet::new();
        for name in &names {
            self.resolve_alias_chain(senv, name, &alias_set, &mut visiting, program);
        }
    }

    fn collect_alias_names(stmts: &[Statement<'_>]) -> Vec<String> {
        let mut names = Vec::new();
        for stmt in stmts {
            if let Some(decl) = stmt.as_declaration()
                && let Declaration::TSTypeAliasDeclaration(a) = decl
            {
                names.push(a.id.name.to_string());
            } else if let Some(m) = stmt.as_module_declaration()
                && let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                && let Some(Declaration::TSTypeAliasDeclaration(a)) = e.declaration.as_ref()
            {
                names.push(a.id.name.to_string());
            }
        }
        names
    }

    fn resolve_alias_chain(
        &mut self,
        senv: &mut SkeletonEnv,
        name: &str,
        alias_set: &HashSet<String>,
        visiting: &mut HashSet<String>,
        program: &Program<'_>,
    ) {
        if self.resolved_aliases.contains_key(name) {
            return;
        }
        if !visiting.insert(name.to_string()) {
            self.record_alias_cycle(senv, name, program);
            let id = senv.types.intern(&Type::Error);
            self.resolved_aliases.insert(name.to_string(), id);
            return;
        }
        for stmt in &program.body {
            let annotation_opt: Option<&TSType<'_>> = if let Some(decl) = stmt.as_declaration() {
                if let Declaration::TSTypeAliasDeclaration(a) = decl
                    && a.id.name == name
                {
                    Some(&a.type_annotation)
                } else {
                    None
                }
            } else if let Some(m) = stmt.as_module_declaration() {
                if let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                    && let Some(Declaration::TSTypeAliasDeclaration(a)) = e.declaration.as_ref()
                    && a.id.name == name
                {
                    Some(&a.type_annotation)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(rhs) = annotation_opt {
                self.pre_resolve_aliases_in_type(senv, rhs, alias_set, visiting, program);
                let target_id = self.resolve_ts_type(senv, Some(rhs));
                visiting.remove(name);
                self.resolved_aliases.insert(name.to_string(), target_id);
                return;
            }
        }
        visiting.remove(name);
        let id = senv.types.intern(&Type::Error);
        self.resolved_aliases.insert(name.to_string(), id);
    }

    fn pre_resolve_aliases_in_type(
        &mut self,
        senv: &mut SkeletonEnv,
        ty: &TSType<'_>,
        alias_set: &HashSet<String>,
        visiting: &mut HashSet<String>,
        program: &Program<'_>,
    ) {
        match ty {
            TSType::TSTypeReference(r) => {
                if let TSTypeName::IdentifierReference(id) = &r.type_name {
                    let dep_name = id.name.as_str();
                    if alias_set.contains(dep_name) && !self.resolved_aliases.contains_key(dep_name)
                    {
                        self.resolve_alias_chain(senv, dep_name, alias_set, visiting, program);
                    }
                }
                if let Some(type_args) = &r.type_arguments {
                    for arg in &type_args.params {
                        self.pre_resolve_aliases_in_type(senv, arg, alias_set, visiting, program);
                    }
                }
            }
            TSType::TSUnionType(u) => {
                for variant in &u.types {
                    self.pre_resolve_aliases_in_type(senv, variant, alias_set, visiting, program);
                }
            }
            TSType::TSIntersectionType(i) => {
                for part in &i.types {
                    self.pre_resolve_aliases_in_type(senv, part, alias_set, visiting, program);
                }
            }
            TSType::TSTupleType(t) => {
                for element in &t.element_types {
                    if let Some(ty) = element.as_ts_type() {
                        self.pre_resolve_aliases_in_type(senv, ty, alias_set, visiting, program);
                    }
                }
            }
            TSType::TSArrayType(a) => {
                self.pre_resolve_aliases_in_type(
                    senv,
                    &a.element_type,
                    alias_set,
                    visiting,
                    program,
                );
            }
            TSType::TSFunctionType(f) => {
                for p in &f.params.items {
                    if let Some(ann) = p.type_annotation.as_deref() {
                        self.pre_resolve_aliases_in_type(
                            senv,
                            &ann.type_annotation,
                            alias_set,
                            visiting,
                            program,
                        );
                    }
                }
                if let Some(rest) = &f.params.rest
                    && let Some(ann) = rest.type_annotation.as_deref()
                {
                    self.pre_resolve_aliases_in_type(
                        senv,
                        &ann.type_annotation,
                        alias_set,
                        visiting,
                        program,
                    );
                }
                self.pre_resolve_aliases_in_type(
                    senv,
                    &f.return_type.type_annotation,
                    alias_set,
                    visiting,
                    program,
                );
            }
            _ => {}
        }
    }

    fn record_alias_cycle(&mut self, senv: &mut SkeletonEnv, name: &str, program: &Program<'_>) {
        let span = Self::find_alias_span(name, program).unwrap_or_else(|| {
            let end = self.source_len;
            CoreSpan::new(0, end)
        });
        senv.diagnostics.push(Diagnostic::warning(
            ALIAS_CYCLE_CODE,
            format!("type alias `{name}` participates in a recursive cycle"),
            span,
        ));
    }

    fn find_alias_span(name: &str, program: &Program<'_>) -> Option<CoreSpan> {
        for stmt in &program.body {
            let span = stmt.span();
            if let Some(decl) = stmt.as_declaration()
                && let Declaration::TSTypeAliasDeclaration(a) = decl
                && a.id.name == name
            {
                return Some(CoreSpan::new(span.start, span.end));
            }
            if let Some(m) = stmt.as_module_declaration()
                && let oxc_ast::ast::ModuleDeclaration::ExportNamedDeclaration(e) = m
                && let Some(Declaration::TSTypeAliasDeclaration(a)) = e.declaration.as_ref()
                && a.id.name == name
            {
                return Some(CoreSpan::new(span.start, span.end));
            }
        }
        None
    }

    pub(crate) fn walk_top_level(&mut self, senv: &mut SkeletonEnv, stmt: &Statement<'_>) {
        if let Some(decl) = stmt.as_declaration() {
            self.walk_declaration(senv, decl);
        } else if let Some(m) = stmt.as_module_declaration() {
            self.walk_module_declaration(senv, m);
        } else if let Statement::ExpressionStatement(expr_stmt) = stmt {
            if self.module {
                self.walk_tla_expression_statement(senv, expr_stmt);
            }
        } else if !matches!(stmt, Statement::EmptyStatement(_)) {
            senv.report_unsupported(
                "E0300",
                "top-level statement is not supported by foundation pass",
                stmt.span(),
            );
        }
    }

    fn walk_tla_expression_statement(
        &mut self,
        senv: &mut SkeletonEnv,
        expr_stmt: &oxc_ast::ast::ExpressionStatement<'_>,
    ) {
        let expr = self.walk_top_level_expr(senv, &expr_stmt.expression);
        self.tla_main_body.push(TlaMainItem::Expr(expr));
    }

    fn walk_top_level_expr(&mut self, senv: &mut SkeletonEnv, e: &Expression<'_>) -> HirExpr {
        self.is_generator_stack.push(false);
        self.module_init_depth = self.module_init_depth.saturating_add(1);
        let mut scope = std::mem::replace(&mut self.tla_top_level_scope, BodyScope::new(0));
        let result = self.walk_expr(senv, e, &mut scope);
        self.tla_top_level_scope = scope;
        self.module_init_depth = self.module_init_depth.saturating_sub(1);
        self.is_generator_stack.pop();
        result
    }

    pub(crate) fn walk_top_level_init(
        &mut self,
        senv: &mut SkeletonEnv,
        e: &Expression<'_>,
    ) -> HirExpr {
        self.walk_top_level_expr(senv, e)
    }

    pub(crate) fn merged_type_params(&self) -> Option<TypeParamMap> {
        if self.type_param_stack.is_empty() {
            return None;
        }
        let mut merged = TypeParamMap::new();
        for map in &self.type_param_stack {
            for (k, v) in map.iter_bindings() {
                merged.bind(k, v);
            }
        }
        Some(merged)
    }

    pub(super) fn narrowed_storage(&self, narrowed: LocalId) -> Option<(LocalId, TypeId)> {
        self.narrowed_stack
            .last()
            .and_then(|frame| frame.get(&narrowed).copied())
    }

    pub(super) fn declared_name_is_taken(&self, name: &str) -> bool {
        self.declared_names_stack
            .last()
            .is_some_and(|names| names.contains(name))
    }

    pub(super) fn visible_generator_alias(&self, name: &str) -> Option<LocalGeneratorAlias> {
        self.generator_alias_stack
            .iter()
            .rev()
            .find_map(|frame| frame.get(name))
            .cloned()
    }

    pub(super) fn generator_alias_declared_here(&self, name: &str) -> bool {
        self.generator_alias_stack
            .last()
            .is_some_and(|frame| frame.contains_key(name))
    }

    pub(super) fn generator_alias_shadows_outer(&self, name: &str) -> bool {
        self.generator_alias_stack
            .iter()
            .rev()
            .skip(1)
            .any(|frame| frame.contains_key(name))
    }

    pub(super) fn captured_name_is_visible(senv: &SkeletonEnv<'_>, name: &str) -> bool {
        senv.capture_types
            .last()
            .is_some_and(|types| types.contains_key(name))
    }

    pub(crate) fn push_pending_closure_captures(
        &mut self,
        scope: &BodyScope,
        narrowed: &HashMap<LocalId, (LocalId, TypeId)>,
    ) {
        self.pending_closure_captures.push(PendingClosureCaptures {
            enclosing_body_id: scope.body_id(),
            closure_depth: self.body_depth.saturating_add(1),
            enclosing_bindings: crate::expr::capture_binding_map(scope, narrowed),
            required: Vec::new(),
        });
    }

    pub(crate) fn finish_pending_closure_captures(&mut self, captures: &mut Vec<HirExpr>) {
        let Some(pending) = self.pending_closure_captures.pop() else {
            return;
        };
        for (id, ty, span) in pending.required {
            let present = captures.iter().any(
                |capture| matches!(capture, HirExpr::Local { id: existing, .. } if existing.raw() == id.raw()),
            );
            if !present {
                captures.push(HirExpr::Local { id, ty, span });
            }
        }
    }
}
