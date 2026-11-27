use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, AssignmentExpression, AssignmentTarget,
    AssignmentTargetMaybeDefault, AssignmentTargetPattern, AssignmentTargetProperty,
    BindingPattern, ChainElement, Class, ClassElement, Declaration, Expression, ForInStatement,
    ForOfStatement, ForStatement, ForStatementInit, ForStatementLeft, FormalParameters, Function,
    MemberExpression, ModuleDeclaration, ObjectPropertyKind, PropertyKey, SimpleAssignmentTarget,
    Statement, SwitchStatement, TryStatement, UnaryOperator, VariableDeclaration,
    VariableDeclarationKind, match_assignment_target, match_declaration, match_expression,
    match_member_expression,
};
use oxc_span::GetSpan;
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator};
use ts_aot_core::{
    Atom, LocalId, NARROWED_LOCAL_PREFIX, NullishKinds, Span, Type, TypeId, TypeTable,
};
use ts_aot_ir_hir::{
    HirBinaryOp, HirCallee, HirCatchBinding, HirCatchClause, HirExpr, HirStmt, HirSwitchCase,
    HirUnaryOp, Visitor, walk_expr, walk_stmt,
};

use crate::body::LoopEffects;
use crate::ops::{label_atom, left_span};
use crate::scope::BodyScope;
use crate::skeleton::{
    ClassValueFieldBase, ESCAPING_THIS_CAPTURE_CODE, ESCAPING_THIS_CAPTURE_MESSAGE,
    LocalGeneratorAlias, ProvableArrayFact, ProvableArrayFacts, SkeletonBuilder, SkeletonEnv,
    SwitchCaseEvent, SwitchCaseEventKind,
};
use crate::util::{binding_pattern_name, collect_binding_names, core_span_from_oxc};

impl SkeletonEnv<'_> {
    pub(crate) fn expand_array_destructuring(
        &mut self,
        array_pat: &oxc_ast::ast::ArrayPattern<'_>,
        init: HirExpr,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
        merge_parameters: bool,
        declare_at_function_scope: bool,
        proved: &ProvableArrayFacts<'_>,
    ) -> Vec<LocalId> {
        let mut declared: Vec<LocalId> = Vec::new();
        let init_ty_id = init.ty();
        let Some(Type::Array { element }) = self.types.resolve(init_ty_id) else {
            self.report_unsupported(
                "E0500",
                "array destructuring requires an array type on the right-hand side",
                array_pat.span,
            );
            return declared;
        };
        let element_ty_id = *element;
        let span = core_span_from_oxc(array_pat.span);
        let present: Vec<bool> = {
            let history: Vec<ArrayStmtRef<'_>> = out.iter().map(ArrayStmtRef::Stmt).collect();
            (0..array_pat.elements.len())
                .map(|idx| {
                    array_element_definitely_present(
                        &init,
                        idx,
                        &history,
                        scope.body_id(),
                        &self.destructured_temp_ids,
                        proved,
                    )
                })
                .collect()
        };
        let destructured_uid = self.next_destructured_id.get();
        self.next_destructured_id.set(destructured_uid + 1);
        let temp_name = Atom::from(format!("_destructured_{destructured_uid}"));
        let temp_id = LocalId::from_raw(scope.peek_next());
        scope.set_next(temp_id.raw().saturating_add(1));
        self.destructured_temp_ids
            .insert((scope.body_id(), temp_id));
        out.push(HirStmt::Let {
            id: temp_id,
            name: temp_name,
            ty: init_ty_id,
            init: Some(init),
            narrowed: false,
        });
        let temp_ref = HirExpr::Local {
            id: temp_id,
            ty: init_ty_id,
            span,
        };
        for (idx, element) in array_pat.elements.iter().enumerate() {
            let Some(element) = element else {
                continue;
            };
            let BindingPattern::BindingIdentifier(id) = element else {
                self.report_unwalked(
                    "array destructuring element must be a simple binding name (no nested patterns, no defaults, no rename)",
                    element.span(),
                );
                continue;
            };
            let name_str = id.name.to_compact_str();
            let present_here = present.get(idx).copied().unwrap_or(false);
            let (init_call, binding_ty, optional_element) =
                self.destructured_element_read(&temp_ref, idx, element_ty_id, present_here, span);
            let existing = if merge_parameters {
                known_parameter_binding(self, scope, name_str.as_str())
                    .or_else(|| scope.function_scoped_binding(name_str.as_str()))
            } else {
                None
            };
            if let Some((target_id, target_ty)) = existing {
                scope.remove_predeclared_shadow(name_str.as_str());
                if optional_element
                    && !destructured_read_fits_target(
                        self.types,
                        target_ty,
                        binding_ty,
                        optional_element,
                    )
                {
                    self.report_unsupported(
                        "P0018",
                        format!(
                            "`{}` destructures an array element that may be absent, but the \
                             existing binding it writes is not optional; widen that binding's \
                             type to accept `undefined`, or rename the destructured name",
                            name_str.as_str()
                        )
                        .as_str(),
                        element.span(),
                    );
                    continue;
                }
                out.push(HirStmt::Expr {
                    expr: HirExpr::Assignment {
                        target: Box::new(HirExpr::Local {
                            id: target_id,
                            ty: target_ty,
                            span,
                        }),
                        value: Box::new(init_call),
                        ty: binding_ty,
                        span,
                    },
                });
                continue;
            }
            let new_id = if declare_at_function_scope {
                scope.declare_function_scoped(name_str.as_str(), binding_ty)
            } else {
                scope.declare(name_str.as_str(), binding_ty)
            };
            declared.push(new_id);
            out.push(HirStmt::Let {
                id: new_id,
                name: Atom::from(name_str.as_str()),
                ty: binding_ty,
                init: Some(init_call),
                narrowed: false,
            });
        }
        declared
    }

    fn element_type_absence_is_undefined(&self, ty: TypeId) -> bool {
        match self.types.resolve(ty) {
            Some(Type::Void) => true,
            Some(Type::Optional { nullish, .. }) => nullish.undefined,
            Some(Type::Union { variants }) => variants
                .iter()
                .any(|variant| self.element_type_absence_is_undefined(*variant)),
            _ => false,
        }
    }

    fn destructured_element_read(
        &mut self,
        temp_ref: &HirExpr,
        idx: usize,
        element_ty_id: TypeId,
        present: bool,
        span: Span,
    ) -> (HirExpr, TypeId, bool) {
        let optional_element = !present && !self.element_type_absence_is_undefined(element_ty_id);
        let binding_ty = if optional_element {
            self.types.intern(&Type::Optional {
                inner: element_ty_id,
                nullish: NullishKinds::UNDEFINED_ONLY,
            })
        } else {
            element_ty_id
        };
        let runtime_name = Atom::from(if optional_element {
            "__ts_aot_array_get_optional"
        } else {
            "__ts_aot_array_get_or_default"
        });
        let init_call = HirExpr::Call {
            callee: HirCallee::Runtime {
                name: runtime_name,
                ty: binding_ty,
            },
            args: vec![
                temp_ref.clone(),
                HirExpr::Int(
                    i64::try_from(idx).expect("array index fits in i64 (usize overflow)"),
                    span,
                ),
            ],
            type_args: Vec::new(),
            ty: binding_ty,
            span,
        };
        (init_call, binding_ty, optional_element)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ArrayStmtRef<'a> {
    Stmt(&'a HirStmt),
    Expr(&'a HirExpr),
}

#[derive(Clone, Copy)]
enum ArrayTarget<'a> {
    Locals(&'a [LocalId]),
    Global(&'a str),
}

struct ArrayWriteScan<'a> {
    target: ArrayTarget<'a>,
    index: Option<usize>,
    destructured_temps: &'a HashSet<(u32, LocalId)>,
    body_id: Option<u32>,
    found: bool,
}

impl ArrayWriteScan<'_> {
    fn is_target(&self, expr: &HirExpr) -> bool {
        match (expr, self.target) {
            (HirExpr::Local { id, .. }, ArrayTarget::Locals(ids)) => ids.contains(id),
            (HirExpr::Global { name, .. }, ArrayTarget::Global(target)) => name.as_str() == target,
            _ => false,
        }
    }

    fn writes_target(&self, target: &HirExpr) -> bool {
        let mut current = target;
        loop {
            match current {
                HirExpr::Field { owner, .. } | HirExpr::Index { owner, .. } => current = owner,
                _ => return self.is_target(current),
            }
        }
    }

    fn is_destructured_temp(&self, id: LocalId) -> bool {
        match self.body_id {
            Some(body_id) => self.destructured_temps.contains(&(body_id, id)),
            None => false,
        }
    }

    fn preserves_element(&self, method: &str) -> bool {
        method == "push" || (method == "unshift" && self.index == Some(0))
    }
}

impl Visitor for ArrayWriteScan<'_> {
    fn visit_stmt(&mut self, stmt: &HirStmt) {
        if let HirStmt::Let { id, init, .. } = stmt
            && self.is_destructured_temp(*id)
        {
            let Some(init) = init else {
                return;
            };
            if self.is_target(strip_type_assertion(init)) {
                return;
            }
            self.visit_expr(init);
            return;
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpr) {
        if self.found {
            return;
        }
        match expr {
            HirExpr::Local { .. } | HirExpr::Global { .. } => {
                if self.is_target(expr) {
                    self.found = true;
                }
            }
            HirExpr::Index { owner, index, .. } => {
                if self.is_target(owner) {
                    self.visit_expr(index);
                } else {
                    self.visit_expr(owner);
                    self.visit_expr(index);
                }
            }
            HirExpr::Field { owner, .. } => {
                if !self.is_target(owner) {
                    self.visit_expr(owner);
                }
            }
            HirExpr::Assignment { target, value, .. } => {
                if self.writes_target(target) {
                    self.found = true;
                } else {
                    self.visit_expr(target);
                    self.visit_expr(value);
                }
            }
            HirExpr::CompoundUpdate { target, rhs, .. } => {
                if self.writes_target(target) {
                    self.found = true;
                } else {
                    self.visit_expr(target);
                    self.visit_expr(rhs);
                }
            }
            HirExpr::Unary {
                op: HirUnaryOp::Delete,
                expr,
                ..
            } => {
                if self.writes_target(expr) {
                    self.found = true;
                } else {
                    self.visit_expr(expr);
                }
            }
            HirExpr::Call { callee, args, .. } => {
                if let HirCallee::Indirect(inner) = callee {
                    match strip_type_assertion(inner) {
                        HirExpr::Field {
                            owner, field_name, ..
                        } if self.is_target(owner) => {
                            if self.preserves_element(field_name.as_str()) {
                                for arg in args {
                                    self.visit_expr(arg);
                                }
                            } else {
                                self.found = true;
                            }
                            return;
                        }
                        HirExpr::Index { owner, index, .. } if self.is_target(owner) => {
                            if string_literal_index(index)
                                .is_some_and(|method| self.preserves_element(method))
                            {
                                for arg in args {
                                    self.visit_expr(arg);
                                }
                            } else {
                                self.found = true;
                            }
                            return;
                        }
                        _ => {}
                    }
                }
                for arg in args {
                    self.visit_expr(arg);
                }
                if let HirCallee::Indirect(inner) = callee {
                    self.visit_expr(inner);
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}

fn strip_type_assertion(expr: &HirExpr) -> &HirExpr {
    let mut current = expr;
    while let HirExpr::TypeAssertion { expr, .. } = current {
        current = expr;
    }
    current
}

fn string_literal_index(expr: &HirExpr) -> Option<&str> {
    match strip_type_assertion(expr) {
        HirExpr::String(name, _) => Some(name.as_str()),
        _ => None,
    }
}

fn array_writes_in_stmt(stmt: &HirStmt, scan: &mut ArrayWriteScan<'_>) -> bool {
    scan.found = false;
    scan.visit_stmt(stmt);
    scan.found
}

fn array_writes_in_history(stmt: &ArrayStmtRef<'_>, scan: &mut ArrayWriteScan<'_>) -> bool {
    scan.found = false;
    match stmt {
        ArrayStmtRef::Stmt(stmt) => scan.visit_stmt(stmt),
        ArrayStmtRef::Expr(expr) => scan.visit_expr(expr),
    }
    scan.found
}

#[derive(Clone, Default, PartialEq, Eq)]
struct WritePositions {
    any_time: bool,
    indices: Vec<usize>,
}

impl WritePositions {
    fn at(index: usize) -> Self {
        WritePositions {
            any_time: false,
            indices: vec![index],
        }
    }

    fn anywhere() -> Self {
        WritePositions {
            any_time: true,
            indices: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        !self.any_time && self.indices.is_empty()
    }

    fn merge(&mut self, other: &WritePositions) -> bool {
        let mut changed = false;
        if other.any_time && !self.any_time {
            self.any_time = true;
            changed = true;
        }
        for index in &other.indices {
            if !self.indices.contains(index) {
                self.indices.push(*index);
                changed = true;
            }
        }
        changed
    }

    fn before(&self, read_order: Option<usize>) -> bool {
        if self.any_time {
            return true;
        }
        match read_order {
            Some(order) => self.indices.iter().any(|index| *index <= order),
            None => !self.indices.is_empty(),
        }
    }
}

#[derive(Default)]
pub(crate) struct ModuleGlobalWrites {
    names: HashMap<String, WritePositions>,
    opaque: WritePositions,
}

impl ModuleGlobalWrites {
    pub(crate) fn writes_before(&self, name: &str, read_order: Option<usize>) -> bool {
        self.opaque.before(read_order)
            || self
                .names
                .get(name)
                .is_some_and(|positions| positions.before(read_order))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefPos {
    Read,
    Write,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ScanMode {
    #[default]
    Immediate,
    Deferred,
    Forced,
}

#[derive(Default)]
struct BodyScan {
    writes: HashSet<String>,
    deferred: HashSet<String>,
    referenced: HashSet<String>,
    deferred_referenced: HashSet<String>,
    opaque: bool,
    deferred_opaque: bool,
    mode: ScanMode,
}

impl BodyScan {
    fn reference(&mut self, name: &str) {
        if self.mode == ScanMode::Deferred {
            self.deferred_referenced.insert(name.to_owned());
        } else {
            self.referenced.insert(name.to_owned());
        }
    }

    fn write(&mut self, name: &str) {
        self.reference(name);
        if self.mode == ScanMode::Deferred {
            self.deferred.insert(name.to_owned());
        } else {
            self.writes.insert(name.to_owned());
        }
    }

    fn mark_opaque(&mut self) {
        if self.mode == ScanMode::Deferred {
            self.deferred_opaque = true;
        } else {
            self.opaque = true;
        }
    }

    fn defer<T>(&mut self, scan: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.mode;
        if saved != ScanMode::Forced {
            self.mode = ScanMode::Deferred;
        }
        let result = scan(self);
        self.mode = saved;
        result
    }

    fn force<T>(&mut self, scan: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.mode;
        self.mode = ScanMode::Forced;
        let result = scan(self);
        self.mode = saved;
        result
    }
}

struct ModuleCandidate {
    name: Option<String>,
    always_reachable: bool,
    index: usize,
    scan: BodyScan,
}

pub(crate) fn module_global_writes(program: &oxc_ast::ast::Program<'_>) -> ModuleGlobalWrites {
    let mut candidates: Vec<ModuleCandidate> = Vec::new();
    let mut referenced: HashMap<String, WritePositions> = HashMap::new();
    for (index, stmt) in program.body.iter().enumerate() {
        scan_module_statement(stmt, index, &mut candidates, &mut referenced);
    }
    for candidate in &candidates {
        let positions = WritePositions::at(candidate.index);
        for name in &candidate.scan.referenced {
            referenced
                .entry(name.clone())
                .or_default()
                .merge(&positions);
        }
    }
    let mut assigned: Vec<WritePositions> = vec![WritePositions::default(); candidates.len()];
    loop {
        let mut changed = false;
        for (index, candidate) in candidates.iter().enumerate() {
            let mut positions = if candidate.always_reachable {
                WritePositions::anywhere()
            } else {
                WritePositions::default()
            };
            if let Some(name) = candidate.name.as_deref()
                && let Some(found) = referenced.get(name)
            {
                positions.merge(found);
            }
            if positions.is_empty() {
                continue;
            }
            if assigned[index].merge(&positions) {
                changed = true;
            }
            let propagated = assigned[index].clone();
            for name in &candidate.scan.deferred_referenced {
                if referenced
                    .entry(name.clone())
                    .or_default()
                    .merge(&propagated)
                {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut result = ModuleGlobalWrites::default();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let immediate = WritePositions::at(candidate.index);
        for name in candidate.scan.writes {
            result.names.entry(name).or_default().merge(&immediate);
        }
        if candidate.scan.opaque {
            result.opaque.merge(&immediate);
        }
        let deferred = assigned[index].clone();
        if !deferred.is_empty() {
            for name in candidate.scan.deferred {
                result.names.entry(name).or_default().merge(&deferred);
            }
            if candidate.scan.deferred_opaque {
                result.opaque.merge(&deferred);
            }
        }
    }
    result
}

fn scan_module_statement(
    stmt: &Statement<'_>,
    index: usize,
    candidates: &mut Vec<ModuleCandidate>,
    referenced: &mut HashMap<String, WritePositions>,
) {
    if let Some(decl) = stmt.as_declaration() {
        scan_module_declaration(decl, false, index, candidates, referenced);
    } else if let Some(module) = stmt.as_module_declaration() {
        scan_module_declaration_block(module, index, candidates, referenced);
    } else {
        let mut scan = BodyScan::default();
        scan_stmt(stmt, &mut scan);
        candidates.push(ModuleCandidate {
            name: None,
            always_reachable: true,
            index,
            scan,
        });
    }
}

fn scan_module_declaration(
    decl: &Declaration<'_>,
    exported: bool,
    index: usize,
    candidates: &mut Vec<ModuleCandidate>,
    referenced: &mut HashMap<String, WritePositions>,
) {
    match decl {
        Declaration::FunctionDeclaration(function) => {
            candidates.push(ModuleCandidate {
                name: function.id.as_ref().map(|id| id.name.as_str().to_owned()),
                always_reachable: exported,
                index,
                scan: scan_function(function),
            });
        }
        Declaration::ClassDeclaration(class) => {
            let mut scan = BodyScan::default();
            scan_class(class, &mut scan);
            candidates.push(ModuleCandidate {
                name: class.id.as_ref().map(|id| id.name.as_str().to_owned()),
                always_reachable: exported || class.id.is_none(),
                index,
                scan,
            });
        }
        Declaration::VariableDeclaration(variable) => {
            scan_module_variable_declaration(variable, exported, index, candidates, referenced);
        }
        _ => {}
    }
}

fn scan_module_variable_declaration(
    variable: &VariableDeclaration<'_>,
    exported: bool,
    index: usize,
    candidates: &mut Vec<ModuleCandidate>,
    referenced: &mut HashMap<String, WritePositions>,
) {
    for declarator in &variable.declarations {
        let mut scan = BodyScan::default();
        scan_binding_pattern(&declarator.id, &mut scan);
        if let Some(init) = &declarator.init {
            let position = if binding_is_destructuring(&declarator.id) {
                RefPos::Read
            } else {
                RefPos::Write
            };
            scan_expr(init, position, &mut scan);
        }
        let callable = declarator
            .init
            .as_ref()
            .is_some_and(is_callable_initializer);
        if callable && let Some(name) = binding_pattern_name(&declarator.id) {
            candidates.push(ModuleCandidate {
                name: Some(name.as_str().to_owned()),
                always_reachable: exported,
                index,
                scan,
            });
            continue;
        }
        let positions = WritePositions::at(index);
        for name in scan
            .referenced
            .iter()
            .chain(scan.deferred_referenced.iter())
        {
            referenced
                .entry(name.clone())
                .or_default()
                .merge(&positions);
        }
    }
}

fn is_callable_initializer(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::ClassExpression(_) => true,
        Expression::ParenthesizedExpression(paren) => is_callable_initializer(&paren.expression),
        Expression::TSAsExpression(as_expr) => is_callable_initializer(&as_expr.expression),
        Expression::TSSatisfiesExpression(satisfies) => {
            is_callable_initializer(&satisfies.expression)
        }
        Expression::TSTypeAssertion(assertion) => is_callable_initializer(&assertion.expression),
        Expression::TSNonNullExpression(non_null) => is_callable_initializer(&non_null.expression),
        Expression::TSInstantiationExpression(instantiation) => {
            is_callable_initializer(&instantiation.expression)
        }
        _ => false,
    }
}

fn scan_module_declaration_block(
    module: &ModuleDeclaration<'_>,
    index: usize,
    candidates: &mut Vec<ModuleCandidate>,
    referenced: &mut HashMap<String, WritePositions>,
) {
    if let ModuleDeclaration::ExportNamedDeclaration(export) = module {
        if let Some(decl) = export.declaration.as_ref() {
            scan_module_declaration(decl, true, index, candidates, referenced);
        }
        let positions = WritePositions::at(index);
        for oxc_ast::ast::ExportSpecifier { local, .. } in &export.specifiers {
            if let oxc_ast::ast::ModuleExportName::IdentifierReference(id) = local {
                referenced
                    .entry(id.name.as_str().to_owned())
                    .or_default()
                    .merge(&positions);
            }
        }
    }
}

fn scan_function(function: &Function<'_>) -> BodyScan {
    let mut scan = BodyScan::default();
    scan.defer(|scan| {
        scan_formal_parameters(&function.params, scan);
        if let Some(body) = &function.body {
            scan_stmts(&body.statements, scan);
        }
    });
    scan
}

fn scan_formal_parameters(params: &FormalParameters<'_>, scan: &mut BodyScan) {
    for param in &params.items {
        scan_binding_pattern(&param.pattern, scan);
        if let Some(initializer) = &param.initializer {
            scan_expr(initializer, RefPos::Write, scan);
        }
    }
    if let Some(rest) = &params.rest {
        scan_binding_pattern(&rest.rest.argument, scan);
    }
}

fn scan_class(class: &Class<'_>, scan: &mut BodyScan) {
    if let Some(super_class) = &class.super_class {
        scan.force(|scan| scan_expr(super_class, RefPos::Write, scan));
    }
    for element in &class.body.body {
        match element {
            ClassElement::StaticBlock(block) => {
                scan.force(|scan| scan_stmts(&block.body, scan));
            }
            ClassElement::MethodDefinition(method) => {
                scan.defer(|scan| {
                    scan_formal_parameters(&method.value.params, scan);
                    if let Some(body) = &method.value.body {
                        scan_stmts(&body.statements, scan);
                    }
                });
            }
            ClassElement::PropertyDefinition(property) => {
                if let Some(value) = &property.value {
                    if property.r#static {
                        scan.force(|scan| scan_expr(value, RefPos::Write, scan));
                    } else {
                        scan.defer(|scan| scan_expr(value, RefPos::Write, scan));
                    }
                }
            }
            ClassElement::AccessorProperty(property) => {
                if let Some(value) = &property.value {
                    if property.r#static {
                        scan.force(|scan| scan_expr(value, RefPos::Write, scan));
                    } else {
                        scan.defer(|scan| scan_expr(value, RefPos::Write, scan));
                    }
                }
            }
            ClassElement::TSIndexSignature(_) => {}
        }
    }
}

fn scan_stmts(stmts: &[Statement<'_>], scan: &mut BodyScan) {
    for stmt in stmts {
        scan_stmt(stmt, scan);
    }
}

fn scan_stmt(stmt: &Statement<'_>, scan: &mut BodyScan) {
    match stmt {
        Statement::BlockStatement(block) => scan_stmts(&block.body, scan),
        Statement::IfStatement(if_stmt) => {
            scan_expr(&if_stmt.test, RefPos::Write, scan);
            scan_stmt(&if_stmt.consequent, scan);
            if let Some(alternate) = &if_stmt.alternate {
                scan_stmt(alternate, scan);
            }
        }
        Statement::WhileStatement(while_stmt) => {
            scan_expr(&while_stmt.test, RefPos::Write, scan);
            scan_stmt(&while_stmt.body, scan);
        }
        Statement::DoWhileStatement(do_while) => {
            scan_stmt(&do_while.body, scan);
            scan_expr(&do_while.test, RefPos::Write, scan);
        }
        Statement::ForStatement(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                match init {
                    ForStatementInit::VariableDeclaration(variable) => {
                        scan_variable_declaration(variable, scan);
                    }
                    e @ match_expression!(ForStatementInit) => {
                        scan_expr(e.to_expression(), RefPos::Write, scan);
                    }
                }
            }
            if let Some(test) = &for_stmt.test {
                scan_expr(test, RefPos::Write, scan);
            }
            if let Some(update) = &for_stmt.update {
                scan_expr(update, RefPos::Write, scan);
            }
            scan_stmt(&for_stmt.body, scan);
        }
        Statement::ForInStatement(for_in) => {
            scan_for_left(&for_in.left, scan);
            scan_expr(&for_in.right, RefPos::Write, scan);
            scan_stmt(&for_in.body, scan);
        }
        Statement::ForOfStatement(for_of) => {
            scan_for_left(&for_of.left, scan);
            scan_expr(&for_of.right, RefPos::Write, scan);
            scan_stmt(&for_of.body, scan);
        }
        Statement::SwitchStatement(switch) => {
            scan_expr(&switch.discriminant, RefPos::Write, scan);
            for case in &switch.cases {
                if let Some(test) = &case.test {
                    scan_expr(test, RefPos::Write, scan);
                }
                scan_stmts(&case.consequent, scan);
            }
        }
        Statement::TryStatement(try_stmt) => {
            scan_stmts(&try_stmt.block.body, scan);
            if let Some(handler) = &try_stmt.handler {
                if let Some(param) = &handler.param {
                    scan_binding_pattern(&param.pattern, scan);
                }
                scan_stmts(&handler.body.body, scan);
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                scan_stmts(&finalizer.body, scan);
            }
        }
        Statement::LabeledStatement(labeled) => scan_stmt(&labeled.body, scan),
        Statement::ExpressionStatement(expression) => {
            scan_expr(&expression.expression, RefPos::Write, scan);
        }
        Statement::ReturnStatement(return_stmt) => {
            if let Some(argument) = &return_stmt.argument {
                scan_expr(argument, RefPos::Write, scan);
            }
        }
        Statement::ThrowStatement(throw_stmt) => {
            scan_expr(&throw_stmt.argument, RefPos::Write, scan);
        }
        Statement::EmptyStatement(_)
        | Statement::DebuggerStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_) => {}
        other => {
            let Some(decl) = other.as_declaration() else {
                scan.mark_opaque();
                return;
            };
            match decl {
                Declaration::VariableDeclaration(variable) => {
                    scan_variable_declaration(variable, scan);
                }
                Declaration::FunctionDeclaration(function) => {
                    scan_formal_parameters(&function.params, scan);
                    if let Some(body) = &function.body {
                        scan_stmts(&body.statements, scan);
                    }
                }
                Declaration::ClassDeclaration(class) => scan_class(class, scan),
                _ => {}
            }
        }
    }
}

fn scan_for_left(left: &ForStatementLeft<'_>, scan: &mut BodyScan) {
    match left {
        ForStatementLeft::VariableDeclaration(variable) => {
            scan_variable_declaration(variable, scan);
        }
        target @ match_assignment_target!(ForStatementLeft) => {
            scan_assignment_target(target.to_assignment_target(), scan);
        }
    }
}

fn scan_variable_declaration(variable: &VariableDeclaration<'_>, scan: &mut BodyScan) {
    for declarator in &variable.declarations {
        scan_binding_pattern(&declarator.id, scan);
        if let Some(init) = &declarator.init {
            let position = if binding_is_destructuring(&declarator.id) {
                RefPos::Read
            } else {
                RefPos::Write
            };
            scan_expr(init, position, scan);
        }
    }
}

fn binding_is_destructuring(pattern: &BindingPattern<'_>) -> bool {
    match pattern {
        BindingPattern::BindingIdentifier(_) => false,
        BindingPattern::AssignmentPattern(assignment) => binding_is_destructuring(&assignment.left),
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_) => true,
    }
}

fn scan_binding_pattern(pattern: &BindingPattern<'_>, scan: &mut BodyScan) {
    match pattern {
        BindingPattern::BindingIdentifier(_) => {}
        BindingPattern::AssignmentPattern(assignment) => {
            scan_binding_pattern(&assignment.left, scan);
            scan_expr(&assignment.right, RefPos::Write, scan);
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                if property.computed
                    && let key @ match_expression!(PropertyKey) = &property.key
                {
                    scan_expr(key.to_expression(), RefPos::Write, scan);
                }
                scan_binding_pattern(&property.value, scan);
            }
            if let Some(rest) = &object.rest {
                scan_binding_pattern(&rest.argument, scan);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                scan_binding_pattern(element, scan);
            }
            if let Some(rest) = &array.rest {
                scan_binding_pattern(&rest.argument, scan);
            }
        }
    }
}

fn scan_argument(argument: &Argument<'_>, scan: &mut BodyScan) {
    match argument {
        Argument::SpreadElement(spread) => scan_expr(&spread.argument, RefPos::Write, scan),
        other @ match_expression!(Argument) => {
            scan_expr(other.to_expression(), RefPos::Write, scan);
        }
    }
}

fn scan_call(callee: &Expression<'_>, arguments: &[Argument<'_>], scan: &mut BodyScan) {
    if let Some(member) = member_expression_of(callee) {
        match member {
            MemberExpression::ComputedMemberExpression(computed) => {
                scan_write_root(&computed.object, scan);
                scan_expr(&computed.expression, RefPos::Write, scan);
            }
            MemberExpression::StaticMemberExpression(static_member) => {
                if static_member.property.name.as_str() == "push" {
                    scan_expr(&static_member.object, RefPos::Read, scan);
                } else {
                    scan_write_root(&static_member.object, scan);
                }
            }
            MemberExpression::PrivateFieldExpression(private) => {
                scan_write_root(&private.object, scan);
            }
        }
    } else {
        scan_expr(callee, RefPos::Write, scan);
    }
    for argument in arguments {
        scan_argument(argument, scan);
    }
}

fn member_expression_of<'a, 'b>(expr: &'b Expression<'a>) -> Option<&'b MemberExpression<'a>> {
    let mut current = expr;
    loop {
        match current {
            Expression::ParenthesizedExpression(paren) => current = &paren.expression,
            Expression::TSAsExpression(as_expr) => current = &as_expr.expression,
            Expression::TSSatisfiesExpression(satisfies) => current = &satisfies.expression,
            Expression::TSTypeAssertion(assertion) => current = &assertion.expression,
            Expression::TSNonNullExpression(non_null) => current = &non_null.expression,
            Expression::TSInstantiationExpression(instantiation) => {
                current = &instantiation.expression;
            }
            other @ match_member_expression!(Expression) => {
                return Some(other.to_member_expression());
            }
            _ => return None,
        }
    }
}

fn scan_member(member: &MemberExpression<'_>, scan: &mut BodyScan) {
    match member {
        MemberExpression::ComputedMemberExpression(computed) => {
            scan_expr(&computed.object, RefPos::Read, scan);
            scan_expr(&computed.expression, RefPos::Write, scan);
        }
        MemberExpression::StaticMemberExpression(static_member) => {
            scan_expr(&static_member.object, RefPos::Read, scan);
        }
        MemberExpression::PrivateFieldExpression(private) => {
            scan_expr(&private.object, RefPos::Read, scan);
        }
    }
}

fn scan_write_root(expr: &Expression<'_>, scan: &mut BodyScan) {
    match expr {
        Expression::Identifier(identifier) => scan.write(identifier.name.as_str()),
        Expression::ParenthesizedExpression(paren) => scan_write_root(&paren.expression, scan),
        Expression::TSAsExpression(as_expr) => scan_write_root(&as_expr.expression, scan),
        Expression::TSSatisfiesExpression(satisfies) => {
            scan_write_root(&satisfies.expression, scan);
        }
        Expression::TSTypeAssertion(assertion) => scan_write_root(&assertion.expression, scan),
        Expression::TSNonNullExpression(non_null) => scan_write_root(&non_null.expression, scan),
        Expression::TSInstantiationExpression(instantiation) => {
            scan_write_root(&instantiation.expression, scan);
        }
        other @ match_member_expression!(Expression) => match other.to_member_expression() {
            MemberExpression::ComputedMemberExpression(computed) => {
                scan_write_root(&computed.object, scan);
                scan_expr(&computed.expression, RefPos::Write, scan);
            }
            MemberExpression::StaticMemberExpression(static_member) => {
                scan_write_root(&static_member.object, scan);
            }
            MemberExpression::PrivateFieldExpression(private) => {
                scan_write_root(&private.object, scan);
            }
        },
        other => scan_expr(other, RefPos::Write, scan),
    }
}

fn scan_assignment(assignment: &AssignmentExpression<'_>, scan: &mut BodyScan) {
    if let Some(pattern) = assignment.left.as_assignment_target_pattern() {
        scan_assignment_pattern(pattern, scan);
        scan_expr(&assignment.right, RefPos::Read, scan);
    } else if let Some(target) = assignment.left.as_simple_assignment_target() {
        scan_simple_assignment_target(target, scan);
        scan_expr(&assignment.right, RefPos::Write, scan);
    }
}

fn scan_assignment_pattern(pattern: &AssignmentTargetPattern<'_>, scan: &mut BodyScan) {
    match pattern {
        AssignmentTargetPattern::ArrayAssignmentTarget(array) => {
            for element in array.elements.iter().flatten() {
                scan_assignment_target_maybe_default(element, scan);
            }
            if let Some(rest) = &array.rest {
                scan_assignment_target(&rest.target, scan);
            }
        }
        AssignmentTargetPattern::ObjectAssignmentTarget(object) => {
            for property in &object.properties {
                match property {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(property) => {
                        scan.write(property.binding.name.as_str());
                        if let Some(init) = &property.init {
                            scan_expr(init, RefPos::Write, scan);
                        }
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                        if property.computed
                            && let key @ match_expression!(PropertyKey) = &property.name
                        {
                            scan_expr(key.to_expression(), RefPos::Write, scan);
                        }
                        scan_assignment_target_maybe_default(&property.binding, scan);
                    }
                }
            }
            if let Some(rest) = &object.rest {
                scan_assignment_target(&rest.target, scan);
            }
        }
    }
}

fn scan_assignment_target_maybe_default(
    target: &AssignmentTargetMaybeDefault<'_>,
    scan: &mut BodyScan,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
            scan_assignment_target(&default.binding, scan);
            scan_expr(&default.init, RefPos::Write, scan);
        }
        other @ match_assignment_target!(AssignmentTargetMaybeDefault) => {
            scan_assignment_target(other.to_assignment_target(), scan);
        }
    }
}

fn scan_assignment_target(target: &AssignmentTarget<'_>, scan: &mut BodyScan) {
    if let Some(pattern) = target.as_assignment_target_pattern() {
        scan_assignment_pattern(pattern, scan);
    } else if let Some(simple) = target.as_simple_assignment_target() {
        scan_simple_assignment_target(simple, scan);
    }
}

fn scan_simple_assignment_target(target: &SimpleAssignmentTarget<'_>, scan: &mut BodyScan) {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
            scan.write(identifier.name.as_str());
        }
        SimpleAssignmentTarget::TSAsExpression(as_expr) => {
            scan_write_root(&as_expr.expression, scan);
        }
        SimpleAssignmentTarget::TSSatisfiesExpression(satisfies) => {
            scan_write_root(&satisfies.expression, scan);
        }
        SimpleAssignmentTarget::TSNonNullExpression(non_null) => {
            scan_write_root(&non_null.expression, scan);
        }
        SimpleAssignmentTarget::TSTypeAssertion(assertion) => {
            scan_write_root(&assertion.expression, scan);
        }
        other @ match_member_expression!(SimpleAssignmentTarget) => {
            match other.to_member_expression() {
                MemberExpression::ComputedMemberExpression(computed) => {
                    scan_write_root(&computed.object, scan);
                    scan_expr(&computed.expression, RefPos::Write, scan);
                }
                MemberExpression::StaticMemberExpression(static_member) => {
                    scan_write_root(&static_member.object, scan);
                }
                MemberExpression::PrivateFieldExpression(private) => {
                    scan_write_root(&private.object, scan);
                }
            }
        }
    }
}

fn scan_expr(expr: &Expression<'_>, position: RefPos, scan: &mut BodyScan) {
    match expr {
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::MetaProperty(_)
        | Expression::Super(_)
        | Expression::ThisExpression(_) => {}
        Expression::Identifier(identifier) => match position {
            RefPos::Read => scan.reference(identifier.name.as_str()),
            RefPos::Write => scan.write(identifier.name.as_str()),
        },
        Expression::TemplateLiteral(template) => {
            for expression in &template.expressions {
                scan_expr(expression, RefPos::Write, scan);
            }
        }
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                match element {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        scan_expr(&spread.argument, RefPos::Write, scan);
                    }
                    e @ match_expression!(ArrayExpressionElement) => {
                        scan_expr(e.to_expression(), RefPos::Write, scan);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                }
            }
        }
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                match property {
                    ObjectPropertyKind::ObjectProperty(property) => {
                        if property.computed
                            && let key @ match_expression!(PropertyKey) = &property.key
                        {
                            scan_expr(key.to_expression(), RefPos::Write, scan);
                        }
                        scan_expr(&property.value, RefPos::Write, scan);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        scan_expr(&spread.argument, RefPos::Write, scan);
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => scan_expr(&paren.expression, position, scan),
        Expression::BinaryExpression(binary) => {
            scan_expr(&binary.left, RefPos::Write, scan);
            scan_expr(&binary.right, RefPos::Write, scan);
        }
        Expression::LogicalExpression(logical) => {
            scan_expr(&logical.left, RefPos::Write, scan);
            scan_expr(&logical.right, RefPos::Write, scan);
        }
        Expression::UnaryExpression(unary) => {
            if unary.operator == UnaryOperator::Delete {
                scan_write_root(&unary.argument, scan);
            } else {
                scan_expr(&unary.argument, RefPos::Write, scan);
            }
        }
        Expression::UpdateExpression(update) => {
            scan_simple_assignment_target(&update.argument, scan);
        }
        Expression::AssignmentExpression(assignment) => scan_assignment(assignment, scan),
        Expression::CallExpression(call) => {
            scan_call(&call.callee, &call.arguments, scan);
        }
        Expression::NewExpression(new_expr) => {
            scan_expr(&new_expr.callee, RefPos::Write, scan);
            for argument in &new_expr.arguments {
                scan_argument(argument, scan);
            }
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => {
                scan_call(&call.callee, &call.arguments, scan);
            }
            ChainElement::TSNonNullExpression(non_null) => {
                scan_expr(&non_null.expression, position, scan);
            }
            other @ match_member_expression!(ChainElement) => {
                scan_member(other.to_member_expression(), scan);
            }
        },
        Expression::AwaitExpression(await_expr) => {
            scan_expr(&await_expr.argument, RefPos::Write, scan);
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(argument) = &yield_expr.argument {
                scan_expr(argument, RefPos::Write, scan);
            }
        }
        Expression::ConditionalExpression(conditional) => {
            scan_expr(&conditional.test, RefPos::Write, scan);
            scan_expr(&conditional.consequent, RefPos::Write, scan);
            scan_expr(&conditional.alternate, RefPos::Write, scan);
        }
        Expression::SequenceExpression(sequence) => {
            for expression in &sequence.expressions {
                scan_expr(expression, RefPos::Write, scan);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            scan_expr(&tagged.tag, RefPos::Write, scan);
            for expression in &tagged.quasi.expressions {
                scan_expr(expression, RefPos::Write, scan);
            }
        }
        Expression::ImportExpression(import) => {
            scan_expr(&import.source, RefPos::Write, scan);
        }
        Expression::PrivateInExpression(private_in) => {
            scan_expr(&private_in.right, RefPos::Write, scan);
        }
        Expression::TSAsExpression(as_expr) => scan_expr(&as_expr.expression, position, scan),
        Expression::TSSatisfiesExpression(satisfies) => {
            scan_expr(&satisfies.expression, position, scan);
        }
        Expression::TSTypeAssertion(assertion) => {
            scan_expr(&assertion.expression, position, scan);
        }
        Expression::TSNonNullExpression(non_null) => {
            scan_expr(&non_null.expression, position, scan);
        }
        Expression::TSInstantiationExpression(instantiation) => {
            scan_expr(&instantiation.expression, position, scan);
        }
        Expression::ArrowFunctionExpression(arrow) => {
            scan.defer(|scan| {
                scan_formal_parameters(&arrow.params, scan);
                scan_stmts(&arrow.body.statements, scan);
            });
        }
        Expression::FunctionExpression(function) => {
            scan.defer(|scan| {
                scan_formal_parameters(&function.params, scan);
                if let Some(body) = &function.body {
                    scan_stmts(&body.statements, scan);
                }
            });
        }
        Expression::ClassExpression(class) => scan_class(class, scan),
        other @ match_member_expression!(Expression) => {
            scan_member(other.to_member_expression(), scan);
        }
        _ => scan.mark_opaque(),
    }
}

fn literal_element_is_present(elements: &[HirExpr], index: usize) -> bool {
    matches!(elements.get(index), Some(element) if !matches!(element, HirExpr::Undefined(_)))
}

fn array_element_definitely_present(
    init: &HirExpr,
    index: usize,
    history: &[ArrayStmtRef<'_>],
    body_id: u32,
    destructured_temps: &HashSet<(u32, LocalId)>,
    proved: &ProvableArrayFacts<'_>,
) -> bool {
    match init {
        HirExpr::ArrayLiteral { elements, .. } => literal_element_is_present(elements, index),
        HirExpr::Local { id, .. } => {
            local_array_element_definitely_present(*id, index, history, body_id, destructured_temps)
                || (!proved.in_loop
                    && proved.facts.get(id).is_some_and(|fact| {
                        fact.body_id == body_id
                            && fact.presence.get(index).copied().unwrap_or(false)
                    }))
        }
        HirExpr::Global { name, .. } => global_array_element_definitely_present(
            name.as_str(),
            index,
            history,
            body_id,
            destructured_temps,
            proved,
        ),
        _ => false,
    }
}

fn global_array_element_definitely_present(
    name: &str,
    index: usize,
    history: &[ArrayStmtRef<'_>],
    body_id: u32,
    destructured_temps: &HashSet<(u32, LocalId)>,
    proved: &ProvableArrayFacts<'_>,
) -> bool {
    let mut written: Option<(usize, &[HirExpr])> = None;
    for (position, stmt) in history.iter().enumerate().rev() {
        let expr = match stmt {
            ArrayStmtRef::Expr(expr) => expr,
            ArrayStmtRef::Stmt(HirStmt::Expr { expr }) => expr,
            ArrayStmtRef::Stmt(_) => continue,
        };
        let HirExpr::Assignment { target, value, .. } = expr else {
            continue;
        };
        let HirExpr::Global { name: target, .. } = target.as_ref() else {
            continue;
        };
        if target.as_str() != name {
            continue;
        }
        let HirExpr::ArrayLiteral { elements, .. } = value.as_ref() else {
            return false;
        };
        written = Some((position, elements.as_slice()));
        break;
    }
    let Some((position, elements)) = written else {
        return false;
    };
    let mut scan = ArrayWriteScan {
        target: ArrayTarget::Global(name),
        index: Some(index),
        destructured_temps,
        body_id: Some(body_id),
        found: false,
    };
    for (stmt_index, stmt) in history.iter().enumerate() {
        if stmt_index == position {
            continue;
        }
        if array_writes_in_history(stmt, &mut scan) {
            return false;
        }
    }
    if proved.module_writes.writes_before(name, proved.read_order) {
        return false;
    }
    literal_element_is_present(elements, index)
}

fn local_array_element_definitely_present(
    target: LocalId,
    index: usize,
    history: &[ArrayStmtRef<'_>],
    body_id: u32,
    destructured_temps: &HashSet<(u32, LocalId)>,
) -> bool {
    let mut chain: Vec<LocalId> = vec![target];
    let mut declarations: Vec<usize> = Vec::new();
    let mut literal: Option<&[HirExpr]> = None;
    let mut limit = history.len();
    while literal.is_none() {
        let Some(current) = chain.last().copied() else {
            return false;
        };
        let located = history[..limit]
            .iter()
            .enumerate()
            .rev()
            .find_map(|(position, stmt)| match stmt {
                ArrayStmtRef::Stmt(HirStmt::Let { id, init, .. }) if *id == current => {
                    Some((position, init.as_ref()))
                }
                _ => None,
            });
        let Some((position, init)) = located else {
            return false;
        };
        declarations.push(position);
        limit = position;
        match init {
            Some(HirExpr::ArrayLiteral { elements, .. }) => literal = Some(elements),
            Some(HirExpr::Local { id, .. }) if !chain.contains(id) && chain.len() < 8 => {
                chain.push(*id);
            }
            _ => return false,
        }
    }
    let mut scan = ArrayWriteScan {
        target: ArrayTarget::Locals(&chain),
        index: Some(index),
        destructured_temps,
        body_id: Some(body_id),
        found: false,
    };
    for (position, stmt) in history.iter().enumerate() {
        if declarations.contains(&position) {
            continue;
        }
        if array_writes_in_history(stmt, &mut scan) {
            return false;
        }
    }
    literal.is_some_and(|elements| literal_element_is_present(elements, index))
}

fn known_parameter_binding(
    senv: &SkeletonEnv<'_>,
    scope: &BodyScope,
    name: &str,
) -> Option<(LocalId, TypeId)> {
    scope
        .parameter_binding(name)
        .filter(|(_, param_ty)| !matches!(senv.types.resolve(*param_ty), None | Some(Type::Error)))
}

struct NarrowedBinding {
    name: Atom,
    storage_id: LocalId,
    storage_ty: TypeId,
    inner: TypeId,
    span: Span,
}

struct UndefinedGuard {
    binding: NarrowedBinding,
    undefined_is_then: bool,
}

fn strip_parentheses<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    let mut current = expr;
    while let Expression::ParenthesizedExpression(inner) = current {
        current = &inner.expression;
    }
    current
}

fn sequence_is_all_destructuring(sequence: &oxc_ast::ast::SequenceExpression<'_>) -> bool {
    sequence
        .expressions
        .iter()
        .all(|item| match strip_parentheses(item) {
            Expression::AssignmentExpression(assign) => {
                assign.left.as_assignment_target_pattern().is_some()
            }
            Expression::SequenceExpression(inner) => sequence_is_all_destructuring(inner),
            _ => false,
        })
}

pub(crate) fn is_destructuring_assignment_expression(expr: &Expression<'_>) -> bool {
    match strip_parentheses(expr) {
        Expression::SequenceExpression(sequence) => sequence_is_all_destructuring(sequence),
        Expression::AssignmentExpression(assign) => {
            assign.left.as_assignment_target_pattern().is_some()
        }
        _ => false,
    }
}

fn identifier_reference_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Identifier(id) if id.name.as_str() != "undefined" => Some(id.name.as_str()),
        _ => None,
    }
}

fn is_undefined_reference(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Identifier(id) if id.name.as_str() == "undefined")
}

fn undefined_guard_test<'a>(test: &'a Expression<'a>) -> Option<(&'a str, bool)> {
    let Expression::BinaryExpression(binary) = strip_parentheses(test) else {
        return None;
    };
    let undefined_is_then = match binary.operator {
        BinaryOperator::StrictEquality => true,
        BinaryOperator::StrictInequality => false,
        _ => return None,
    };
    if is_undefined_reference(&binary.right) {
        return identifier_reference_name(&binary.left).map(|name| (name, undefined_is_then));
    }
    if is_undefined_reference(&binary.left) {
        return identifier_reference_name(&binary.right).map(|name| (name, undefined_is_then));
    }
    None
}

fn undefined_only_inner(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    let Some(Type::Optional { inner, nullish }) = types.resolve(ty) else {
        return None;
    };
    if !nullish.undefined || nullish.null {
        return None;
    }
    match types.resolve(*inner) {
        None | Some(Type::Error | Type::Null | Type::Void) => None,
        Some(_) => Some(*inner),
    }
}

fn drop_parameter_var_shadows(
    senv: &SkeletonEnv<'_>,
    stmts: &[Statement<'_>],
    scope: &mut BodyScope,
) {
    let mut names: Vec<Atom> = Vec::new();
    for stmt in stmts {
        let Some(Declaration::VariableDeclaration(decl)) = stmt.as_declaration() else {
            continue;
        };
        if !matches!(decl.kind, VariableDeclarationKind::Var) {
            continue;
        }
        for declarator in &decl.declarations {
            collect_binding_names(&declarator.id, &mut names);
        }
    }
    for name in names {
        if known_parameter_binding(senv, scope, name.as_str()).is_some() {
            scope.remove_predeclared_shadow(name.as_str());
        }
    }
}

impl SkeletonBuilder {
    fn enclosing_this_local(&self) -> Option<LocalId> {
        self.this_local_stack.iter().rev().flatten().copied().next()
    }

    fn closure_captures_this(&self, value: &HirExpr) -> bool {
        let Some(this_id) = self.enclosing_this_local() else {
            return false;
        };
        let HirExpr::Closure { captures, .. } = value else {
            return false;
        };
        captures
            .iter()
            .any(|capture| matches!(capture, HirExpr::Local { id, .. } if *id == this_id))
    }

    fn tracked_this_capture(&self, value: &HirExpr) -> bool {
        let tracked_somewhere = |id: &LocalId| {
            self.this_capture_locals
                .iter()
                .rev()
                .any(|locals| locals.contains(id))
        };
        match value {
            HirExpr::Closure { .. } => self.closure_captures_this(value),
            HirExpr::Local { id, .. } => self
                .this_capture_locals
                .last()
                .is_some_and(|locals| locals.contains(id)),
            HirExpr::Global { name, .. } => self
                .pending_closure_captures
                .last()
                .and_then(|frame| frame.enclosing_bindings.get(name.as_str()))
                .is_some_and(|(id, _)| tracked_somewhere(id)),
            _ => false,
        }
    }

    fn refuse_escaping_this_capture(
        &mut self,
        senv: &mut SkeletonEnv,
        value: Option<&HirExpr>,
        span: oxc_span::Span,
    ) {
        let Some(value) = value else {
            return;
        };
        if self.tracked_this_capture(value) {
            senv.report_unsupported(
                ESCAPING_THIS_CAPTURE_CODE,
                ESCAPING_THIS_CAPTURE_MESSAGE,
                span,
            );
        }
    }

    fn refuse_stored_this_capture(
        &mut self,
        senv: &mut SkeletonEnv,
        expr: &HirExpr,
        span: oxc_span::Span,
    ) {
        let Some(this_id) = self.enclosing_this_local() else {
            return;
        };
        let HirExpr::Assignment { target, value, .. } = expr else {
            return;
        };
        let stores_on_this = match target.as_ref() {
            HirExpr::Field { owner, .. } | HirExpr::Index { owner, .. } => {
                matches!(owner.as_ref(), HirExpr::Local { id, .. } if *id == this_id)
                    || matches!(
                        owner.as_ref(),
                        HirExpr::Global { name, .. } if name.as_str() == "this"
                    )
            }
            _ => false,
        };
        if stores_on_this && self.tracked_this_capture(value) {
            senv.report_unsupported(
                ESCAPING_THIS_CAPTURE_CODE,
                ESCAPING_THIS_CAPTURE_MESSAGE,
                span,
            );
        }
    }

    pub(crate) fn walk_stmts(
        &mut self,
        senv: &mut SkeletonEnv,
        stmts: &[Statement<'_>],
        scope: &mut BodyScope,
    ) -> Vec<HirStmt> {
        let mut out = Vec::new();
        drop_parameter_var_shadows(senv, stmts, scope);
        self.collect_array_literal_hints(senv, stmts, scope);
        self.hoisted_fn_decls.push(Vec::new());
        for s in stmts {
            self.walk_stmt(senv, s, &mut out, scope);
        }
        let mut hoisted = self.hoisted_fn_decls.pop().unwrap_or_default();
        if !hoisted.is_empty() {
            hoisted.append(&mut out);
            out = hoisted;
        }
        out
    }

    fn collect_array_literal_hints(
        &mut self,
        senv: &mut SkeletonEnv,
        stmts: &[Statement<'_>],
        scope: &BodyScope,
    ) {
        for (index, stmt) in stmts.iter().enumerate() {
            let Statement::VariableDeclaration(decl) = stmt else {
                continue;
            };
            for declarator in &decl.declarations {
                if declarator.type_annotation.is_some() {
                    continue;
                }
                let Some(name) = binding_pattern_name(&declarator.id) else {
                    continue;
                };
                let Some(Expression::ArrayExpression(array)) = declarator.init.as_ref() else {
                    continue;
                };
                if !array.elements.is_empty() {
                    continue;
                }
                let mut values: Vec<&Expression<'_>> = Vec::new();
                collect_array_uses_stmts(
                    &stmts[index + 1..],
                    name.as_str(),
                    false,
                    &HashSet::new(),
                    &mut values,
                );
                let mut element: Option<TypeId> = None;
                let mut conflicting = false;
                for value in values {
                    let Some(ty) = array_element_value_ty(senv, self, scope, value) else {
                        continue;
                    };
                    match element {
                        None => element = Some(ty),
                        Some(previous) if previous == ty => {}
                        Some(_) => conflicting = true,
                    }
                }
                if !conflicting && let Some(element) = element {
                    self.array_literal_hints.insert(array.span.start, element);
                }
            }
        }
    }

    fn walk_child(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &Statement<'_>,
        scope: &mut BodyScope,
    ) -> Box<HirStmt> {
        self.walk_child_maybe_conditional(senv, s, scope, true)
    }

    fn walk_child_maybe_conditional(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &Statement<'_>,
        scope: &mut BodyScope,
        conditional: bool,
    ) -> Box<HirStmt> {
        let mut inner = Vec::new();
        let braceless = !matches!(s, Statement::BlockStatement(_));
        if braceless {
            self.suppress_narrowing += 1;
        }
        if conditional {
            self.enter_class_value_branch();
        }
        self.rust_scope_depth = self.rust_scope_depth.saturating_add(1);
        self.walk_stmt(senv, s, &mut inner, scope);
        self.rust_scope_depth = self.rust_scope_depth.saturating_sub(1);
        if conditional {
            self.leave_class_value_branch();
        }
        if braceless {
            self.suppress_narrowing -= 1;
        }
        if inner.len() == 1 {
            Box::new(inner.into_iter().next().expect("len checked to be 1"))
        } else {
            Box::new(HirStmt::Block(inner))
        }
    }

    pub(super) fn redirect_narrowed_target(
        &mut self,
        target: HirExpr,
        scope: &mut BodyScope,
    ) -> HirExpr {
        let HirExpr::Local { id, .. } = &target else {
            return target;
        };
        let narrowed_id = *id;
        let Some((storage_id, storage_ty)) = self.narrowed_storage(narrowed_id) else {
            return target;
        };
        let mut target = target;
        if let HirExpr::Local { id, ty, .. } = &mut target {
            *id = storage_id;
            *ty = storage_ty;
        }
        scope.widen_id(narrowed_id);
        target
    }

    fn widen_before_loop(&mut self, scope: &mut BodyScope, effects: &LoopEffects) {
        if effects.has_effect {
            self.constant_int_locals.clear();
        } else {
            for name in &effects.writes {
                if let Some(id) = scope.visible_binding(name) {
                    self.constant_int_locals
                        .remove(&self.class_value_storage(id));
                }
            }
        }
        if effects.has_effect {
            for name in scope.names() {
                Self::widen_binding(scope, &name);
            }
            return;
        }
        for name in &effects.writes {
            Self::widen_binding(scope, name);
        }
    }

    fn widen_binding(scope: &mut BodyScope, name: &str) {
        let Some(id) = scope.visible_binding(name) else {
            return;
        };
        scope.widen_id(id);
    }

    fn while_effects(test: &Expression<'_>, body: &Statement<'_>) -> LoopEffects {
        let mut effects = LoopEffects::default();
        effects.scan_expr(test);
        effects.scan_stmt(body);
        effects
    }

    fn for_effects(f: &ForStatement<'_>) -> LoopEffects {
        let mut effects = LoopEffects::default();
        if let Some(init) = &f.init {
            effects.scan_for_init(init);
        }
        if let Some(test) = &f.test {
            effects.scan_expr(test);
        }
        if let Some(update) = &f.update {
            effects.scan_expr(update);
        }
        effects.scan_stmt(&f.body);
        effects
    }

    fn for_binding_effects(
        left: &ForStatementLeft<'_>,
        right: &Expression<'_>,
        body: &Statement<'_>,
    ) -> LoopEffects {
        let mut effects = LoopEffects::default();
        effects.scan_for_left(left);
        effects.scan_expr(right);
        effects.scan_stmt(body);
        effects
    }

    fn walk_stmt(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &Statement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let start = out.len();
        self.walk_stmt_inner(senv, s, out, scope);
        let body_id = scope.body_id();
        self.invalidate_provable_arrays(senv, body_id, &out[start..]);
    }

    fn invalidate_provable_arrays(
        &mut self,
        senv: &SkeletonEnv<'_>,
        body_id: u32,
        stmts: &[HirStmt],
    ) {
        if self.provable_array_facts.is_empty() {
            return;
        }
        let ids: Vec<LocalId> = self.provable_array_facts.keys().copied().collect();
        let mut dead: Vec<LocalId> = Vec::new();
        for id in ids {
            let mut scan = ArrayWriteScan {
                target: ArrayTarget::Locals(std::slice::from_ref(&id)),
                index: None,
                destructured_temps: &senv.destructured_temp_ids,
                body_id: Some(body_id),
                found: false,
            };
            for stmt in stmts {
                if array_writes_in_stmt(stmt, &mut scan) {
                    dead.push(id);
                    break;
                }
            }
        }
        for id in dead {
            self.provable_array_facts.remove(&id);
        }
    }

    fn invalidate_provable_arrays_in_expr(
        &mut self,
        senv: &SkeletonEnv<'_>,
        body_id: u32,
        expr: &HirExpr,
    ) {
        if self.provable_array_facts.is_empty() {
            return;
        }
        let ids: Vec<LocalId> = self.provable_array_facts.keys().copied().collect();
        let mut dead: Vec<LocalId> = Vec::new();
        for id in ids {
            let mut scan = ArrayWriteScan {
                target: ArrayTarget::Locals(std::slice::from_ref(&id)),
                index: None,
                destructured_temps: &senv.destructured_temp_ids,
                body_id: Some(body_id),
                found: false,
            };
            scan.visit_expr(expr);
            if scan.found {
                dead.push(id);
            }
        }
        for id in dead {
            self.provable_array_facts.remove(&id);
        }
    }

    fn walk_stmt_inner(
        &mut self,
        senv: &mut SkeletonEnv,
        s: &Statement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        match s {
            Statement::BlockStatement(b) => {
                scope.push();
                let inner = self.walk_block_with_predeclare(senv, &b.body, scope);
                scope.pop();
                out.push(HirStmt::Block(inner));
            }
            d @ match_declaration!(Statement) => {
                let decl = d.to_declaration();
                match decl {
                    Declaration::VariableDeclaration(v) => {
                        self.walk_var_decl(senv, v, out, scope);
                    }
                    Declaration::ClassDeclaration(c) => {
                        self.walk_local_class_decl(senv, c, out, scope);
                    }
                    Declaration::FunctionDeclaration(f) => {
                        self.walk_local_function_decl(senv, f, out, scope);
                    }
                    other => {
                        senv.report_unwalked(
                            "statement form is not supported by the body walker",
                            other.span(),
                        );
                    }
                }
            }
            Statement::ExpressionStatement(e) => {
                if is_destructuring_assignment_expression(&e.expression) {
                    let history: Vec<ArrayStmtRef<'_>> =
                        out.iter().map(ArrayStmtRef::Stmt).collect();
                    let mut lowered: Vec<HirStmt> = Vec::new();
                    if self.lower_destructuring_assignment(
                        senv,
                        &e.expression,
                        &history,
                        &mut lowered,
                        scope,
                        None,
                    ) {
                        out.extend(lowered);
                        return;
                    }
                }
                let previous_statement_expr = self.own_statement_expr;
                self.own_statement_expr = true;
                let mut expr = self.walk_expr(senv, &e.expression, scope);
                self.own_statement_expr = previous_statement_expr;
                self.refuse_stored_this_capture(senv, &expr, e.span);
                let narrowing = self.narrow_after_assignment(senv, &e.expression, &mut expr, scope);
                let copy_back = self.finish_statement_expression(&expr, senv);
                out.push(HirStmt::Expr { expr });
                if let Some(stmt) = narrowing {
                    out.push(stmt);
                }
                if let Some(stmt) = copy_back {
                    out.push(stmt);
                }
            }
            Statement::ReturnStatement(r) => {
                let value = r.argument.as_ref().map(|e| self.walk_expr(senv, e, scope));
                self.refuse_escaping_this_capture(senv, value.as_ref(), r.span);
                self.note_may_exit(false);
                out.push(HirStmt::Return { value });
            }
            Statement::IfStatement(i) => {
                let cond = self.walk_expr(senv, &i.test, scope);
                self.invalidate_provable_arrays_in_expr(senv, scope.body_id(), &cond);
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                let constant = match &cond {
                    HirExpr::Bool(true, _) => Some(true),
                    HirExpr::Bool(false, _) => Some(false),
                    // An immediately preceding boolean initializer cannot have been
                    // changed before this direct local read. Preserve dead-branch facts.
                    HirExpr::Local { id, .. } => match out.last() {
                        Some(HirStmt::Let {
                            id: declared,
                            init: Some(HirExpr::Bool(value, _)),
                            ..
                        }) if declared == id => Some(*value),
                        _ => None,
                    },
                    _ => None,
                };
                let guard = if constant.is_none() && self.suppress_narrowing == 0 {
                    Self::undefined_guard(senv, &i.test, scope)
                } else {
                    None
                };
                let joins = constant.is_none() && i.alternate.is_some();
                let saved_fields = if joins {
                    Some(self.snapshot_class_value_field_state())
                } else {
                    None
                };
                let mut branch_fields = Vec::new();
                let then_start = self.class_value_poison_writes.len();
                let then = if constant == Some(false) {
                    self.walk_child(senv, &i.consequent, scope)
                } else if let Some(guard) = guard
                    .as_ref()
                    .filter(|guard| !guard.undefined_is_then)
                    .map(|guard| &guard.binding)
                {
                    self.walk_branch_narrowed(senv, &i.consequent, scope, guard, constant.is_none())
                } else {
                    self.walk_child_maybe_conditional(
                        senv,
                        &i.consequent,
                        scope,
                        constant.is_none(),
                    )
                };
                if saved_fields.is_some() {
                    branch_fields.push(self.snapshot_class_value_field_state());
                }
                let then_constants = self.snapshot_constant_int_locals();
                if constant != Some(true) {
                    self.restore_constant_int_locals(&saved_constants);
                }
                let else_start = self.class_value_poison_writes.len();
                if let Some(saved) = &saved_fields {
                    self.restore_class_value_field_state(saved);
                }
                let otherwise = i.alternate.as_ref().map(|a| {
                    if constant == Some(true) {
                        self.walk_child(senv, a, scope)
                    } else if let Some(guard) = guard
                        .as_ref()
                        .filter(|guard| guard.undefined_is_then)
                        .map(|guard| &guard.binding)
                    {
                        self.walk_branch_narrowed(senv, a, scope, guard, constant.is_none())
                    } else {
                        self.walk_child_maybe_conditional(senv, a, scope, constant.is_none())
                    }
                });
                if saved_fields.is_some() {
                    branch_fields.push(self.snapshot_class_value_field_state());
                }
                if constant == Some(true) {
                    self.restore_constant_int_locals(&then_constants);
                } else if constant.is_none() {
                    self.intersect_constant_int_locals(&then_constants);
                }
                self.join_class_value_field_states(&branch_fields);
                if joins {
                    let else_end = self.class_value_poison_writes.len();
                    let branch_depth = self.conditional_depth + 1;
                    self.join_class_value_field_poison(
                        &[(then_start, else_start), (else_start, else_end)],
                        &branch_fields,
                        saved_fields.as_ref().expect("joins implies a snapshot"),
                        branch_depth,
                    );
                }
                self.merge_unknown_class_value_locals(saved_unknown);
                out.push(HirStmt::If {
                    cond,
                    then,
                    otherwise,
                });
                if let Some(guard) = &guard {
                    let undefined_branch_terminates = if guard.undefined_is_then {
                        statement_always_terminates(&i.consequent)
                    } else {
                        i.alternate
                            .as_ref()
                            .is_some_and(|alternate| statement_always_terminates(alternate))
                    };
                    let surviving = if guard.undefined_is_then {
                        i.alternate.as_ref()
                    } else {
                        Some(&i.consequent)
                    };
                    let surviving_writes = surviving.is_some_and(|stmt| {
                        let mut scan = BodyScan::default();
                        scan_stmt(stmt, &mut scan);
                        scan.writes.contains(guard.binding.name.as_str())
                    });
                    if undefined_branch_terminates && !surviving_writes {
                        let narrowed = self.narrowed_binding_stmt(senv, scope, &guard.binding);
                        out.push(narrowed);
                    }
                }
            }
            Statement::WhileStatement(w) => {
                self.widen_before_loop(scope, &Self::while_effects(&w.test, &w.body));
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let cond = self.walk_expr(senv, &w.test, scope);
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                let body = self.walk_child(senv, &w.body, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
                out.push(HirStmt::While { cond, body });
            }
            Statement::DoWhileStatement(d) => {
                self.widen_before_loop(scope, &Self::while_effects(&d.test, &d.body));
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                let body = self.walk_child_maybe_conditional(senv, &d.body, scope, false);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                let cond = self.walk_expr(senv, &d.test, scope);
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
                out.push(HirStmt::DoWhile { body, cond });
            }
            Statement::ForStatement(f) => {
                self.widen_before_loop(scope, &Self::for_effects(f));
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                self.walk_c_for(senv, f, out, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
            }
            Statement::ForOfStatement(f) => {
                self.widen_before_loop(
                    scope,
                    &Self::for_binding_effects(&f.left, &f.right, &f.body),
                );
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                self.walk_for_of(senv, f, out, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
            }
            Statement::ForInStatement(f) => {
                self.widen_before_loop(
                    scope,
                    &Self::for_binding_effects(&f.left, &f.right, &f.body),
                );
                self.loop_frames.push(Vec::new());
                let saved_unknown = self.unknown_class_value_locals.clone();
                let saved_constants = self.snapshot_constant_int_locals();
                self.class_value_breakable_depth += 1;
                self.enter_class_value_loop_body();
                self.walk_for_in(senv, f, out, scope);
                self.leave_class_value_loop_body();
                self.class_value_breakable_depth -= 1;
                self.intersect_constant_int_locals(&saved_constants);
                self.merge_unknown_class_value_locals(saved_unknown);
                self.verify_loop_class_value_reads(senv);
            }
            Statement::SwitchStatement(sw) => {
                let saved_unknown = self.unknown_class_value_locals.clone();
                self.walk_switch(senv, sw, out, scope);
                self.merge_unknown_class_value_locals(saved_unknown);
            }
            Statement::TryStatement(t) => {
                let saved_unknown = self.unknown_class_value_locals.clone();
                self.walk_try(senv, t, out, scope);
                self.merge_unknown_class_value_locals(saved_unknown);
            }
            Statement::ThrowStatement(t) => {
                let expr = self.walk_expr(senv, &t.argument, scope);
                self.note_may_throw();
                out.push(HirStmt::Throw { expr });
            }
            Statement::BreakStatement(b) => {
                if b.label.is_some() {
                    senv.report_unwalked(
                        "labeled `break` is not supported by the body walker",
                        b.span,
                    );
                }
                self.note_may_exit(true);
                out.push(HirStmt::Break {
                    label: label_atom(b.label.as_ref().map(|l| l.name.as_str())),
                });
            }
            Statement::ContinueStatement(c) => {
                if c.label.is_some() {
                    senv.report_unwalked(
                        "labeled `continue` is not supported by the body walker",
                        c.span,
                    );
                }
                self.note_may_exit(true);
                out.push(HirStmt::Continue {
                    label: label_atom(c.label.as_ref().map(|l| l.name.as_str())),
                });
            }
            Statement::LabeledStatement(l) => {
                senv.report_unwalked(
                    "labeled statements are not supported by the body walker",
                    l.span,
                );
                self.walk_stmt(senv, &l.body, out, scope);
            }
            Statement::EmptyStatement(_) | Statement::DebuggerStatement(_) => {}
            other => {
                senv.report_unwalked(
                    "statement form is not supported by the body walker",
                    other.span(),
                );
            }
        }
    }

    fn walk_local_function_decl(
        &mut self,
        senv: &mut SkeletonEnv,
        func: &oxc_ast::ast::Function<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let Some(id) = func.id.as_ref() else {
            senv.report_unsupported(
                "E0500",
                "a function declaration inside a function body must be named",
                func.span,
            );
            return;
        };
        if func.generator {
            if func.type_parameters.is_some() {
                senv.report_unsupported(
                    "E0500",
                    "a generic generator declaration inside a function body is not supported by \
                     the body walker",
                    func.span,
                );
                return;
            }
            self.lift_local_generator_decl(senv, func, id.name.as_str(), scope);
            return;
        }
        let name = Atom::from(id.name.as_str());
        let referenced_earlier = scope.predeclared_is_allocated(name.as_str());
        let closure = self.walk_function_expression(senv, func, scope, true);
        let ty = closure.ty();
        let local = scope.declare(name.as_str(), ty);
        if let Some(bodies) = crate::skeleton::closure_bodies_of(&closure) {
            for body in bodies {
                scope.set_closure_body(local, body);
            }
        }
        let stmt = HirStmt::Let {
            id: local,
            name: name.clone(),
            ty,
            init: Some(closure),
            narrowed: false,
        };
        let forwardable = matches!(&stmt, HirStmt::Let { init: Some(closure), .. }
            if matches!(closure, HirExpr::Closure { captures, .. } if captures.is_empty()));
        if !forwardable && referenced_earlier {
            senv.report_unsupported(
                "E0419",
                format!(
                    "`{}` is referenced before its declaration, but the local function captures \
                     a variable of the enclosing function and cannot be hoisted above the \
                     reference; move the declaration above the reference",
                    name.as_str()
                )
                .as_str(),
                func.span,
            );
        }
        if forwardable && let Some(frame) = self.hoisted_fn_decls.last_mut() {
            frame.push(stmt);
            self.note_switch_case_hoisted_decl(local, &name, scope.depth());
            return;
        }
        out.push(stmt);
    }

    fn lift_local_generator_decl(
        &mut self,
        senv: &mut SkeletonEnv,
        func: &oxc_ast::ast::Function<'_>,
        name: &str,
        scope: &mut BodyScope,
    ) {
        if scope.depth() > 1 {
            senv.report_unsupported(
                "E0500",
                "a generator function declaration inside a nested block is not supported by the \
                 body walker; declare it directly in the function body",
                func.span,
            );
            return;
        }
        if self.generator_alias_declared_here(name) {
            senv.report_unsupported(
                "E0500",
                format!(
                    "two generator declarations named `{name}` in one function body are not \
                     supported by the body walker"
                )
                .as_str(),
                func.span,
            );
            return;
        }
        if self.generator_alias_shadows_outer(name) {
            senv.report_unsupported(
                "E0500",
                format!(
                    "a generator declaration named `{name}` that shadows a generator of an \
                     enclosing function is not supported by the body walker; rename one of them"
                )
                .as_str(),
                func.span,
            );
            return;
        }
        if self.generator_alias_stack.is_empty() {
            senv.report_unsupported(
                "E0500",
                "a generator function declaration outside a function body is not supported by \
                 the body walker",
                func.span,
            );
            return;
        }
        let previous_callee_position = self.in_callee_position;
        self.in_callee_position = false;
        let lifted = self.walk_generator_function_expression(senv, func, scope);
        self.in_callee_position = previous_callee_position;
        let HirExpr::Global {
            name: lifted, ty, ..
        } = lifted
        else {
            return;
        };
        let call_ty = match senv.types.resolve(ty) {
            Some(Type::Fn { ret, .. }) => *ret,
            _ => ty,
        };
        if let Some(frame) = self.generator_alias_stack.last_mut() {
            frame.insert(
                name.to_owned(),
                LocalGeneratorAlias {
                    lifted,
                    call_ty,
                    fn_ty: ty,
                },
            );
        }
    }

    fn walk_for_of(
        &mut self,
        senv: &mut SkeletonEnv,
        f: &ForOfStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let iter = self.walk_expr(senv, &f.right, scope);
        let runs_user_code = f.r#await
            || !matches!(
                senv.types.resolve(iter.ty()),
                Some(Type::Array { .. } | Type::String)
            );
        if runs_user_code {
            self.invalidate_captured_narrowings(scope);
        }
        scope.push();
        let binding = self.for_binding(senv, &f.left, scope);
        let body = self.walk_child(senv, &f.body, scope);
        scope.pop();
        if f.r#await {
            out.push(HirStmt::ForAwaitOf {
                binding,
                iter,
                body,
            });
        } else {
            out.push(HirStmt::ForOf {
                binding,
                iter,
                body,
            });
        }
    }

    fn walk_for_in(
        &mut self,
        senv: &mut SkeletonEnv,
        f: &ForInStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let iter = self.walk_expr(senv, &f.right, scope);
        scope.push();
        let binding = self.for_binding(senv, &f.left, scope);
        let body = self.walk_child(senv, &f.body, scope);
        scope.pop();
        out.push(HirStmt::ForIn {
            binding,
            iter,
            body,
        });
    }

    fn walk_switch(
        &mut self,
        senv: &mut SkeletonEnv,
        sw: &SwitchStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        let disc = self.walk_expr(senv, &sw.discriminant, scope);
        let disc_is_class = self.is_class_value_expr(&disc, scope.body_id());
        let disc_constant = match &disc {
            HirExpr::Local { id, .. } => self
                .constant_int_locals
                .get(&self.class_value_storage(*id))
                .copied(),
            _ => None,
        };
        scope.push();
        for c in &sw.cases {
            crate::body::predeclare_forward_declarations(&c.consequent, scope, senv.error_ty());
        }
        let outer: HashMap<Atom, LocalId> = scope
            .names()
            .into_iter()
            .filter_map(|name| {
                scope
                    .visible_binding(name.as_str())
                    .map(|id| (Atom::from(name.as_str()), id))
            })
            .collect();
        self.push_switch_case_frame(scope.depth(), outer);
        let unconditional_default = sw.cases.len() == 1 && sw.cases[0].test.is_none();
        let has_default = sw.cases.iter().any(|c| c.test.is_none());
        let ends_every_case = sw.cases.iter().all(|c| case_covers_its_end(&c.consequent));
        let joins = !unconditional_default && has_default && ends_every_case;
        let saved_fields = if joins {
            Some(self.snapshot_class_value_field_state())
        } else {
            None
        };
        self.class_value_breakable_depth += 1;
        let mut branch_fields = Vec::new();
        let mut case_entries: Vec<crate::skeleton::ClassValueFieldState> = Vec::new();
        if let Some(saved) = &saved_fields {
            case_entries.push(saved.clone());
        }
        let mut cases = Vec::with_capacity(sw.cases.len());
        let mut case_writes = Vec::with_capacity(sw.cases.len());
        let mut group_start: Option<usize> = None;
        for c in &sw.cases {
            self.start_switch_case();
            if !case_entries.is_empty() {
                self.join_class_value_field_states(&case_entries);
            }
            let case_start = self.class_value_poison_writes.len();
            group_start.get_or_insert(case_start);
            if !unconditional_default {
                self.enter_class_value_branch();
            }
            let test = c.test.as_ref().map(|e| self.walk_expr(senv, e, scope));
            let checkpoint = scope.narrowing_checkpoint();
            let reachable = reachable_case_prefix(&c.consequent);
            self.rust_scope_depth = self.rust_scope_depth.saturating_add(1);
            let body = self.walk_stmts(senv, &c.consequent[..reachable], scope);
            self.rust_scope_depth = self.rust_scope_depth.saturating_sub(1);
            scope.discard_narrowings_since(checkpoint);
            if !unconditional_default {
                self.leave_class_value_branch();
            }
            let case_end = self.class_value_poison_writes.len();
            cases.push(HirSwitchCase { test, body });
            if !c.consequent.is_empty() {
                if saved_fields.is_some() {
                    case_entries.push(self.snapshot_class_value_field_state());
                }
                if ends_with_a_terminator(&c.consequent) {
                    let start = group_start.take().unwrap_or(case_start);
                    case_writes.push((start, case_end));
                    if saved_fields.is_some() {
                        branch_fields.push(self.snapshot_class_value_field_state());
                    }
                }
            }
        }
        let case_frame = self.pop_switch_case_frame();
        if let Some(frame) = case_frame {
            check_switch_case_tdz(senv, &disc, disc_constant, &cases, &frame.events, sw.span);
        }
        if let Some(start) = group_start {
            case_writes.push((start, self.class_value_poison_writes.len()));
            if saved_fields.is_some() {
                let state = self.snapshot_class_value_field_state();
                case_entries.push(state.clone());
                branch_fields.push(state);
            }
        }
        self.join_class_value_field_states(&branch_fields);
        if joins {
            let branch_depth = self.conditional_depth + 1;
            self.join_class_value_field_poison(
                &case_writes,
                &branch_fields,
                saved_fields.as_ref().expect("joins implies a snapshot"),
                branch_depth,
            );
        }
        self.class_value_breakable_depth -= 1;
        for index in (0..cases.len().saturating_sub(1)).rev() {
            if !ends_with_a_terminator(&sw.cases[index].consequent) {
                let next = cases[index + 1].body.clone();
                cases[index].body.extend(next);
            }
        }
        if disc_is_class {
            for case in &cases {
                if case
                    .test
                    .as_ref()
                    .is_some_and(|test| self.is_class_value_expr(test, scope.body_id()))
                {
                    senv.report_unsupported(
                        "E0419",
                        "a class value used as a `switch` case is not supported; compare it with \
                         `===` instead",
                        sw.span,
                    );
                }
            }
            if cases
                .iter()
                .filter_map(|case| case.test.as_ref())
                .any(|test| {
                    !matches!(
                        test,
                        HirExpr::Int(..)
                            | HirExpr::Float(..)
                            | HirExpr::Bool(..)
                            | HirExpr::Null(..)
                            | HirExpr::Undefined(..)
                            | HirExpr::String(..)
                    )
                })
            {
                senv.report_unsupported(
                    "E0419",
                    "effectful switch tests on a class value are not supported",
                    sw.span,
                );
            }
            cases.retain(|case| {
                case.test
                    .as_ref()
                    .is_none_or(|test| self.is_class_value_expr(test, scope.body_id()))
            });
        }
        scope.pop();
        out.push(HirStmt::Switch { disc, cases });
    }

    fn walk_try(
        &mut self,
        senv: &mut SkeletonEnv,
        t: &TryStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        scope.push();
        let saved_skips = self.try_body_skips.replace(false);
        let saved_fields = t
            .handler
            .as_ref()
            .map(|_| self.snapshot_class_value_field_state());
        let entry_fields = t
            .finalizer
            .as_ref()
            .map(|_| self.snapshot_class_value_field_state());
        let writes_before = self.class_value_poison_writes.len();
        let exits_before = self.class_value_exits.len();
        self.rust_scope_depth = self.rust_scope_depth.saturating_add(1);
        let body = HirStmt::Block(self.walk_block_with_predeclare(senv, &t.block.body, scope));
        self.rust_scope_depth = self.rust_scope_depth.saturating_sub(1);
        let body_may_skip = self.try_body_skips == Some(true);
        let body_fields = saved_fields
            .as_ref()
            .map(|_| self.snapshot_class_value_field_state());
        scope.pop();

        self.try_body_skips = Some(false);
        if let Some(saved) = &saved_fields {
            self.restore_class_value_field_state(saved);
        }
        let catch = t.handler.as_ref().map(|handler| {
            scope.push();
            self.enter_class_value_branch();
            let binding = handler.param.as_ref().and_then(|p| {
                let Some(name) = binding_pattern_name(&p.pattern) else {
                    senv.report_unwalked(
                        "destructuring catch bindings are not supported by the body walker",
                        p.span,
                    );
                    return None;
                };
                let ty = if p.type_annotation.is_some() {
                    self.resolve_ts_type_from_annotation_in_scope(
                        senv,
                        p.type_annotation.as_deref(),
                        Some(scope),
                    )
                } else {
                    senv.error_ty()
                };
                let id = scope.declare(name.as_str(), ty);
                self.mutable_locals.insert((scope.body_id(), id));
                Some(HirCatchBinding::new(id, Atom::from(name.as_str()), ty))
            });
            let cbody = {
                self.rust_scope_depth = self.rust_scope_depth.saturating_add(1);
                let cbody = HirStmt::Block(self.walk_block_with_predeclare(
                    senv,
                    &handler.body.body,
                    scope,
                ));
                self.rust_scope_depth = self.rust_scope_depth.saturating_sub(1);
                cbody
            };
            self.leave_class_value_branch();
            scope.pop();
            HirCatchClause::new(binding, Box::new(cbody))
        });
        let catch_may_skip = self.try_body_skips == Some(true);

        match body_fields {
            Some(body_fields) if body_may_skip => {
                let catch_fields = self.snapshot_class_value_field_state();
                self.join_class_value_field_states(&[body_fields, catch_fields]);
            }
            Some(body_fields) => {
                self.restore_class_value_field_state(&body_fields);
            }
            None => {}
        }

        self.try_body_skips = Some(false);
        if let Some(entry_fields) = &entry_fields {
            self.degrade_class_value_fields_written_since(
                writes_before,
                exits_before,
                entry_fields,
            );
        }
        let finally = t.finalizer.as_ref().map(|f| {
            scope.push();
            self.rust_scope_depth = self.rust_scope_depth.saturating_add(1);
            let block = HirStmt::Block(self.walk_block_with_predeclare(senv, &f.body, scope));
            self.rust_scope_depth = self.rust_scope_depth.saturating_sub(1);
            scope.pop();
            Box::new(block)
        });
        let finally_may_skip = self.try_body_skips == Some(true);

        let escapes = (t.handler.is_none() && body_may_skip) || catch_may_skip || finally_may_skip;
        self.try_body_skips = saved_skips.map(|previous| previous || escapes);

        out.push(HirStmt::Try {
            body: Box::new(body),
            catch,
            finally,
        });
    }

    pub(crate) fn lower_destructuring_assignment(
        &mut self,
        senv: &mut SkeletonEnv,
        expr: &Expression<'_>,
        history: &[ArrayStmtRef<'_>],
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
        read_order: Option<usize>,
    ) -> bool {
        let mut expr = expr;
        while let Expression::ParenthesizedExpression(paren) = expr {
            expr = &paren.expression;
        }
        if let Expression::SequenceExpression(sequence) = expr {
            if !sequence_is_all_destructuring(sequence) {
                return false;
            }
            let mut accumulated: Vec<HirStmt> = Vec::new();
            for item in &sequence.expressions {
                let extended: Vec<ArrayStmtRef<'_>> = history
                    .iter()
                    .copied()
                    .chain(accumulated.iter().map(ArrayStmtRef::Stmt))
                    .collect();
                let mut item_out: Vec<HirStmt> = Vec::new();
                self.lower_destructuring_assignment(
                    senv,
                    item,
                    &extended,
                    &mut item_out,
                    scope,
                    read_order,
                );
                accumulated.extend(item_out);
            }
            out.extend(accumulated);
            return true;
        }
        let Expression::AssignmentExpression(assign) = expr else {
            return false;
        };
        let Some(pattern) = assign.left.as_assignment_target_pattern() else {
            return false;
        };
        match pattern {
            AssignmentTargetPattern::ArrayAssignmentTarget(array_pattern) => {
                self.lower_array_destructuring_assignment(
                    senv,
                    assign,
                    array_pattern,
                    history,
                    out,
                    scope,
                    read_order,
                );
            }
            AssignmentTargetPattern::ObjectAssignmentTarget(_) => {
                senv.report_unsupported(
                    "E0500",
                    "an object destructuring assignment is not supported by the body walker; \
                     assign each property separately",
                    assign.span,
                );
            }
        }
        true
    }

    fn lower_array_destructuring_assignment(
        &mut self,
        senv: &mut SkeletonEnv,
        assign: &AssignmentExpression<'_>,
        array_pattern: &oxc_ast::ast::ArrayAssignmentTarget<'_>,
        history: &[ArrayStmtRef<'_>],
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
        read_order: Option<usize>,
    ) {
        if assign.operator != AssignmentOperator::Assign {
            senv.report_unsupported(
                "E0500",
                "a compound operator on an array destructuring target is not supported by the \
                 body walker",
                assign.span,
            );
            return;
        }
        if let Some(rest) = &array_pattern.rest {
            senv.report_unsupported(
                "E0500",
                "an array destructuring assignment with `...rest` is not supported by the body \
                 walker",
                rest.span,
            );
            return;
        }
        let rhs = self.walk_expr(senv, &assign.right, scope);
        let init_ty_id = rhs.ty();
        let Some(Type::Array { element }) = senv.types.resolve(init_ty_id) else {
            senv.report_unsupported(
                "E0500",
                "an array destructuring assignment requires an array type on the right-hand side",
                array_pattern.span,
            );
            return;
        };
        let element_ty_id = *element;
        let span = core_span_from_oxc(array_pattern.span);
        let present: Vec<bool> = (0..array_pattern.elements.len())
            .map(|idx| {
                array_element_definitely_present(
                    &rhs,
                    idx,
                    history,
                    scope.body_id(),
                    &senv.destructured_temp_ids,
                    &ProvableArrayFacts {
                        facts: &self.provable_array_facts,
                        in_loop: !self.loop_frames.is_empty(),
                        module_writes: &self.module_global_writes,
                        read_order,
                    },
                )
            })
            .collect();
        let destructured_uid = senv.next_destructured_id.get();
        senv.next_destructured_id.set(destructured_uid + 1);
        let temp_name = Atom::from(format!("_destructured_{destructured_uid}"));
        let temp_id = LocalId::from_raw(scope.peek_next());
        scope.set_next(temp_id.raw().saturating_add(1));
        senv.destructured_temp_ids
            .insert((scope.body_id(), temp_id));
        out.push(HirStmt::Let {
            id: temp_id,
            name: temp_name,
            ty: init_ty_id,
            init: Some(rhs),
            narrowed: false,
        });
        let temp_ref = HirExpr::Local {
            id: temp_id,
            ty: init_ty_id,
            span,
        };
        for (idx, element) in array_pattern.elements.iter().enumerate() {
            let Some(element) = element else {
                continue;
            };
            let AssignmentTargetMaybeDefault::AssignmentTargetIdentifier(id) = element else {
                senv.report_unsupported(
                    "E0500",
                    "only a plain binding name is supported as an array destructuring assignment \
                     element (no nested patterns, defaults or member targets)",
                    element.span(),
                );
                continue;
            };
            let target_name = id.name.as_str();
            let target = senv.ident_to_expr(target_name, scope, core_span_from_oxc(id.span));
            let target = self.redirect_narrowed_target(target, scope);
            let target_ty = match &target {
                HirExpr::Local { ty, .. } | HirExpr::Global { ty, .. } => *ty,
                _ => {
                    senv.report_unsupported(
                        "E0500",
                        format!(
                            "`{target_name}` cannot be the target of an array destructuring \
                             assignment in this AOT target"
                        )
                        .as_str(),
                        id.span,
                    );
                    continue;
                }
            };
            let present_here = present.get(idx).copied().unwrap_or(false);
            let (init_call, binding_ty, optional_element) =
                senv.destructured_element_read(&temp_ref, idx, element_ty_id, present_here, span);
            let target_accepts =
                destructured_read_fits_target(senv.types, target_ty, binding_ty, optional_element);
            if !target_accepts {
                let absent = if optional_element {
                    "may be absent or `undefined`"
                } else {
                    "may be `undefined`"
                };
                senv.report_unsupported(
                    "P0018",
                    format!(
                        "`{target_name}` is the target of an array destructuring assignment, but \
                         the destructured element {absent}; the target's type cannot represent \
                         that value. Widen the target's type to accept it, or rename the target"
                    )
                    .as_str(),
                    element.span(),
                );
                continue;
            }
            out.push(HirStmt::Expr {
                expr: HirExpr::Assignment {
                    target: Box::new(target),
                    value: Box::new(init_call),
                    ty: binding_ty,
                    span,
                },
            });
        }
    }

    fn walk_var_decl(
        &mut self,
        senv: &mut SkeletonEnv,
        v: &VariableDeclaration<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        for d in &v.declarations {
            let simple_binding = matches!(&d.id, BindingPattern::BindingIdentifier(_));
            let previous_statement_expr = self.own_statement_expr;
            if simple_binding {
                self.own_statement_expr = true;
            }
            let name = binding_pattern_name(&d.id);
            let is_var = matches!(v.kind, VariableDeclarationKind::Var);
            let parameter_merge = if is_var {
                name.as_ref()
                    .and_then(|name| known_parameter_binding(senv, scope, name.as_str()))
            } else {
                None
            };
            let function_merge = if is_var && parameter_merge.is_none() {
                name.as_ref()
                    .and_then(|name| scope.function_scoped_binding(name.as_str()))
            } else {
                None
            };
            if is_var {
                match &d.id {
                    BindingPattern::ArrayPattern(array_pat) => {
                        for element in array_pat.elements.iter().flatten() {
                            if let BindingPattern::BindingIdentifier(id) = element
                                && known_parameter_binding(senv, scope, id.name.as_str()).is_some()
                            {
                                scope.remove_predeclared_shadow(id.name.as_str());
                            }
                        }
                    }
                    _ => {
                        if parameter_merge.is_some()
                            && let Some(name) = &name
                        {
                            scope.remove_predeclared_shadow(name.as_str());
                        }
                    }
                }
            }
            let init = d.init.as_ref().map(|e| self.walk_expr(senv, e, scope));
            self.own_statement_expr = previous_statement_expr;
            if let BindingPattern::ArrayPattern(array_pat) = &d.id {
                let Some(init_expr) = init else {
                    senv.report_unwalked(
                        "array destructuring without initializer is not yet supported",
                        d.span,
                    );
                    continue;
                };
                if array_pat.rest.is_some() {
                    senv.report_unwalked(
                        "array destructuring with `...rest` is not yet supported",
                        d.span,
                    );
                    continue;
                }
                let declared = senv.expand_array_destructuring(
                    array_pat,
                    init_expr,
                    out,
                    scope,
                    matches!(v.kind, VariableDeclarationKind::Var),
                    matches!(v.kind, VariableDeclarationKind::Var) && self.rust_scope_depth == 0,
                    &ProvableArrayFacts {
                        facts: &self.provable_array_facts,
                        in_loop: !self.loop_frames.is_empty(),
                        module_writes: &self.module_global_writes,
                        read_order: None,
                    },
                );
                if matches!(
                    v.kind,
                    VariableDeclarationKind::Let | VariableDeclarationKind::Var
                ) {
                    for id in declared {
                        self.mutable_locals.insert((scope.body_id(), id));
                    }
                }
                continue;
            }
            let Some(name) = name else {
                senv.report_unwalked(
                    "destructuring binding is not supported by the body walker",
                    d.span,
                );
                continue;
            };
            let merge = parameter_merge
                .map(|(id, ty)| (id, ty, true))
                .or_else(|| function_merge.map(|(id, ty)| (id, ty, false)));
            if let Some((merge_id, merge_ty, is_parameter)) = merge {
                scope.widen(name.as_str());
                let nullish_init = matches!(
                    init.as_ref(),
                    Some(
                        HirExpr::Null(_)
                            | HirExpr::Undefined(_)
                            | HirExpr::Unit(_)
                            | HirExpr::Unary {
                                op: HirUnaryOp::Void,
                                ..
                            }
                    )
                );
                let accepts_nullish = matches!(
                    senv.types.resolve(merge_ty),
                    Some(Type::Optional { .. } | Type::Null | Type::Void)
                );
                if nullish_init && !accepts_nullish {
                    let site = if is_parameter {
                        "names a parameter, so its initializer is written into the parameter"
                    } else {
                        "redeclares a function-scoped `var`, so its initializer is written into \
                         that binding"
                    };
                    senv.report_unsupported(
                        "P0018",
                        format!(
                            "`var {} = ...` {site}; the initializer is nullish, but the target's \
                             type is not optional. The AOT target represents an optional value as \
                             `Option<T>` and has no implicit nullish-to-value unwrapping; widen \
                             the target's type to accept `null`/`undefined`, or rename the local",
                            name.as_str()
                        )
                        .as_str(),
                        d.span,
                    );
                    continue;
                }
                let copy_back = init
                    .as_ref()
                    .and_then(|init_expr| self.finish_statement_expression(init_expr, senv));
                if let Some(init_expr) = init {
                    let span = core_span_from_oxc(d.span);
                    let value_ty = crate::expr::literal_hir_ty(senv.types, &init_expr)
                        .unwrap_or_else(|| {
                            let value_ty = init_expr.ty();
                            if value_ty == TypeId::from_raw(0) {
                                senv.error_ty()
                            } else {
                                value_ty
                            }
                        });
                    out.push(HirStmt::Expr {
                        expr: HirExpr::Assignment {
                            target: Box::new(HirExpr::Local {
                                id: merge_id,
                                ty: merge_ty,
                                span,
                            }),
                            value: Box::new(init_expr),
                            ty: value_ty,
                            span,
                        },
                    });
                }
                if let Some(stmt) = copy_back {
                    out.push(stmt);
                }
                continue;
            }
            let mut ty = if d.type_annotation.is_some() {
                self.resolve_ts_type_from_annotation_in_declaration_scope(
                    senv,
                    d.type_annotation.as_deref(),
                    Some(scope),
                )
            } else {
                senv.error_ty()
            };
            let mut init = init;
            if d.type_annotation.is_none()
                && let Some(HirExpr::Closure { ty: fn_ty, .. }) = &init
            {
                ty = *fn_ty;
            }
            if d.type_annotation.is_none()
                && let Some(HirExpr::New { ty: new_ty, .. }) = &init
            {
                ty = *new_ty;
            }
            if d.type_annotation.is_none()
                && matches!(senv.types.resolve(ty), None | Some(Type::Error))
                && let Some(result_ty) = init.as_ref().map(HirExpr::ty)
                && matches!(
                    senv.types.resolve(result_ty),
                    Some(Type::GeneratorResult { .. } | Type::AsyncGenYield { .. })
                )
            {
                ty = result_ty;
            }
            if d.type_annotation.is_none()
                && matches!(senv.types.resolve(ty), None | Some(Type::Error))
            {
                let inferred = init.as_ref().and_then(|e| {
                    crate::expr::literal_hir_ty(senv.types, e).or_else(|| {
                        e.has_static_type()
                            .then(|| e.ty())
                            .filter(|t| !matches!(senv.types.resolve(*t), None | Some(Type::Error)))
                    })
                });
                if let Some(t) = inferred {
                    ty = t;
                }
            }
            if let Some(HirExpr::ArrayLiteral { ty: arr_ty, .. }) = init.as_mut()
                && matches!(senv.types.resolve(ty), Some(Type::Array { .. }))
            {
                *arr_ty = ty;
            }
            if init.is_none()
                && d.type_annotation.is_some()
                && matches!(v.kind, VariableDeclarationKind::Var)
                && self.switch_case_frames.is_empty()
            {
                ty = crate::util::with_undefined(senv.types, ty);
            }
            let class_value = match &init {
                Some(HirExpr::DefaultConstruct {
                    ty: class_ty, span, ..
                }) => Some((*class_ty, *span)),
                _ => None,
            };
            let generic_class = matches!(
                &init,
                Some(HirExpr::DefaultConstruct { ty, .. })
                    if senv.program.declarations.iter().any(|d| matches!(
                        d,
                        ts_aot_ir_hir::HirDecl::Class(c)
                            if c.ty == *ty && !c.type_params.is_empty()
                    ))
            );
            if let Some((class_ty, span)) = class_value
                && !generic_class
            {
                let tag = self.class_value_tag(class_ty);
                init = Some(HirExpr::Int(tag, span));
                ty = senv.types.intern(&ts_aot_core::Type::I64);
            }
            let id = if is_var && self.rust_scope_depth == 0 {
                scope.declare_function_scoped(name.as_str(), ty)
            } else {
                scope.declare(name.as_str(), ty)
            };
            if matches!(
                v.kind,
                VariableDeclarationKind::Let | VariableDeclarationKind::Var
            ) {
                self.mutable_locals.insert((scope.body_id(), id));
            }
            let switch_kind = if matches!(v.kind, VariableDeclarationKind::Var) {
                SwitchCaseEventKind::VarDecl
            } else {
                SwitchCaseEventKind::Decl
            };
            self.note_switch_case_declaration(
                id,
                &Atom::from(name.as_str()),
                scope.depth(),
                switch_kind,
            );
            if let Some(init_expr) = init.as_ref()
                && self.tracked_this_capture(init_expr)
                && let Some(top) = self.this_capture_locals.last_mut()
            {
                top.insert(id);
            }
            if let Some(bodies) = init.as_ref().and_then(crate::skeleton::closure_bodies_of) {
                for body in bodies {
                    scope.set_closure_body(id, body);
                }
            } else {
                match &init {
                    Some(HirExpr::Local { id: source, .. }) => {
                        match scope.closure_bodies_of_local(*source) {
                            Some(bodies) => {
                                for body in bodies {
                                    scope.set_closure_body(id, body);
                                }
                            }
                            None => scope.poison_closure_body(id),
                        }
                    }
                    Some(init) if crate::skeleton::may_hold_a_callable(init) => {
                        scope.poison_closure_body(id);
                    }
                    _ => {}
                }
            }
            if let Some((class_ty, _)) = class_value {
                self.class_value_locals.insert(id, vec![class_ty]);
            }
            let fresh_base =
                ClassValueFieldBase::Local(scope.body_id(), self.class_value_storage(id));
            if matches!(init, Some(HirExpr::New { .. })) {
                self.note_fresh_field_base(fresh_base);
            } else {
                self.forget_fresh_field_base(&fresh_base);
            }
            if let Some(HirExpr::Local { id: source, .. }) = &init {
                let storage = self.class_value_storage(*source);
                if let Some(classes) = self.class_value_locals.get(&storage).cloned() {
                    self.record_loop_class_value_copies(&[(storage, classes.clone())], d.span);
                    self.class_value_locals.insert(id, classes);
                }
                if self.unknown_class_value_locals.contains_key(&storage) {
                    self.unknown_class_value_locals
                        .insert(id, name.as_str().to_owned());
                }
            } else if let Some(HirExpr::Global { name: source, .. }) = &init {
                if let Some(storage) = self.captured_local_id(source.as_str()) {
                    if let Some(classes) = self.captured_class_values(source.as_str(), d.span) {
                        self.class_value_locals.insert(id, classes);
                    }
                    if self.unknown_class_value_locals.contains_key(&storage) {
                        self.unknown_class_value_locals
                            .insert(id, name.as_str().to_owned());
                    }
                } else if let Some(classes) = self.class_value_globals.get(source.as_str()).cloned()
                {
                    self.record_class_value_global_copy(&[source.as_str()], d.span);
                    self.class_value_locals.insert(id, classes);
                }
            }
            match (&v.kind, init.as_ref()) {
                (VariableDeclarationKind::Const, Some(init_expr)) if class_value.is_none() => {
                    let facts = self.class_value_poison_facts(scope.body_id(), init_expr);
                    self.class_value_poisoned_locals
                        .insert((scope.body_id(), id), facts);
                }
                _ => {
                    self.class_value_poisoned_locals
                        .remove(&(scope.body_id(), id));
                }
            }
            if class_value.is_none()
                && matches!(
                    v.kind,
                    VariableDeclarationKind::Const | VariableDeclarationKind::Let
                )
                && let Some(HirExpr::Int(value, _)) = init.as_ref()
            {
                self.constant_int_locals.insert(id, *value);
            } else {
                self.constant_int_locals.remove(&id);
            }
            let copy_back = match (simple_binding, init.as_ref()) {
                (true, Some(init)) => self.finish_statement_expression(init, senv),
                _ => None,
            };
            let provable_presence = init.as_ref().and_then(|init_expr| match init_expr {
                HirExpr::ArrayLiteral { elements, .. } => Some(
                    elements
                        .iter()
                        .map(|element| !matches!(element, HirExpr::Undefined(_)))
                        .collect::<Vec<bool>>(),
                ),
                _ => None,
            });
            out.push(HirStmt::Let {
                id,
                name: Atom::from(name.as_str()),
                ty,
                init,
                narrowed: false,
            });
            if let Some(presence) = provable_presence
                && self.conditional_depth == 0
            {
                self.provable_array_facts.insert(
                    id,
                    ProvableArrayFact {
                        presence,
                        body_id: scope.body_id(),
                    },
                );
            }
            if let Some(stmt) = copy_back {
                out.push(stmt);
            }
        }
    }

    fn walk_c_for(
        &mut self,
        senv: &mut SkeletonEnv,
        f: &ForStatement<'_>,
        out: &mut Vec<HirStmt>,
        scope: &mut BodyScope,
    ) {
        scope.push();
        let mut block: Vec<HirStmt> = Vec::new();
        if let Some(init) = &f.init {
            match init {
                ForStatementInit::VariableDeclaration(v) => {
                    self.walk_var_decl(senv, v, &mut block, scope);
                }
                e @ match_expression!(ForStatementInit) => {
                    let expr = self.walk_expr(senv, e.to_expression(), scope);
                    block.push(HirStmt::Expr { expr });
                }
            }
        }
        self.widen_before_loop(scope, &Self::for_effects(f));
        self.enter_class_value_branch();
        let cond = match &f.test {
            Some(e) => self.walk_expr(senv, e, scope),
            None => HirExpr::Bool(true, Span::default()),
        };
        let mut loop_body = Vec::new();
        self.rust_scope_depth = self.rust_scope_depth.saturating_add(1);
        self.walk_stmt(senv, &f.body, &mut loop_body, scope);
        self.rust_scope_depth = self.rust_scope_depth.saturating_sub(1);
        if let Some(update) = &f.update {
            let expr = self.walk_expr(senv, update, scope);
            inject_update_before_continue(&mut loop_body, &expr);
            loop_body.push(HirStmt::Expr { expr });
        }
        self.leave_class_value_branch();
        block.push(HirStmt::While {
            cond,
            body: Box::new(HirStmt::Block(loop_body)),
        });
        scope.pop();
        out.push(HirStmt::Block(block));
    }

    fn for_binding(
        &mut self,
        senv: &mut SkeletonEnv,
        left: &ForStatementLeft<'_>,
        scope: &mut BodyScope,
    ) -> LocalId {
        if let ForStatementLeft::VariableDeclaration(v) = left
            && let Some(d) = v.declarations.first()
            && let Some(name) = binding_pattern_name(&d.id)
        {
            let ty = if d.type_annotation.is_some() {
                self.resolve_ts_type_from_annotation_in_scope(
                    senv,
                    d.type_annotation.as_deref(),
                    Some(scope),
                )
            } else {
                senv.error_ty()
            };
            let mutable = matches!(
                v.kind,
                VariableDeclarationKind::Let | VariableDeclarationKind::Var
            );
            let id = scope.declare(name.as_str(), ty);
            if mutable {
                self.mutable_locals.insert((scope.body_id(), id));
            }
            return id;
        }
        if let t @ match_assignment_target!(ForStatementLeft) = left
            && let t = t.to_assignment_target()
            && let Some(name) = extract_simple_target_name(t)
        {
            senv.report_unsupported(
                "E0500",
                format!(
                    "a loop head that assigns to the existing binding `{name}` is not supported \
                     by the body walker; declare the loop binding with `let` or `const`"
                )
                .as_str(),
                left_span(left),
            );
            let ty = senv.error_ty();
            let id = scope.declare(&name, ty);
            self.mutable_locals.insert((scope.body_id(), id));
            return id;
        }
        senv.report_unsupported(
            "E0500",
            "loop binding must be a simple `let`/`const` identifier; destructuring and member \
             loop bindings are not supported by the body walker",
            left_span(left),
        );
        let ty = senv.error_ty();
        scope.declare("_", ty)
    }
}

fn extract_simple_target_name(t: &oxc_ast::ast::AssignmentTarget<'_>) -> Option<String> {
    use oxc_ast::ast::SimpleAssignmentTarget;
    match t.as_simple_assignment_target()? {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn inject_update_before_continue(stmts: &mut Vec<HirStmt>, update: &HirExpr) {
    let mut i = 0;
    while i < stmts.len() {
        if matches!(stmts[i], HirStmt::Continue { .. }) {
            stmts.insert(
                i,
                HirStmt::Expr {
                    expr: update.clone(),
                },
            );
            i += 2;
            continue;
        }
        inject_update_in_child(&mut stmts[i], update);
        i += 1;
    }
}

fn inject_update_in_child(s: &mut HirStmt, update: &HirExpr) {
    match s {
        HirStmt::Block(inner) => inject_update_before_continue(inner, update),
        HirStmt::If {
            then, otherwise, ..
        } => {
            inject_update_in_boxed(then, update);
            if let Some(otherwise) = otherwise {
                inject_update_in_boxed(otherwise, update);
            }
        }
        HirStmt::Switch { cases, .. } => {
            for c in cases {
                inject_update_before_continue(&mut c.body, update);
            }
        }
        HirStmt::Try {
            body,
            catch,
            finally,
        } => {
            inject_update_in_boxed(body, update);
            if let Some(catch) = catch {
                inject_update_in_boxed(&mut catch.body, update);
            }
            if let Some(finally) = finally {
                inject_update_in_boxed(finally, update);
            }
        }
        _ => {}
    }
}

fn inject_update_in_boxed(s: &mut Box<HirStmt>, update: &HirExpr) {
    if matches!(**s, HirStmt::Continue { .. }) {
        let cont = std::mem::replace(s.as_mut(), HirStmt::Break { label: None });
        **s = HirStmt::Block(vec![
            HirStmt::Expr {
                expr: update.clone(),
            },
            cont,
        ]);
    } else {
        inject_update_in_child(s, update);
    }
}
impl SkeletonBuilder {
    fn narrow_after_assignment(
        &mut self,
        senv: &mut SkeletonEnv,
        ast: &Expression<'_>,
        walked: &mut HirExpr,
        scope: &mut BodyScope,
    ) -> Option<HirStmt> {
        let Expression::AssignmentExpression(assign) = ast else {
            return None;
        };
        let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(ident)) =
            assign.left.as_simple_assignment_target()
        else {
            return None;
        };
        let name = ident.name.as_str().to_owned();
        if let HirExpr::CompoundUpdate { op, .. } = walked
            && !matches!(
                op,
                HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or
            )
        {
            return None;
        }
        if self.suppress_narrowing > 0 {
            return None;
        }
        match walked {
            HirExpr::Assignment { .. } => {}
            HirExpr::CompoundUpdate { op, .. } if !matches!(op, HirBinaryOp::And) => {}
            _ => return None,
        }
        let current = scope.type_of_visible(&name)?;
        let inner = non_nullish_part(current, senv.types)?;
        let (storage_id, storage_ty) = assignment_target_local(walked)?;
        let can_narrow = {
            let walked: &HirExpr = walked;
            let assigned = match walked {
                HirExpr::Assignment { value, .. } => value.as_ref(),
                HirExpr::CompoundUpdate { rhs, .. } => rhs.as_ref(),
                _ => return None,
            };
            assigned_is_non_nullish(assigned, senv.types)
                && assignment_can_narrow(inner, assigned, senv.types)
        };
        if !can_narrow {
            scope.widen(&name);
            return None;
        }
        let span = core_span_from_oxc(assign.span);
        let binding = NarrowedBinding {
            name: Atom::from(name.as_str()),
            storage_id,
            storage_ty,
            inner,
            span,
        };
        Some(self.narrowed_binding_stmt(senv, scope, &binding))
    }

    fn narrowed_binding_stmt(
        &mut self,
        senv: &mut SkeletonEnv,
        scope: &mut BodyScope,
        binding: &NarrowedBinding,
    ) -> HirStmt {
        let id = LocalId::from_raw(scope.peek_next());
        scope.set_next(id.raw().saturating_add(1));
        let init = HirExpr::TypeAssertion {
            expr: Box::new(HirExpr::Local {
                id: binding.storage_id,
                ty: binding.storage_ty,
                span: binding.span,
            }),
            target: binding.inner,
            span: binding.span,
        };
        scope.narrow(binding.name.as_str(), id, binding.inner);
        if let Some(frame) = self.narrowed_stack.last_mut() {
            frame.insert(id, (binding.storage_id, binding.storage_ty));
        }
        let visible = scope.names();
        let mut uid = senv.next_destructured_id.get();
        let mut binding_name = Atom::from(format!("{NARROWED_LOCAL_PREFIX}{uid}"));
        while self.declared_name_is_taken(binding_name.as_str())
            || visible
                .iter()
                .any(|visible| visible == binding_name.as_str())
        {
            uid += 1;
            binding_name = Atom::from(format!("{NARROWED_LOCAL_PREFIX}{uid}"));
        }
        senv.next_destructured_id.set(uid + 1);
        HirStmt::narrowed_let(id, binding_name, binding.inner, Some(init))
    }

    fn undefined_guard(
        senv: &SkeletonEnv<'_>,
        test: &Expression<'_>,
        scope: &BodyScope,
    ) -> Option<UndefinedGuard> {
        let (name, undefined_is_then) = undefined_guard_test(test)?;
        let storage_ty = scope.type_of_visible(name)?;
        let storage_id = scope.visible_binding(name)?;
        let inner = undefined_only_inner(storage_ty, senv.types)?;
        Some(UndefinedGuard {
            binding: NarrowedBinding {
                name: Atom::from(name),
                storage_id,
                storage_ty,
                inner,
                span: core_span_from_oxc(test.span()),
            },
            undefined_is_then,
        })
    }

    fn walk_branch_narrowed(
        &mut self,
        senv: &mut SkeletonEnv,
        stmt: &Statement<'_>,
        scope: &mut BodyScope,
        binding: &NarrowedBinding,
        conditional: bool,
    ) -> Box<HirStmt> {
        scope.push();
        let narrowed = self.narrowed_binding_stmt(senv, scope, binding);
        let branch = self.walk_child_maybe_conditional(senv, stmt, scope, conditional);
        scope.pop();
        let mut stmts = vec![narrowed];
        match *branch {
            HirStmt::Block(mut body) => {
                stmts.append(&mut body);
                Box::new(HirStmt::Block(stmts))
            }
            other => {
                stmts.push(other);
                Box::new(HirStmt::Block(stmts))
            }
        }
    }

    fn statement_narrowed_update(
        &self,
        walked: &HirExpr,
    ) -> Option<(LocalId, TypeId, LocalId, TypeId, Span)> {
        let HirExpr::CompoundUpdate {
            op, target, span, ..
        } = walked
        else {
            return None;
        };
        if matches!(
            op,
            HirBinaryOp::Coalesce | HirBinaryOp::And | HirBinaryOp::Or
        ) {
            return None;
        }
        let HirExpr::Local { id, ty, .. } = target.as_ref() else {
            return None;
        };
        let (storage_id, storage_ty) = self.narrowed_storage(*id)?;
        Some((*id, *ty, storage_id, storage_ty, *span))
    }

    fn finish_statement_expression(
        &mut self,
        walked: &HirExpr,
        senv: &mut SkeletonEnv,
    ) -> Option<HirStmt> {
        let root = self.statement_narrowed_update(walked);
        let root_key = root.map(|(narrowed_id, _, _, _, span)| (narrowed_id, span));
        let mut claimed = false;
        for update in std::mem::take(&mut self.pending_narrowed_updates) {
            let is_root = !claimed
                && root_key.is_some_and(|(narrowed_id, span)| {
                    narrowed_id == update.narrowed_id && span == update.span
                });
            if is_root {
                claimed = true;
                continue;
            }
            senv.report_narrowed_update(update.span);
        }
        let (narrowed_id, narrowed_ty, storage_id, storage_ty, span) = root?;
        Some(HirStmt::Expr {
            expr: HirExpr::Assignment {
                target: Box::new(HirExpr::Local {
                    id: storage_id,
                    ty: storage_ty,
                    span,
                }),
                value: Box::new(HirExpr::Local {
                    id: narrowed_id,
                    ty: narrowed_ty,
                    span,
                }),
                ty: storage_ty,
                span,
            },
        })
    }
}

pub(super) fn assignment_target_local(walked: &HirExpr) -> Option<(LocalId, TypeId)> {
    let (HirExpr::Assignment { target, .. } | HirExpr::CompoundUpdate { target, .. }) = walked
    else {
        return None;
    };
    match target.as_ref() {
        HirExpr::Local { id, ty, .. } => Some((*id, *ty)),
        _ => None,
    }
}

fn non_nullish_part(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.resolve(ty) {
        Some(Type::Optional { inner, .. }) => match types.resolve(*inner) {
            None | Some(Type::Error) => None,
            Some(_) => Some(*inner),
        },
        Some(Type::Union { variants })
            if variants
                .iter()
                .any(|v| matches!(types.resolve(*v), Some(Type::Null | Type::Void))) =>
        {
            None
        }
        None | Some(Type::Error | Type::Null | Type::Void) => None,
        Some(_) => Some(ty),
    }
}

fn assigned_is_non_nullish(assigned: &HirExpr, types: &TypeTable) -> bool {
    match assigned {
        HirExpr::Null(_) | HirExpr::Unit(_) | HirExpr::Undefined(_) => false,
        HirExpr::Int(..) | HirExpr::Float(..) | HirExpr::String(..) | HirExpr::Bool(..) => true,
        other => non_nullish_part(other.ty(), types).is_some(),
    }
}

fn assignment_can_narrow(inner: TypeId, assigned: &HirExpr, types: &TypeTable) -> bool {
    match assigned {
        HirExpr::Null(_) | HirExpr::Unit(_) | HirExpr::Undefined(_) => false,
        HirExpr::Int(..) | HirExpr::Float(..) => is_numeric_ty(inner, types),
        HirExpr::String(..) => matches!(types.resolve(inner), Some(Type::String)),
        HirExpr::Bool(..) => matches!(types.resolve(inner), Some(Type::Bool)),
        other => {
            let assigned_ty = other.ty();
            if types.resolve(inner) == types.resolve(assigned_ty) {
                return true;
            }
            is_numeric_ty(inner, types) && is_numeric_ty(assigned_ty, types)
        }
    }
}

fn destructured_read_fits_target(
    types: &TypeTable,
    target_ty: TypeId,
    binding_ty: TypeId,
    optional_element: bool,
) -> bool {
    if target_ty == binding_ty {
        return true;
    }
    let target = types.resolve(target_ty);
    if matches!(target, Some(Type::Error)) {
        return true;
    }
    let target_inner = match target {
        Some(Type::Optional { inner, .. }) => Some(*inner),
        _ => None,
    };
    if target_inner == Some(binding_ty) {
        return true;
    }
    if let (
        Some(Type::Optional {
            inner: target_inner,
            nullish: target_kinds,
        }),
        Some(Type::Optional {
            inner: read_inner,
            nullish: read_kinds,
        }),
    ) = (target, types.resolve(binding_ty))
    {
        let covers = (!read_kinds.null || target_kinds.null)
            && (!read_kinds.undefined || target_kinds.undefined);
        if covers && target_inner == read_inner {
            return true;
        }
    }
    !optional_element
        && is_numeric_ty(binding_ty, types)
        && (is_numeric_ty(target_ty, types)
            || target_inner.is_some_and(|inner| is_numeric_ty(inner, types)))
}

fn is_numeric_ty(ty: TypeId, types: &TypeTable) -> bool {
    matches!(
        types.resolve(ty),
        Some(
            Type::F32
                | Type::F64
                | Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
        )
    )
}

enum SwitchStart {
    At(usize),
    NoCase,
    Unknown,
}

fn constant_switch_start(
    disc: &HirExpr,
    known: Option<i64>,
    cases: &[HirSwitchCase],
) -> SwitchStart {
    let matches_test = |case: &HirSwitchCase| match (disc, case.test.as_ref()) {
        (HirExpr::Int(value, _), Some(HirExpr::Int(test, _))) => value == test,
        (HirExpr::String(value, _), Some(HirExpr::String(test, _))) => value == test,
        _ => false,
    };
    if matches!(disc, HirExpr::Int(..) | HirExpr::String(..)) {
        if let Some(index) = cases.iter().position(matches_test) {
            return SwitchStart::At(index);
        }
        return default_switch_start(cases);
    }
    let Some(known) = known else {
        return SwitchStart::Unknown;
    };
    match cases
        .iter()
        .position(|case| matches!(&case.test, Some(HirExpr::Int(test, _)) if *test == known))
    {
        Some(index) => SwitchStart::At(index),
        None => default_switch_start(cases),
    }
}

fn default_switch_start(cases: &[HirSwitchCase]) -> SwitchStart {
    cases
        .iter()
        .position(|case| case.test.is_none())
        .map_or(SwitchStart::NoCase, SwitchStart::At)
}

fn hir_case_ends_early(stmts: &[HirStmt]) -> bool {
    match stmts.last() {
        Some(
            HirStmt::Break { .. }
            | HirStmt::Continue { .. }
            | HirStmt::Return { .. }
            | HirStmt::Throw { .. },
        ) => true,
        Some(HirStmt::Block(inner)) => hir_case_ends_early(inner),
        _ => false,
    }
}

fn check_switch_case_tdz(
    senv: &mut SkeletonEnv<'_>,
    disc: &HirExpr,
    known: Option<i64>,
    cases: &[HirSwitchCase],
    events: &[Vec<SwitchCaseEvent>],
    span: oxc_span::Span,
) {
    let mut declared: Vec<(LocalId, Atom, Option<LocalId>)> = Vec::new();
    let mut var_declared: Vec<(LocalId, Atom)> = Vec::new();
    for case_events in events {
        for SwitchCaseEvent {
            local,
            kind,
            name,
            shadowed,
        } in case_events
        {
            let Some(name) = name else {
                continue;
            };
            match kind {
                SwitchCaseEventKind::Decl => {
                    if !declared.iter().any(|(other, _, _)| other == local) {
                        declared.push((*local, name.clone(), *shadowed));
                    }
                }
                SwitchCaseEventKind::VarDecl => {
                    if !var_declared.iter().any(|(other, _)| other == local) {
                        var_declared.push((*local, name.clone()));
                    }
                }
                SwitchCaseEventKind::Use => {}
            }
        }
    }
    if declared.is_empty() && var_declared.is_empty() {
        return;
    }
    let starts: Vec<usize> = match constant_switch_start(disc, known, cases) {
        SwitchStart::At(index) => vec![index],
        SwitchStart::NoCase => Vec::new(),
        SwitchStart::Unknown => (0..cases.len()).collect(),
    };
    let span = core_span_from_oxc(span);
    for (id, name, shadowed) in declared {
        let mut reported = false;
        for &start in &starts {
            let mut initialized = false;
            for (index, case) in cases.iter().enumerate().skip(start) {
                if let Some(case_events) = events.get(index) {
                    for SwitchCaseEvent { local, kind, .. } in case_events {
                        let same_local = *local == id;
                        let shadow_use =
                            !same_local && shadowed.is_some_and(|outer| outer == *local);
                        if !same_local && !shadow_use {
                            continue;
                        }
                        if *kind == SwitchCaseEventKind::Decl {
                            if same_local {
                                initialized = true;
                            }
                        } else if *kind == SwitchCaseEventKind::Use && !initialized {
                            senv.report_unsupported(
                                "E0419",
                                format!(
                                    "`{name}` is declared in a `switch` case whose declaration \
                                     may not run before this use; that is a temporal-dead-zone \
                                     error at run time, so declare it before the `switch`"
                                )
                                .as_str(),
                                span,
                            );
                            reported = true;
                            break;
                        }
                    }
                }
                if reported || hir_case_ends_early(&case.body) {
                    break;
                }
            }
            if reported {
                break;
            }
        }
    }
    for (id, name) in var_declared {
        let Some(ty) = case_local_type(cases, id) else {
            continue;
        };
        if switch_case_local_is_hoistable(ty, senv.types) {
            continue;
        }
        let mut reported = false;
        for &start in &starts {
            let mut initialized = false;
            for (index, case) in cases.iter().enumerate().skip(start) {
                if let Some(case_events) = events.get(index) {
                    for SwitchCaseEvent { local, kind, .. } in case_events {
                        if *local != id {
                            continue;
                        }
                        match kind {
                            SwitchCaseEventKind::VarDecl => initialized = true,
                            SwitchCaseEventKind::Use if !initialized => {
                                senv.report_unsupported(
                                    "E0419",
                                    format!(
                                        "`{name}` is declared with `var` in a `switch` case \
                                         whose declaration may not run before this use, and its \
                                         type has no default value, so declare it before the \
                                         `switch`"
                                    )
                                    .as_str(),
                                    span,
                                );
                                reported = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                if reported || hir_case_ends_early(&case.body) {
                    break;
                }
            }
            if reported {
                break;
            }
        }
    }
}

fn case_local_type(cases: &[HirSwitchCase], id: LocalId) -> Option<TypeId> {
    cases.iter().find_map(|case| {
        case.body.iter().find_map(|stmt| match stmt {
            HirStmt::Let { id: local, ty, .. } if *local == id => Some(*ty),
            _ => None,
        })
    })
}

fn switch_case_local_is_hoistable(ty: TypeId, types: &TypeTable) -> bool {
    match types.resolve(ty) {
        Some(Type::String | Type::Array { .. } | Type::Fn { .. }) => false,
        Some(Type::Struct { .. } | Type::Named { .. }) => false,
        Some(
            Type::Optional { inner, .. }
            | Type::GeneratorResult { inner }
            | Type::AsyncGenYield { inner }
            | Type::Generator { inner, .. }
            | Type::Promise { ok: inner, .. }
            | Type::WeakMap { value: inner, .. },
        ) => switch_case_local_is_hoistable(*inner, types),
        Some(Type::Result { ok, err }) => {
            switch_case_local_is_hoistable(*ok, types)
                && switch_case_local_is_hoistable(*err, types)
        }
        _ => true,
    }
}

fn ends_with_a_terminator(stmts: &[Statement<'_>]) -> bool {
    match stmts.last() {
        Some(
            Statement::BreakStatement(_)
            | Statement::ReturnStatement(_)
            | Statement::ThrowStatement(_)
            | Statement::ContinueStatement(_),
        ) => true,
        Some(Statement::BlockStatement(block)) => ends_with_a_terminator(&block.body),
        _ => false,
    }
}

fn statement_always_terminates(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::BreakStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::ContinueStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_always_terminates),
        Statement::IfStatement(i) => i.alternate.as_ref().is_some_and(|alternate| {
            statement_always_terminates(&i.consequent) && statement_always_terminates(alternate)
        }),
        _ => false,
    }
}

fn statement_leaves_case(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::BreakStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::ContinueStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_leaves_case),
        Statement::IfStatement(i) => {
            statement_leaves_case(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|alternate| statement_leaves_case(alternate))
        }
        Statement::WhileStatement(w) => statement_contains_return_or_throw(&w.body),
        Statement::DoWhileStatement(d) => statement_contains_return_or_throw(&d.body),
        Statement::ForStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForInStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForOfStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::SwitchStatement(s) => s
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(statement_contains_return_or_throw)),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(statement_leaves_case)
                || t.handler
                    .as_ref()
                    .is_some_and(|handler| handler.body.body.iter().any(statement_leaves_case))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|finalizer| finalizer.body.iter().any(statement_leaves_case))
        }
        Statement::LabeledStatement(l) => statement_leaves_case(&l.body),
        _ => false,
    }
}

fn statement_contains_return_or_throw(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => {
            block.body.iter().any(statement_contains_return_or_throw)
        }
        Statement::IfStatement(i) => {
            statement_contains_return_or_throw(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|alternate| statement_contains_return_or_throw(alternate))
        }
        Statement::WhileStatement(w) => statement_contains_return_or_throw(&w.body),
        Statement::DoWhileStatement(d) => statement_contains_return_or_throw(&d.body),
        Statement::ForStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForInStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::ForOfStatement(f) => statement_contains_return_or_throw(&f.body),
        Statement::SwitchStatement(s) => s
            .cases
            .iter()
            .any(|c| c.consequent.iter().any(statement_contains_return_or_throw)),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(statement_contains_return_or_throw)
                || t.handler.as_ref().is_some_and(|handler| {
                    handler
                        .body
                        .body
                        .iter()
                        .any(statement_contains_return_or_throw)
                })
                || t.finalizer.as_ref().is_some_and(|finalizer| {
                    finalizer
                        .body
                        .iter()
                        .any(statement_contains_return_or_throw)
                })
        }
        Statement::LabeledStatement(l) => statement_contains_return_or_throw(&l.body),
        _ => false,
    }
}

fn reachable_case_prefix(stmts: &[Statement<'_>]) -> usize {
    match stmts.iter().position(statement_always_terminates) {
        Some(index) => index + 1,
        None => stmts.len(),
    }
}

fn case_covers_its_end(stmts: &[Statement<'_>]) -> bool {
    let Some(cut) = stmts.iter().position(statement_always_terminates) else {
        return true;
    };
    cut == 0 || !statement_leaves_case(&stmts[0])
}

fn collect_array_uses_stmts<'b, 'a>(
    stmts: &'b [Statement<'a>],
    name: &str,
    shadowed: bool,
    nested: &HashSet<String>,
    out: &mut Vec<&'b Expression<'a>>,
) {
    let shadowed = shadowed || stmts.iter().any(|stmt| statement_declares_name(stmt, name));
    let mut next = nested.clone();
    for stmt in stmts {
        collect_stmt_declared_names(stmt, &mut next);
    }
    for stmt in stmts {
        collect_array_uses_stmt(stmt, name, shadowed, &next, out);
    }
}

fn collect_stmt_declared_names(stmt: &Statement<'_>, out: &mut HashSet<String>) {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            for declarator in &decl.declarations {
                let mut names: Vec<Atom> = Vec::new();
                collect_binding_names(&declarator.id, &mut names);
                for declared in names {
                    out.insert(declared.as_str().to_owned());
                }
            }
        }
        Statement::FunctionDeclaration(func) => {
            if let Some(id) = &func.id {
                out.insert(id.name.as_str().to_owned());
            }
        }
        Statement::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                out.insert(id.name.as_str().to_owned());
            }
        }
        _ => {}
    }
}

fn declarations_declare_name(decl: &VariableDeclaration<'_>, name: &str) -> bool {
    decl.declarations
        .iter()
        .any(|declarator| binding_pattern_declares(&declarator.id, name))
}

fn binding_pattern_declares(pattern: &BindingPattern<'_>, name: &str) -> bool {
    let mut names: Vec<Atom> = Vec::new();
    collect_binding_names(pattern, &mut names);
    names.iter().any(|declared| declared.as_str() == name)
}

fn for_init_declares_name(init: &ForStatementInit<'_>, name: &str) -> bool {
    match init {
        ForStatementInit::VariableDeclaration(decl) => declarations_declare_name(decl, name),
        _ => false,
    }
}

fn for_left_declares_name(left: &ForStatementLeft<'_>, name: &str) -> bool {
    match left {
        ForStatementLeft::VariableDeclaration(decl) => declarations_declare_name(decl, name),
        _ => false,
    }
}

fn statement_declares_name(stmt: &Statement<'_>, name: &str) -> bool {
    match stmt {
        Statement::VariableDeclaration(decl) => declarations_declare_name(decl, name),
        Statement::FunctionDeclaration(func) => {
            func.id.as_ref().is_some_and(|id| id.name.as_str() == name)
        }
        Statement::ClassDeclaration(class) => {
            class.id.as_ref().is_some_and(|id| id.name.as_str() == name)
        }
        _ => false,
    }
}

fn collect_array_uses_stmt<'b, 'a>(
    stmt: &'b Statement<'a>,
    name: &str,
    shadowed: bool,
    nested: &HashSet<String>,
    out: &mut Vec<&'b Expression<'a>>,
) {
    match stmt {
        Statement::ExpressionStatement(e) => {
            collect_array_uses_expr(&e.expression, name, shadowed, nested, out);
        }
        Statement::ReturnStatement(r) => {
            if let Some(argument) = &r.argument {
                collect_array_uses_expr(argument, name, shadowed, nested, out);
            }
        }
        Statement::ThrowStatement(t) => {
            collect_array_uses_expr(&t.argument, name, shadowed, nested, out);
        }
        Statement::VariableDeclaration(d) => {
            for declarator in &d.declarations {
                if let Some(init) = &declarator.init {
                    collect_array_uses_expr(init, name, shadowed, nested, out);
                }
            }
        }
        Statement::BlockStatement(b) => {
            collect_array_uses_stmts(&b.body, name, shadowed, nested, out);
        }
        Statement::IfStatement(i) => {
            collect_array_uses_expr(&i.test, name, shadowed, nested, out);
            collect_array_uses_stmt(&i.consequent, name, shadowed, nested, out);
            if let Some(alternate) = &i.alternate {
                collect_array_uses_stmt(alternate, name, shadowed, nested, out);
            }
        }
        Statement::WhileStatement(w) => {
            collect_array_uses_expr(&w.test, name, shadowed, nested, out);
            collect_array_uses_stmt(&w.body, name, shadowed, nested, out);
        }
        Statement::DoWhileStatement(d) => {
            collect_array_uses_expr(&d.test, name, shadowed, nested, out);
            collect_array_uses_stmt(&d.body, name, shadowed, nested, out);
        }
        Statement::ForStatement(f) => {
            let loop_shadowed = shadowed
                || f.init
                    .as_ref()
                    .is_some_and(|init| for_init_declares_name(init, name));
            if let Some(init) = &f.init
                && let init @ match_expression!(ForStatementInit) = init
            {
                collect_array_uses_expr(init.to_expression(), name, shadowed, nested, out);
            }
            if let Some(update) = &f.update {
                collect_array_uses_expr(update, name, loop_shadowed, nested, out);
            }
            collect_array_uses_stmt(&f.body, name, loop_shadowed, nested, out);
        }
        Statement::ForInStatement(f) => {
            collect_array_uses_expr(&f.right, name, shadowed, nested, out);
            let body_shadowed = shadowed || for_left_declares_name(&f.left, name);
            collect_array_uses_stmt(&f.body, name, body_shadowed, nested, out);
        }
        Statement::ForOfStatement(f) => {
            collect_array_uses_expr(&f.right, name, shadowed, nested, out);
            let body_shadowed = shadowed || for_left_declares_name(&f.left, name);
            collect_array_uses_stmt(&f.body, name, body_shadowed, nested, out);
        }
        Statement::SwitchStatement(s) => {
            collect_array_uses_expr(&s.discriminant, name, shadowed, nested, out);
            let cases_shadowed = shadowed
                || s.cases.iter().any(|case| {
                    case.consequent
                        .iter()
                        .any(|stmt| statement_declares_name(stmt, name))
                });
            let mut cases_nested = nested.clone();
            for case in &s.cases {
                for stmt in &case.consequent {
                    collect_stmt_declared_names(stmt, &mut cases_nested);
                }
            }
            for case in &s.cases {
                collect_array_uses_stmts(
                    &case.consequent,
                    name,
                    cases_shadowed,
                    &cases_nested,
                    out,
                );
            }
        }
        Statement::TryStatement(t) => {
            collect_array_uses_stmts(&t.block.body, name, shadowed, nested, out);
            if let Some(handler) = &t.handler {
                let handler_shadowed = shadowed
                    || handler
                        .param
                        .as_ref()
                        .is_some_and(|param| binding_pattern_declares(&param.pattern, name));
                let mut handler_nested = nested.clone();
                if let Some(param) = &handler.param {
                    let mut names: Vec<Atom> = Vec::new();
                    collect_binding_names(&param.pattern, &mut names);
                    for declared in names {
                        handler_nested.insert(declared.as_str().to_owned());
                    }
                }
                collect_array_uses_stmts(
                    &handler.body.body,
                    name,
                    handler_shadowed,
                    &handler_nested,
                    out,
                );
            }
            if let Some(finalizer) = &t.finalizer {
                collect_array_uses_stmts(&finalizer.body, name, shadowed, nested, out);
            }
        }
        Statement::LabeledStatement(l) => {
            collect_array_uses_stmt(&l.body, name, shadowed, nested, out);
        }
        _ => {}
    }
}

fn is_shadowed_identifier(expr: &Expression<'_>, nested: &HashSet<String>) -> bool {
    matches!(expr, Expression::Identifier(id) if nested.contains(id.name.as_str()))
}

fn strip_parens<'b, 'a>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    let mut current = expr;
    while let Expression::ParenthesizedExpression(p) = current {
        current = &p.expression;
    }
    current
}

fn collect_array_uses_expr<'b, 'a>(
    expr: &'b Expression<'a>,
    name: &str,
    shadowed: bool,
    nested: &HashSet<String>,
    out: &mut Vec<&'b Expression<'a>>,
) {
    if shadowed {
        return;
    }
    match strip_parens(expr) {
        Expression::CallExpression(call) => {
            let Expression::StaticMemberExpression(member) = strip_parens(&call.callee) else {
                return;
            };
            if !matches!(
                strip_parens(&member.object),
                Expression::Identifier(id) if id.name.as_str() == name
            ) || !matches!(member.property.name.as_str(), "push" | "unshift")
            {
                return;
            }
            for argument in &call.arguments {
                let argument @ match_expression!(Argument) = argument else {
                    continue;
                };
                let argument = strip_parens(argument.to_expression());
                if !is_shadowed_identifier(argument, nested) {
                    out.push(argument);
                }
            }
        }
        Expression::AssignmentExpression(a) => {
            let Some(oxc_ast::ast::MemberExpression::ComputedMemberExpression(computed)) =
                a.left.as_member_expression()
            else {
                return;
            };
            if matches!(
                strip_parens(&computed.object),
                Expression::Identifier(id) if id.name.as_str() == name
            ) && !is_shadowed_identifier(strip_parens(&a.right), nested)
            {
                out.push(strip_parens(&a.right));
            }
        }
        _ => {}
    }
}

fn concrete_hint_ty(senv: &SkeletonEnv, ty: TypeId) -> Option<TypeId> {
    match senv.types.resolve(ty) {
        Some(Type::Error | Type::Void | Type::Null | Type::Optional { .. }) | None => None,
        Some(_) => Some(ty),
    }
}

fn array_element_value_ty(
    senv: &mut SkeletonEnv,
    builder: &SkeletonBuilder,
    scope: &BodyScope,
    expr: &Expression<'_>,
) -> Option<TypeId> {
    match expr {
        Expression::NumericLiteral(_) => Some(senv.types.intern(&Type::F64)),
        Expression::StringLiteral(_) => Some(senv.types.intern(&Type::String)),
        Expression::BooleanLiteral(_) => Some(senv.types.intern(&Type::Bool)),
        Expression::BigIntLiteral(_) => Some(senv.types.intern(&Type::BigInt)),
        Expression::Identifier(id) => {
            let ty = scope.type_of_visible(id.name.as_str())?;
            concrete_hint_ty(senv, ty)
        }
        Expression::NewExpression(new_expr) => {
            let Expression::Identifier(id) = &new_expr.callee else {
                return None;
            };
            scope
                .visible_class(id.name.as_str())
                .map(|binding| binding.ty)
                .or_else(|| builder.class_types.get(id.name.as_str()).copied())
        }
        _ => None,
    }
}
