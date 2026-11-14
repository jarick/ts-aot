use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use ts_aot_core::{
    Atom, FieldId, FunctionId, LocalId, ModuleId, Span, StructId, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{HirCallee, HirClass, HirDecl, HirExpr, HirProgram};

use ts_aot_ir_mir::{MirExpr, MirLocalDecl};

use crate::PassContext;
use crate::hir_to_mir::{PLACEHOLDER_FUNCTION, qualified_name};

/// The callee key the throws fixpoint records for a call.
///
/// A *declared* callee is looked up by its exact key: the free-function
/// declaration the call selects, a constructor, or a method whose receiver class
/// is known statically. A method whose receiver class is *not* known is only a
/// bare name, and the fixpoint falls back to a `::name` suffix match over the
/// method keys - a fallback that must not apply to free functions, whose name may
/// coincide with a method's.
enum RecordedCallee {
    Declared(Atom),
    Method(Atom),
}

/// The key of the free-function declaration a call to `name` selects: the
/// innermost namespace that declares it wins, exactly as `resolve_callee`
/// resolves the call.
///
/// `declarations` holds every function and method keyed the way the throws map
/// is, so membership answers "is this declared?" - resolution must not depend on
/// the declaration being a *throwing* one.
fn resolve_free_function(
    namespace_path: &[String],
    name: &str,
    declarations: &HashMap<Atom, TypeId>,
) -> Option<Atom> {
    for depth in (0..=namespace_path.len()).rev() {
        let probe = qualified_name(&namespace_path[..depth], name);
        if declarations.contains_key(&probe) {
            return Some(probe);
        }
    }
    None
}

/// Error type of every function that can throw, computed as a fixpoint over the
/// call graph: a function that calls a throwing function can throw the same
/// error itself (and so does its caller, transitively). Without this the callee
/// returned a `Result` while the caller was emitted as a plain function, which
/// only showed up as a type error inside the generated Rust.
pub(super) fn module_throws(program: &HirProgram, types: &mut TypeTable) -> HashMap<Atom, TypeId> {
    use ts_aot_ir_hir::{HirCallee, HirDecl, Visitor, walk_expr};

    struct Callees<'a> {
        out: Vec<RecordedCallee>,
        class_keys: &'a HashMap<TypeId, Atom>,
        declarations: &'a HashMap<Atom, TypeId>,
        types: &'a TypeTable,
        namespace_path: &'a [String],
    }
    impl Visitor for Callees<'_> {
        fn visit_stmt(&mut self, stmt: &ts_aot_ir_hir::HirStmt) {
            if let ts_aot_ir_hir::HirStmt::Try { catch, finally, .. } = stmt {
                if let Some(catch) = catch {
                    self.visit_stmt(&catch.body);
                }
                if let Some(finally) = finally {
                    self.visit_stmt(finally);
                }
            } else {
                ts_aot_ir_hir::walk_stmt(self, stmt);
            }
        }

        fn visit_expr(&mut self, expr: &HirExpr) {
            if let HirExpr::Closure { captures, .. } = expr {
                for capture in captures {
                    self.visit_expr(capture);
                }
                return;
            }
            if let HirExpr::New { ty, .. } = expr
                && let Some(class) = self.class_keys.get(&unwrap_optional_ty(*ty, self.types))
            {
                // A throwing constructor makes its caller throwing as well: its
                // `Result` is propagated like any other callee's.
                self.out.push(RecordedCallee::Declared(
                    crate::hir_to_mir::program::extend_qualified(class, "constructor"),
                ));
            }
            if let HirExpr::Call {
                callee: HirCallee::Indirect(inner),
                ..
            } = expr
            {
                match inner.as_ref() {
                    HirExpr::Global { name, .. } => {
                        // Only the declaration the call *selects* is recorded: a
                        // module-level function of the same name is a different
                        // callee and must not supply its error type.
                        self.out.push(RecordedCallee::Declared(
                            resolve_free_function(
                                self.namespace_path,
                                name.as_str(),
                                self.declarations,
                            )
                            .unwrap_or_else(|| name.clone()),
                        ));
                    }
                    // A method call is qualified by the receiver's class when it
                    // is known statically, so same-named methods of different
                    // classes are not confused; otherwise only the bare name is
                    // known and the fixpoint matches `C::m` by suffix.
                    HirExpr::Field {
                        owner, field_name, ..
                    } => {
                        let owner_ty = unwrap_optional_ty(owner.ty(), self.types);
                        let qualified = self.class_keys.get(&owner_ty).map(|class| {
                            crate::hir_to_mir::program::extend_qualified(class, field_name.as_str())
                        });
                        self.out.push(match qualified {
                            Some(key) => RecordedCallee::Declared(key),
                            None => RecordedCallee::Method(field_name.clone()),
                        });
                    }
                    _ => {}
                }
            }
            walk_expr(self, expr);
        }
    }

    /// The class behind a receiver's static type: `b: B | null` still dispatches
    /// to `B::m`, so the optional (and a single-member union) is unwrapped before
    /// the class lookup - otherwise the key falls back to an ambiguous suffix
    /// match on the bare method name.
    fn unwrap_optional_ty(mut ty: TypeId, types: &TypeTable) -> TypeId {
        loop {
            match types.resolve(ty) {
                Some(Type::Optional { inner, .. }) => ty = *inner,
                Some(Type::Union { variants }) if variants.len() == 1 => ty = variants[0],
                _ => return ty,
            }
        }
    }

    /// The declared return type of the callee a `throw <call>` invokes.
    ///
    /// `throw boom()` carries `boom`'s error when it throws and its return value
    /// otherwise; either way the caller's error type is the callee's declared
    /// return type, so the fixpoint can seed it before the call graph is walked
    /// instead of leaving the operand as `Type::Error`.
    fn thrown_call_return_ty(
        body: &[ts_aot_ir_hir::HirStmt],
        returns: &HashMap<Atom, TypeId>,
        class_keys: &HashMap<TypeId, Atom>,
        namespace_path: &[String],
        types: &TypeTable,
    ) -> Option<TypeId> {
        let expr = crate::hir_to_mir::program::thrown_call_operand(body)?;
        let HirExpr::Call {
            callee: HirCallee::Indirect(inner),
            ..
        } = expr
        else {
            return None;
        };
        let ty = match inner.as_ref() {
            // The same enclosing-namespace resolution `resolve_callee` applies:
            // the innermost namespace that declares the function wins, so a
            // `throw boom()` inside a nested namespace seeds the function the call
            // actually selects (`N::boom`, not the bare `boom`).
            HirExpr::Global { name, .. } => {
                resolve_free_function(namespace_path, name.as_str(), returns)
                    .and_then(|key| returns.get(&key).copied())
            }
            HirExpr::Field {
                owner, field_name, ..
            } => {
                let class = class_keys.get(&unwrap_optional_ty(owner.ty(), types))?;
                returns
                    .get(&crate::hir_to_mir::program::extend_qualified(
                        class,
                        field_name.as_str(),
                    ))
                    .copied()
            }
            _ => None,
        }?;
        // An unresolved type is not an error type: leave the entry out rather
        // than seeding the fixpoint with `Error`.
        match types.resolve(ty) {
            None | Some(Type::Error | Type::GenericParam { .. }) => None,
            _ => Some(ty),
        }
    }

    fn collect(
        decls: &[HirDecl],
        namespace_path: &mut Vec<String>,
        explicit: &mut HashMap<Atom, TypeId>,
        callees: &mut Vec<(Atom, Vec<RecordedCallee>)>,
        class_keys: &HashMap<TypeId, Atom>,
        returns: &HashMap<Atom, TypeId>,
        types: &mut TypeTable,
    ) {
        for decl in decls {
            match decl {
                HirDecl::Function(f) => {
                    let key = qualified_name(namespace_path, f.name.as_str());
                    if let Some(throws) = f
                        .throws
                        .or_else(|| crate::hir_to_mir::program::body_throws_type(&f.body, types))
                        .or_else(|| {
                            thrown_call_return_ty(
                                &f.body,
                                returns,
                                class_keys,
                                namespace_path,
                                types,
                            )
                        })
                    {
                        explicit.insert(key.clone(), throws);
                    }
                    let mut visitor = Callees {
                        out: Vec::new(),
                        class_keys,
                        declarations: returns,
                        types,
                        namespace_path,
                    };
                    visitor.visit_block(&f.body);
                    callees.push((key, visitor.out));
                }
                // Methods live in the class declaration, not in `Function`: a
                // `this.m()`/`c.m()` call resolves through the class-qualified
                // key (`C::m`).
                HirDecl::Class(c) => {
                    let class_key = qualified_name(namespace_path, c.name.as_str());
                    for method in &c.methods {
                        let key = crate::hir_to_mir::program::extend_qualified(
                            &class_key,
                            method.name.as_str(),
                        );
                        if let Some(throws) = method
                            .throws
                            .or_else(|| {
                                crate::hir_to_mir::program::body_throws_type(&method.body, types)
                            })
                            .or_else(|| {
                                thrown_call_return_ty(
                                    &method.body,
                                    returns,
                                    class_keys,
                                    namespace_path,
                                    types,
                                )
                            })
                        {
                            explicit.insert(key.clone(), throws);
                        }
                        let mut visitor = Callees {
                            out: Vec::new(),
                            class_keys,
                            declarations: returns,
                            types,
                            namespace_path,
                        };
                        visitor.visit_block(&method.body);
                        callees.push((key, visitor.out));
                    }
                }
                HirDecl::Namespace { name, members } => {
                    namespace_path.push(name.as_str().to_owned());
                    collect(
                        members,
                        namespace_path,
                        explicit,
                        callees,
                        class_keys,
                        returns,
                        types,
                    );
                    namespace_path.pop();
                }
                _ => {}
            }
        }
    }

    // Class name by the type of an instance: `new C()` and a `c.m()` receiver are
    // qualified through it.
    let mut class_keys: HashMap<TypeId, Atom> = HashMap::new();
    collect_class_keys(&program.declarations, &mut Vec::new(), &mut class_keys);
    // Declared return types, keyed like the throws map: a `throw <call>` operand
    // has no HIR type of its own and is seeded from its callee.
    let mut returns: HashMap<Atom, TypeId> = HashMap::new();
    collect_return_types(&program.declarations, &mut Vec::new(), &mut returns);
    let mut explicit: HashMap<Atom, TypeId> = HashMap::new();
    // Declaration order, so which callee's error type a function inherits is
    // deterministic.
    let mut callees: Vec<(Atom, Vec<RecordedCallee>)> = Vec::new();
    let mut namespace_path = Vec::new();
    collect(
        &program.declarations,
        &mut namespace_path,
        &mut explicit,
        &mut callees,
        &class_keys,
        &returns,
        types,
    );
    // The suffix fallback below has to recognise a class prefix; a set keeps that
    // check out of the candidate scan (it runs for every unresolved method call,
    // on every fixpoint round).
    let class_names: HashSet<&str> = class_keys.values().map(|name| name.as_str()).collect();
    let mut throws = explicit;
    // Bounded by the number of functions: each round can only add entries.
    for _ in 0..=callees.len() {
        let mut changed = false;
        for (name, callee_names) in &callees {
            if throws.contains_key(name) {
                continue;
            }
            if let Some(ty) = callee_names.iter().find_map(|callee| match callee {
                // A free function (a constructor, or a method on a statically
                // known class) is looked up by its exact key: a same-named method
                // must not supply its error type.
                RecordedCallee::Declared(key) => throws.get(key).copied(),
                // A method whose receiver class is not known statically is
                // matched by its `::name` suffix only: a *bare* name in the map
                // belongs to a free function, which this call does not invoke, so
                // it must not supply the error type.
                RecordedCallee::Method(name) => {
                    // A method named `m` may be declared by several classes; pick
                    // the first class-qualified key so the choice is stable. The
                    // key's prefix before `::name` has to be a class of this
                    // program: a namespace-qualified *free* function (`ns::m`) ends
                    // with the same suffix but is not a method this call reaches.
                    let suffix = format!("::{name}");
                    let mut candidates: Vec<&Atom> = throws
                        .keys()
                        .filter(|key| {
                            let Some(class) = key.as_str().strip_suffix(suffix.as_str()) else {
                                return false;
                            };
                            class_names.contains(class)
                        })
                        .collect();
                    candidates.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                    candidates.first().and_then(|key| throws.get(*key).copied())
                }
            }) {
                throws.insert(name.clone(), ty);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    throws
}
/// Declared return type of every function and method, keyed the way the throws
/// map is keyed (a bare/namespace-qualified name, or `Class::method`).
fn collect_return_types(
    decls: &[HirDecl],
    namespace_path: &mut Vec<String>,
    out: &mut HashMap<Atom, TypeId>,
) {
    for decl in decls {
        match decl {
            HirDecl::Function(f) => {
                out.insert(qualified_name(namespace_path, f.name.as_str()), f.ret);
            }
            HirDecl::Class(c) => {
                let class_key = qualified_name(namespace_path, c.name.as_str());
                for method in &c.methods {
                    out.insert(
                        crate::hir_to_mir::program::extend_qualified(
                            &class_key,
                            method.name.as_str(),
                        ),
                        method.ret,
                    );
                }
            }
            HirDecl::Namespace { name, members } => {
                namespace_path.push(name.as_str().to_owned());
                collect_return_types(members, namespace_path, out);
                namespace_path.pop();
            }
            _ => {}
        }
    }
}

/// Class name by the type of an instance of that class (nested namespaces
/// included): used by the throws fixpoint to qualify `new C()` and `c.m()`.
fn collect_class_keys(
    decls: &[HirDecl],
    namespace_path: &mut Vec<String>,
    out: &mut HashMap<TypeId, Atom>,
) {
    for decl in decls {
        match decl {
            HirDecl::Class(c) => {
                out.insert(c.ty, qualified_name(namespace_path, c.name.as_str()));
            }
            HirDecl::Namespace { name, members } => {
                namespace_path.push(name.as_str().to_owned());
                collect_class_keys(members, namespace_path, out);
                namespace_path.pop();
            }
            _ => {}
        }
    }
}

pub struct ExprConverter {
    pub(super) local_map: HashMap<LocalId, LocalId>,
    pub(super) local_names: HashMap<LocalId, Atom>,
    pub(super) function_remap: HashMap<FunctionId, FunctionId>,
    pub(super) name_to_function: Arc<HashMap<Atom, FunctionId>>,
    pub(super) program: Arc<HirProgram>,
    pub(super) namespace_path: Vec<String>,
    pub(super) next_local: u32,
    pub(super) temp_locals: Vec<MirLocalDecl>,
    pub(super) struct_ids: HashMap<TypeId, StructId>,
    pub(super) field_id_lookup: HashMap<(StructId, Atom), FieldId>,
    pub(super) method_map: HashMap<(StructId, Atom, bool), FunctionId>,
    pub(super) method_ret: HashMap<(StructId, Atom, bool), TypeId>,
    pub(super) method_param_tys: HashMap<(StructId, Atom, bool), Vec<TypeId>>,
    pub(super) method_param_optional: HashMap<(StructId, Atom, bool), Vec<bool>>,
    pub(super) class_struct_by_name: HashMap<Atom, StructId>,
    pub(super) current_call_type_args: Vec<TypeId>,
    pub(super) current_return_ty: Option<TypeId>,
    pub(super) expected_ty: Option<TypeId>,
    pub(super) class_index: OnceLock<HashMap<TypeId, ClassPath>>,
    pub(super) global_types: OnceLock<HashMap<Atom, TypeId>>,
    /// Error type of every function that can throw, propagated along the call
    /// graph (see `module_throws`): a caller of a throwing function must itself
    /// return a `Result` of the same error type, otherwise the call cannot be
    /// lowered.
    pub(super) throws_by_name: OnceLock<HashMap<Atom, TypeId>>,
    /// Error type of the function whose body is being converted (`None` outside
    /// a `Result`-returning function, in generators and in closure bodies).
    pub(super) current_throws: Option<TypeId>,
    /// How many `try` blocks enclose the expression being converted: a callee's
    /// error cannot be caught by this AOT target, so a throwing call there is
    /// rejected instead of silently escaping the `try`.
    pub(super) try_depth: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClassPath(pub(crate) Vec<usize>);

impl ExprConverter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_function_remap(HashMap::new())
    }

    #[must_use]
    pub fn with_function_remap(remap: HashMap<FunctionId, FunctionId>) -> Self {
        Self::with_function_remap_and_offset(remap, 0)
    }

    #[must_use]
    pub fn with_function_remap_and_offset(
        remap: HashMap<FunctionId, FunctionId>,
        next_local: u32,
    ) -> Self {
        Self {
            local_map: HashMap::new(),
            local_names: HashMap::new(),
            function_remap: remap,
            name_to_function: Arc::new(HashMap::new()),
            program: Arc::new(HirProgram::new(ModuleId::from_raw(0))),
            namespace_path: Vec::new(),
            next_local,
            temp_locals: Vec::new(),
            struct_ids: HashMap::new(),
            field_id_lookup: HashMap::new(),
            method_map: HashMap::new(),
            method_ret: HashMap::new(),
            method_param_tys: HashMap::new(),
            method_param_optional: HashMap::new(),
            class_struct_by_name: HashMap::new(),
            current_call_type_args: Vec::new(),
            current_return_ty: None,
            expected_ty: None,
            class_index: OnceLock::new(),
            global_types: OnceLock::new(),
            throws_by_name: OnceLock::new(),
            current_throws: None,
            try_depth: 0,
        }
    }

    pub fn set_field_id_lookup(&mut self, lookup: HashMap<(StructId, Atom), FieldId>) {
        self.field_id_lookup = lookup;
    }

    pub(crate) fn set_program_tables(
        &mut self,
        field_id_lookup: HashMap<(StructId, Atom), FieldId>,
        method_map: HashMap<(StructId, Atom, bool), FunctionId>,
        method_ret: HashMap<(StructId, Atom, bool), TypeId>,
        method_param_tys: HashMap<(StructId, Atom, bool), Vec<TypeId>>,
        method_param_optional: HashMap<(StructId, Atom, bool), Vec<bool>>,
        class_struct_by_name: HashMap<Atom, StructId>,
    ) {
        self.field_id_lookup = field_id_lookup;
        self.method_map = method_map;
        self.method_ret = method_ret;
        self.method_param_tys = method_param_tys;
        self.method_param_optional = method_param_optional;
        self.class_struct_by_name = class_struct_by_name;
    }

    pub fn set_namespace_path(&mut self, path: &[String]) {
        self.namespace_path = path.to_vec();
    }

    pub fn set_program(&mut self, program: Arc<HirProgram>) {
        self.program = program;
        self.class_index = OnceLock::new();
        self.global_types = OnceLock::new();
        self.throws_by_name = OnceLock::new();
    }

    /// Computes the call-graph error types once (`module_throws` needs the type
    /// table, so it cannot run inside the `OnceLock` closure).
    pub(super) fn ensure_throws_types(&mut self, types: &mut TypeTable) {
        if self.throws_by_name.get().is_none() {
            let map = module_throws(&self.program, types);
            let _ = self.throws_by_name.set(map);
        }
    }

    /// Whether `name` is a function that may throw, and with which error type.
    pub(super) fn function_throws(&self, name: &Atom) -> Option<TypeId> {
        let map = self.throws_by_name.get()?;
        for depth in (0..=self.namespace_path.len()).rev() {
            let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
            if let Some(&ty) = map.get(&probe) {
                return Some(ty);
            }
        }
        map.get(name).copied()
    }

    pub(super) fn set_current_throws(&mut self, throws: Option<TypeId>) {
        self.current_throws = throws;
    }

    pub(super) fn global_decl_type(&self, name: &Atom) -> Option<TypeId> {
        let map = self.global_types.get_or_init(|| {
            let mut map = HashMap::new();
            fn scan(
                decls: &[HirDecl],
                namespace_path: &mut Vec<String>,
                map: &mut HashMap<Atom, TypeId>,
            ) {
                for decl in decls {
                    match decl {
                        HirDecl::Global { name, ty, .. } => {
                            map.insert(qualified_name(namespace_path, name.as_str()), *ty);
                        }
                        HirDecl::Namespace { name, members } => {
                            namespace_path.push(name.as_str().to_owned());
                            scan(members, namespace_path, map);
                            namespace_path.pop();
                        }
                        _ => {}
                    }
                }
            }
            let mut namespace_path = Vec::new();
            scan(&self.program.declarations, &mut namespace_path, &mut map);
            map
        });
        for depth in (0..=self.namespace_path.len()).rev() {
            let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
            if let Some(&ty) = map.get(&probe) {
                return Some(ty);
            }
        }
        None
    }

    pub(super) fn take_temp_locals(&mut self) -> Vec<MirLocalDecl> {
        std::mem::take(&mut self.temp_locals)
    }

    pub(super) fn push_temp_local(&mut self, id: LocalId, ty: TypeId) {
        self.push_temp_local_with_mut(id, ty, true);
    }

    pub(super) fn push_temp_local_with_mut(&mut self, id: LocalId, ty: TypeId, mutable: bool) {
        self.temp_locals.push(MirLocalDecl {
            id,
            name: Atom::from(""),
            ty,
            mutable,
        });
    }

    #[must_use]
    pub fn peek_next_local(&self) -> u32 {
        self.next_local
    }

    pub(super) fn fresh_local(&mut self) -> LocalId {
        let id = LocalId::from_raw(self.next_local);
        self.next_local += 1;
        id
    }

    pub(super) fn map_local(&mut self, old: LocalId) -> MirExpr {
        if let Some(&new) = self.local_map.get(&old) {
            MirExpr::Local(new)
        } else {
            let new_id = self.fresh_local();
            self.local_map.insert(old, new_id);
            MirExpr::Local(new_id)
        }
    }

    #[must_use]
    pub fn map_local_id(&mut self, old: LocalId) -> LocalId {
        if let Some(&new) = self.local_map.get(&old) {
            new
        } else {
            let new_id = self.fresh_local();
            self.local_map.insert(old, new_id);
            new_id
        }
    }

    pub fn map_local_id_inplace(&mut self, old: LocalId, new: LocalId) {
        self.local_map.insert(old, new);
    }

    pub fn register_local_name(&mut self, id: LocalId, name: Atom) {
        self.local_names.insert(id, name);
    }

    pub fn local_name(&self, id: LocalId) -> Option<Atom> {
        self.local_names.get(&id).cloned()
    }

    pub(super) fn local_name_taken(&self, name: &str) -> bool {
        self.local_names
            .values()
            .any(|existing| existing.as_str() == name)
    }

    pub(super) fn unique_synth_local_name(&mut self, id: LocalId, base: &str) -> Atom {
        let mut name = format!("{}_{}", base, id.raw());
        while self.local_name_taken(&name) {
            name.push('_');
        }
        let name = Atom::from(name);
        self.register_local_name(id, name.clone());
        name
    }

    pub fn seed_params(&mut self, param_ids: &[LocalId]) {
        for &id in param_ids {
            self.local_map.insert(id, id);
        }
        let next = param_ids
            .iter()
            .map(|id| id.raw())
            .max()
            .map_or(0, |m| m.saturating_add(1));
        if next > self.next_local {
            self.next_local = next;
        }
    }

    pub(crate) fn reset_for_function(
        &mut self,
        function_remap: HashMap<FunctionId, FunctionId>,
        param_ids: &[LocalId],
        namespace_path: &[String],
    ) {
        self.local_map.clear();
        self.local_names.clear();
        self.function_remap = function_remap;
        self.namespace_path.clear();
        self.namespace_path.extend_from_slice(namespace_path);
        self.next_local = param_ids
            .iter()
            .map(|id| id.raw())
            .max()
            .map_or(0, |m| m.saturating_add(1));
        self.temp_locals.clear();
        self.struct_ids.clear();
        self.current_call_type_args.clear();
        self.current_return_ty = None;
        self.expected_ty = None;
        self.seed_params(param_ids);
    }

    pub(crate) fn set_return_ty(&mut self, ty: TypeId) {
        self.current_return_ty = Some(ty);
    }

    pub(super) fn resolve_callee(
        &mut self,
        callee: &HirCallee,
        ctx: &mut PassContext,
    ) -> FunctionId {
        match callee {
            HirCallee::Function(fid) => self.function_remap.get(fid).copied().unwrap_or(*fid),
            HirCallee::Indirect(inner) => {
                if let HirExpr::Global { name, .. } = inner.as_ref() {
                    for depth in (0..=self.namespace_path.len()).rev() {
                        let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
                        if let Some(&fid) = self.name_to_function.get(&probe) {
                            return fid;
                        }
                    }
                }
                ctx.warning(
                    "P0005",
                    "indirect (computed) callee resolves to MirExpr::IndirectCall at HIR→MIR; \
                     backend emits callee(args) directly. \
                     OptionalChain callees (e.g. obj?.()) go through optional_call_map_arm when ty is Optional.",
                    Span::new(0, 0),
                );
                PLACEHOLDER_FUNCTION
            }
            HirCallee::Closure(_) => {
                ctx.error(
                    "P0005",
                    "closure callee should have been rewritten to Indirect(Global) by lower_closures before HIR→MIR",
                    Span::new(0, 0),
                );
                PLACEHOLDER_FUNCTION
            }
            HirCallee::Runtime { .. } => {
                ctx.error(
                    "P0005",
                    "runtime callee is not yet supported in HIR→MIR",
                    Span::new(0, 0),
                );
                PLACEHOLDER_FUNCTION
            }
        }
    }

    pub(super) fn lookup_class_struct_by_name(&self, name: &Atom) -> Option<StructId> {
        for depth in (0..=self.namespace_path.len()).rev() {
            let probe = qualified_name(&self.namespace_path[..depth], name.as_str());
            if let Some(&sid) = self.class_struct_by_name.get(&probe) {
                return Some(sid);
            }
        }
        None
    }

    pub(super) fn lookup_or_alloc_struct_id(
        &mut self,
        ty: TypeId,
        shared_ids: &mut HashMap<TypeId, StructId>,
        shared_next: &mut u32,
    ) -> StructId {
        if let Some(&id) = self.struct_ids.get(&ty) {
            return id;
        }
        if let Some(&id) = shared_ids.get(&ty) {
            self.struct_ids.insert(ty, id);
            return id;
        }
        let id = StructId::from_raw(*shared_next);
        *shared_next += 1;
        shared_ids.insert(ty, id);
        self.struct_ids.insert(ty, id);
        id
    }

    fn global_owner_ty(&self, name: &Atom, ty: TypeId, types: &TypeTable) -> TypeId {
        if matches!(types.resolve(ty), Some(Type::Error)) {
            self.global_decl_type(name).unwrap_or(ty)
        } else {
            ty
        }
    }

    pub(super) fn owner_struct_id(
        &self,
        owner: &HirExpr,
        shared_ids: &HashMap<TypeId, StructId>,
        types: &TypeTable,
    ) -> Option<StructId> {
        let owner_ty = match owner {
            HirExpr::Global { name, ty, .. } => self.global_owner_ty(name, *ty, types),
            _ => owner.ty(),
        };
        let owner_ty = match types.resolve(owner_ty) {
            Some(Type::Optional { inner, .. }) => *inner,
            _ => owner_ty,
        };
        self.struct_ids
            .get(&owner_ty)
            .or_else(|| shared_ids.get(&owner_ty))
            .copied()
    }

    pub(super) fn resolve_field_id(
        &self,
        owner: &HirExpr,
        field_name: &Atom,
        placeholder: FieldId,
        shared_ids: &HashMap<TypeId, StructId>,
        types: &TypeTable,
        ctx: &mut PassContext,
    ) -> FieldId {
        let owner_ty = match owner {
            HirExpr::OptionalChain { .. } | HirExpr::Field { .. } => {
                let declarations = &self.program.declarations;
                let class_index = self
                    .class_index
                    .get_or_init(|| build_class_index(declarations));
                let resolve_global = |name: &Atom| self.global_decl_type(name);
                compute_chain_owner_ty(owner, class_index, declarations, types, &resolve_global)
            }
            HirExpr::Global { name, ty, .. } => Some(self.global_owner_ty(name, *ty, types)),
            HirExpr::Local { ty, .. }
            | HirExpr::Index { ty, .. }
            | HirExpr::Call { ty, .. }
            | HirExpr::Binary { ty, .. }
            | HirExpr::Unary { ty, .. }
            | HirExpr::StructLiteral { ty, .. }
            | HirExpr::ArrayLiteral { ty, .. }
            | HirExpr::Closure { ty, .. }
            | HirExpr::Await { ty, .. }
            | HirExpr::Yield { ty, .. }
            | HirExpr::Template { ty, .. }
            | HirExpr::New { ty, .. }
            | HirExpr::Assignment { ty, .. }
            | HirExpr::CompoundUpdate { ty, .. } => Some(*ty),
            HirExpr::TypeAssertion { target, .. } => Some(*target),
            _ => None,
        };
        let Some(ty) = owner_ty.map(|t| match types.resolve(t) {
            Some(Type::Optional { inner, .. }) => *inner,
            _ => t,
        }) else {
            if owner.has_static_type() && matches!(types.resolve(owner.ty()), Some(Type::Error)) {
                ctx.warning(
                    "P0013",
                    format!(
                        "field access `{}` on an already-erroneous owner was skipped; \
                         the owner type failed to resolve earlier",
                        field_name.as_str()
                    ),
                    Span::new(0, 0),
                );
                return placeholder;
            }
            ctx.warning(
                "P0011",
                format!(
                    "owner expression has no static type for field access `{}`; the access \
                     is lowered with a placeholder field",
                    field_name.as_str()
                ),
                Span::new(0, 0),
            );
            return placeholder;
        };
        if matches!(types.resolve(ty), Some(Type::AsyncGenYield { .. })) {
            return match field_name.as_str() {
                "value" => FieldId::from_raw(0),
                "done" => FieldId::from_raw(1),
                _ => {
                    ctx.warning(
                        "P0012",
                        format!(
                            "`AsyncGenYield` has no field `{}`; expected `value` or `done`",
                            field_name.as_str()
                        ),
                        Span::new(0, 0),
                    );
                    placeholder
                }
            };
        }
        if matches!(types.resolve(ty), Some(Type::Error)) {
            ctx.warning(
                "P0013",
                format!(
                    "field access `{}` on an already-erroneous owner was skipped; \
                     the owner type failed to resolve earlier",
                    field_name.as_str()
                ),
                Span::new(0, 0),
            );
            return placeholder;
        }
        let Some(&sid) = self.struct_ids.get(&ty).or_else(|| shared_ids.get(&ty)) else {
            ctx.warning(
                "P0012",
                format!(
                    "owner type {:?} has no struct representation for field access; the \
                     access is lowered with a placeholder field",
                    ty
                ),
                Span::new(0, 0),
            );
            return placeholder;
        };
        match self.field_id_lookup.get(&(sid, field_name.clone())) {
            Some(id) => *id,
            None if is_object_prototype_method(field_name) => {
                ctx.error(
                    "E0407",
                    format!(
                        "Object.prototype method `{}` cannot be used as a bare field value; \
                         invoke it on a struct receiver instead",
                        field_name.as_str()
                    ),
                    Span::new(0, 0),
                );
                placeholder
            }
            // The access is `undefined`, which this backend does not model, so it is lowered
            // with a sentinel field id that no struct declares rather than the struct's own
            // placeholder: a real id would silently read whichever field carries it (`c.k`
            // reading `c.x`), while the sentinel makes a generated crate fail to compile.
            None => {
                ctx.warning(
                    "P0010",
                    format!(
                        "field `{}` is not declared on the resolved type of the owner; the \
                         access is lowered with a placeholder field",
                        field_name.as_str()
                    ),
                    Span::new(0, 0),
                );
                FieldId::from_raw(u32::MAX)
            }
        }
    }
}

impl Default for ExprConverter {
    fn default() -> Self {
        Self::new()
    }
}

const OBJECT_PROTOTYPE_METHODS: &[&str] = &["hasOwnProperty"];

fn is_object_prototype_method(field_name: &Atom) -> bool {
    OBJECT_PROTOTYPE_METHODS.contains(&field_name.as_str())
}

pub(crate) fn compute_chain_owner_ty(
    expr: &HirExpr,
    class_index: &HashMap<TypeId, ClassPath>,
    declarations: &[HirDecl],
    types: &TypeTable,
    resolve_global: &dyn Fn(&Atom) -> Option<TypeId>,
) -> Option<TypeId> {
    let class_at = |ty: TypeId| -> Option<&HirClass> {
        let path = class_index.get(&ty)?;
        class_at_path(declarations, path.clone())
    };
    match expr {
        HirExpr::OptionalChain { base, .. } => {
            let inner =
                compute_chain_owner_ty(base, class_index, declarations, types, resolve_global)?;
            Some(match types.resolve(inner) {
                Some(Type::Optional { inner, .. }) => *inner,
                _ => inner,
            })
        }
        HirExpr::Field {
            owner, field_name, ..
        } => {
            let owner_class =
                compute_chain_owner_ty(owner, class_index, declarations, types, resolve_global)?;
            let owner_class = match types.resolve(owner_class) {
                Some(Type::Optional { inner, .. }) => *inner,
                _ => owner_class,
            };
            let class = class_at(owner_class)?;
            let field_ty = class
                .fields
                .iter()
                .find(|f| f.name == *field_name)
                .map(|f| f.ty)?;
            Some(match types.resolve(field_ty) {
                Some(Type::Optional { inner, .. }) => *inner,
                _ => field_ty,
            })
        }
        HirExpr::Index { owner, .. } => {
            let owner_ty =
                compute_chain_owner_ty(owner, class_index, declarations, types, resolve_global)?;
            Some(match types.resolve(owner_ty) {
                Some(Type::Optional { inner, .. }) => *inner,
                _ => owner_ty,
            })
        }
        HirExpr::Call {
            callee: HirCallee::Indirect(inner),
            ..
        } => {
            let inner_ty =
                compute_chain_owner_ty(inner, class_index, declarations, types, resolve_global)?;
            Some(match types.resolve(inner_ty) {
                Some(Type::Optional { inner, .. }) => *inner,
                _ => inner_ty,
            })
        }
        HirExpr::Local { ty, .. } => Some(*ty),
        HirExpr::Global { name, ty, .. } => {
            if matches!(types.resolve(*ty), Some(Type::Error)) {
                Some(resolve_global(name).unwrap_or(*ty))
            } else {
                Some(*ty)
            }
        }
        _ => Some(expr.ty()),
    }
}

pub(crate) fn build_class_index(declarations: &[HirDecl]) -> HashMap<TypeId, ClassPath> {
    let mut index = HashMap::new();
    for (idx, decl) in declarations.iter().enumerate() {
        index_decl(decl, ClassPath(vec![idx]), &mut index);
    }
    index
}

pub(crate) fn index_decl(decl: &HirDecl, path: ClassPath, index: &mut HashMap<TypeId, ClassPath>) {
    match decl {
        HirDecl::Class(c) => {
            index.insert(c.ty, path.clone());
            if let Some(pre_pass_ty) = c.pre_pass_ty
                && pre_pass_ty != c.ty
            {
                index.insert(pre_pass_ty, path.clone());
            }
        }
        HirDecl::Namespace { members, .. } => {
            for (member_idx, member) in members.iter().enumerate() {
                let mut member_path = path.clone();
                member_path.0.push(member_idx);
                index_decl(member, member_path, index);
            }
        }
        _ => {}
    }
}

pub(crate) fn class_at_path(declarations: &[HirDecl], path: ClassPath) -> Option<&HirClass> {
    let (first, rest) = path.0.split_first()?;
    let mut current = declarations.get(*first)?;
    for &idx in rest {
        current = match current {
            HirDecl::Namespace { members, .. } => members.get(idx)?,
            _ => return None,
        };
    }
    match current {
        HirDecl::Class(c) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod call_graph_tests {
    use super::*;
    use ts_aot_ir_hir::{HirFunction, HirStmt};

    #[test]
    fn excludes_try_and_closure_bodies_but_records_finally_and_normal_calls() {
        let mut types = TypeTable::new();
        let number = types.intern(&Type::F64);
        let call = || HirStmt::Expr {
            expr: HirExpr::Call {
                callee: HirCallee::Indirect(Box::new(HirExpr::Global {
                    name: Atom::from("boom"),
                    ty: number,
                    span: Span::default(),
                })),
                args: Vec::new(),
                type_args: Vec::new(),
                ty: number,
                span: Span::default(),
            },
        };
        let function = |name: &str, body, throws| {
            HirDecl::Function(HirFunction {
                name: Atom::from(name),
                params: Vec::new(),
                ret: number,
                throws,
                body,
                is_async: false,
                is_generator: false,
                is_exported: false,
                type_params: Vec::new(),
                async_info: None,
            })
        };
        let mut program = HirProgram::new(ModuleId::from_raw(0));
        program.declarations = vec![
            function("boom", Vec::new(), Some(number)),
            function(
                "in_try",
                vec![HirStmt::Try {
                    body: Box::new(call()),
                    catch: None,
                    finally: None,
                }],
                None,
            ),
            function(
                "in_closure",
                vec![HirStmt::Expr {
                    expr: HirExpr::Closure {
                        id: LocalId::from_raw(0),
                        params: Vec::new(),
                        captures: Vec::new(),
                        body: vec![call()],
                        ty: number,
                        span: Span::default(),
                    },
                }],
                None,
            ),
            function(
                "in_finally",
                vec![HirStmt::Try {
                    body: Box::new(HirStmt::Block(Vec::new())),
                    catch: None,
                    finally: Some(Box::new(call())),
                }],
                None,
            ),
            function("normal", vec![call()], None),
        ];
        let throws = module_throws(&program, &mut types);
        assert!(!throws.contains_key(&Atom::from("in_try")));
        assert!(!throws.contains_key(&Atom::from("in_closure")));
        assert_eq!(throws.get(&Atom::from("in_finally")), Some(&number));
        assert_eq!(throws.get(&Atom::from("normal")), Some(&number));
    }
}
