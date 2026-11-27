use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    Argument, FunctionBody, Statement, match_assignment_target, match_expression,
    match_member_expression,
};
use ts_aot_core::{Atom, Diagnostic, LocalId, Span as CoreSpan, Type, TypeId};
use ts_aot_ir_hir::{
    HirCallee, HirDecl, HirExpr, HirFunction, HirParam, HirStmt, Visitor, walk_expr,
};

use crate::scope::BodyScope;
use crate::skeleton::{CaptureCheckFrame, LocalGeneratorAlias, SkeletonBuilder, SkeletonEnv};

pub(crate) struct FunctionBodyInput<'a, 'b> {
    pub body: Option<&'a FunctionBody<'b>>,
    pub params: &'a [HirParam],
    pub extra_locals: &'a [(Atom, TypeId, HirExpr)],
    pub items: &'a [oxc_ast::ast::FormalParameter<'b>],
    pub item_offset: usize,
    pub param_base: u32,
    pub outer_classes: Option<&'a BodyScope>,
}

impl SkeletonBuilder {
    pub(crate) fn declare_params_with_prelude(
        &mut self,
        senv: &mut SkeletonEnv,
        scope: &mut BodyScope,
        params: &[HirParam],
        extra_locals: &[(Atom, TypeId, HirExpr)],
        items: &[oxc_ast::ast::FormalParameter<'_>],
        item_offset: usize,
    ) -> Vec<HirStmt> {
        for p in params {
            scope.declare_param(p.name.as_str(), p.id, p.ty);
            self.mutable_locals.insert((scope.body_id(), p.id));
        }
        let mut prelude: Vec<HirStmt> = Vec::new();
        let mut destructured: Vec<(Atom, TypeId, HirExpr)> = extra_locals.to_vec();
        for (i, item) in items.iter().enumerate() {
            let Some(param) = params.get(i + item_offset) else {
                break;
            };
            if let Some(init) = &item.initializer
                && let Some(name) = crate::util::binding_pattern_name(&item.pattern)
            {
                let inner_ty = match senv.types.resolve(param.ty) {
                    Some(Type::Optional { inner, .. }) => *inner,
                    _ => param.ty,
                };
                let span = crate::util::core_span_from_oxc(item.span);
                let abi_value = HirExpr::Local {
                    id: param.id,
                    ty: param.ty,
                    span,
                };
                let default_value = self.walk_expr(senv, init, scope);
                let init_expr = HirExpr::Binary {
                    op: ts_aot_ir_hir::HirBinaryOp::Coalesce,
                    lhs: Box::new(abi_value),
                    rhs: Box::new(default_value),
                    ty: inner_ty,
                    span,
                };
                let id = LocalId::from_raw(scope.peek_next());
                scope.set_next(id.raw().saturating_add(1));
                scope.declare_param(name.as_str(), id, inner_ty);
                self.mutable_locals.insert((scope.body_id(), id));
                prelude.push(HirStmt::Let {
                    id,
                    name: Atom::from(name),
                    ty: inner_ty,
                    init: Some(init_expr),
                    narrowed: false,
                });
            } else if param.name.as_str() == "_" {
                let mut names: Vec<Atom> = Vec::new();
                crate::util::collect_binding_names(&item.pattern, &mut names);
                for name in names {
                    let Some(pos) = destructured
                        .iter()
                        .position(|(existing, _, _)| existing.as_str() == name.as_str())
                    else {
                        continue;
                    };
                    let (name, ty, init) = destructured.remove(pos);
                    let id = scope.declare(name.as_str(), ty);
                    self.mutable_locals.insert((scope.body_id(), id));
                    prelude.push(HirStmt::Let {
                        id,
                        name,
                        ty,
                        init: Some(init),
                        narrowed: false,
                    });
                }
            }
        }
        for (name, ty, init) in destructured {
            let id = scope.declare(name.as_str(), ty);
            self.mutable_locals.insert((scope.body_id(), id));
            prelude.push(HirStmt::Let {
                id,
                name,
                ty,
                init: Some(init),
                narrowed: false,
            });
        }
        prelude
    }

    pub(crate) fn walk_function_body(
        &mut self,
        senv: &mut SkeletonEnv,
        input: &FunctionBodyInput<'_, '_>,
        is_generator: bool,
        inherit_this_scope: bool,
        fresh_class_values: bool,
    ) -> (Vec<HirStmt>, u32) {
        let body = input.body;
        let params = input.params;
        let extra_locals = input.extra_locals;
        let param_base = input.param_base;
        let base = param_base.max(
            params
                .iter()
                .map(|p| p.id.raw())
                .max()
                .map_or(0, |m| m.saturating_add(1)),
        );
        let Some(body) = body else {
            return (Vec::new(), base);
        };
        self.body_depth = self.body_depth.saturating_add(1);
        self.capture_check_frames.push(CaptureCheckFrame::default());
        self.generator_alias_stack.push(HashMap::new());
        let this_id = params
            .iter()
            .find(|p| p.name.as_str() == "this")
            .map(|p| p.id);
        self.this_local_stack.push(this_id);
        self.this_capture_locals.push(HashSet::new());
        let saved_class_values = if fresh_class_values {
            Some((
                std::mem::take(&mut self.class_value_locals),
                std::mem::take(&mut self.unknown_class_value_locals),
            ))
        } else {
            None
        };
        if fresh_class_values {
            self.pending_capture_class_value_reads.push(Vec::new());
        }
        let saved_static = self.in_static_method;
        if !inherit_this_scope {
            self.in_static_method = false;
        }
        let saved_module_init = std::mem::take(&mut self.module_init_depth);
        let saved_loop_frames = std::mem::take(&mut self.loop_frames);
        let saved_rust_scope = std::mem::take(&mut self.rust_scope_depth);
        self.is_generator_stack.push(is_generator);
        self.narrowed_stack.push(std::collections::HashMap::new());
        self.captured_stack.push(std::collections::HashSet::new());
        self.declared_names_stack
            .push(collect_declared_names(&body.statements, input.items));
        let mut scope = BodyScope::new(base);
        if let Some(outer) = input.outer_classes {
            scope.inherit_classes(outer);
        }
        let mut prelude = self.declare_params_with_prelude(
            senv,
            &mut scope,
            params,
            extra_locals,
            input.items,
            input.item_offset,
        );
        let mut result = self.walk_block_with_predeclare(senv, &body.statements, &mut scope);
        self.is_generator_stack.pop();
        self.narrowed_stack.pop();
        self.captured_stack.pop();
        self.declared_names_stack.pop();
        self.in_static_method = saved_static;
        self.module_init_depth = saved_module_init;
        self.loop_frames = saved_loop_frames;
        self.rust_scope_depth = saved_rust_scope;
        self.this_local_stack.pop();
        self.this_capture_locals.pop();
        if !prelude.is_empty() {
            prelude.append(&mut result);
            result = prelude;
        }
        let local_generators = self.generator_alias_stack.pop().unwrap_or_default();
        Self::report_local_generators_before_declaration(senv, &result, &local_generators);
        self.finish_capture_check_frame(senv, &result, &body.statements);
        if fresh_class_values {
            self.verify_capture_class_value_reads(senv);
        }
        if let Some((class_values, unknown_class_values)) = saved_class_values {
            self.class_value_locals = class_values;
            self.unknown_class_value_locals = unknown_class_values;
        }
        let used = scope.peek_next();
        self.body_depth = self.body_depth.saturating_sub(1);
        (result, used)
    }

    fn report_local_generators_before_declaration(
        senv: &mut SkeletonEnv,
        body: &[HirStmt],
        aliases: &HashMap<String, LocalGeneratorAlias>,
    ) {
        if aliases.is_empty() {
            return;
        }
        let functions: HashMap<&Atom, &HirFunction> = senv
            .program
            .declarations
            .iter()
            .filter_map(|decl| match decl {
                HirDecl::Function(function) => Some((&function.name, function)),
                _ => None,
            })
            .collect();
        let mut scan = GeneratorSourceRefScan {
            aliases,
            found: None,
        };
        for stmt in body {
            scan.visit_stmt(stmt);
        }
        for alias in aliases.values() {
            let Some(wrapper) = functions.get(&alias.lifted) else {
                continue;
            };
            for stmt in &wrapper.body {
                scan.visit_stmt(stmt);
            }
            if let Some(implementation) = forwarded_generator_implementation(&wrapper.body)
                && let Some(implementation) = functions.get(&implementation)
            {
                for stmt in &implementation.body {
                    scan.visit_stmt(stmt);
                }
            }
        }
        if let Some((name, span)) = scan.found {
            senv.diagnostics.push(Diagnostic::error(
                "E0500",
                format!(
                    "the local generator `{name}` is referenced before its declaration; move the \
                     declaration above the first reference"
                ),
                span,
            ));
        }
    }

    pub(crate) fn walk_block_with_predeclare(
        &mut self,
        senv: &mut SkeletonEnv,
        stmts: &[oxc_ast::ast::Statement<'_>],
        scope: &mut BodyScope,
    ) -> Vec<HirStmt> {
        predeclare_forward_declarations(stmts, scope, senv.error_ty());
        let result = self.walk_stmts(senv, stmts, scope);
        self.repair_capture_field_types(senv, scope);
        result
    }

    fn repair_capture_field_types(&mut self, senv: &mut SkeletonEnv<'_>, scope: &BodyScope) {
        if self.class_capture_fields.is_empty() {
            return;
        }
        let body_id = scope.body_id();
        let class_tys: Vec<TypeId> = self
            .class_capture_fields
            .keys()
            .filter(|ty| self.class_decl_body.get(ty).copied() == Some(body_id))
            .copied()
            .collect();
        for class_ty in class_tys {
            let repairs: Vec<(Atom, LocalId, TypeId)> = self
                .class_capture_fields
                .get(&class_ty)
                .map_or_else(Vec::new, |entries| {
                    entries
                        .iter()
                        .filter(|(_, _, _, binding_ty, _)| {
                            matches!(senv.types.resolve(*binding_ty), None | Some(Type::Error))
                        })
                        .filter_map(|(field, _, binding, _, _)| {
                            let real_ty = scope.type_of_local(*binding)?;
                            if matches!(senv.types.resolve(real_ty), None | Some(Type::Error)) {
                                return None;
                            }
                            Some((field.clone(), *binding, real_ty))
                        })
                        .collect()
                });
            if repairs.is_empty() {
                continue;
            }
            let repaired_ty = |name: &Atom| -> Option<TypeId> {
                repairs
                    .iter()
                    .find(|(field, _, _)| field == name)
                    .map(|(_, _, ty)| *ty)
            };
            if let Some(entries) = self.class_capture_fields.get_mut(&class_ty) {
                for entry in entries.iter_mut() {
                    if repairs
                        .iter()
                        .any(|(field, binding, _)| *field == entry.0 && *binding == entry.2)
                        && let Some(real_ty) = repaired_ty(&entry.0)
                    {
                        entry.3 = real_ty;
                    }
                }
            }
            if let Some(fields) = senv.program.class_fields.get_mut(&class_ty) {
                for field in &mut *fields {
                    if let Some(real_ty) = repaired_ty(&field.name) {
                        field.ty = real_ty;
                    }
                }
            }
            for decl in &mut senv.program.declarations {
                if let HirDecl::Class(class) = decl
                    && class.ty == class_ty
                {
                    for field in &mut class.fields {
                        if let Some(real_ty) = repaired_ty(&field.name) {
                            field.ty = real_ty;
                        }
                    }
                }
            }
            if let Some(inits) = senv.program.class_field_inits.get_mut(&class_ty) {
                for (name, init) in inits.iter_mut() {
                    if let Some(real_ty) = repaired_ty(name)
                        && let HirExpr::Local { ty, .. } = init
                    {
                        *ty = real_ty;
                    }
                }
            }
        }
    }

    pub(crate) fn current_function_is_generator(&self) -> bool {
        self.is_generator_stack
            .last()
            .copied()
            .expect("is_generator_stack must be non-empty at every call site (walk_function_body pushes before invoking the walker)")
    }
}

#[derive(Default)]
pub(crate) struct LoopEffects {
    pub(crate) writes: HashSet<String>,
    pub(crate) has_effect: bool,
}

impl LoopEffects {
    pub(crate) fn scan_expr(&mut self, expr: &oxc_ast::ast::Expression<'_>) {
        scan_loop_expr(expr, self);
    }

    pub(crate) fn scan_stmt(&mut self, stmt: &Statement<'_>) {
        scan_loop_stmt(stmt, self);
    }

    pub(crate) fn scan_stmts(&mut self, stmts: &[Statement<'_>]) {
        for stmt in stmts {
            scan_loop_stmt(stmt, self);
        }
    }

    pub(crate) fn scan_for_init(&mut self, init: &oxc_ast::ast::ForStatementInit<'_>) {
        use oxc_ast::ast::ForStatementInit;
        match init {
            ForStatementInit::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    if let Some(init) = &declarator.init {
                        self.scan_expr(init);
                    }
                }
            }
            e @ match_expression!(ForStatementInit) => self.scan_expr(e.to_expression()),
        }
    }

    pub(crate) fn scan_for_left(&mut self, left: &oxc_ast::ast::ForStatementLeft<'_>) {
        use oxc_ast::ast::{ForStatementLeft, SimpleAssignmentTarget};
        match left {
            ForStatementLeft::VariableDeclaration(_) => {}
            t @ match_assignment_target!(ForStatementLeft) => {
                let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(id)) =
                    t.to_assignment_target().as_simple_assignment_target()
                else {
                    self.has_effect = true;
                    return;
                };
                self.writes.insert(id.name.as_str().to_owned());
            }
        }
    }
}

fn scan_loop_member(member: &oxc_ast::ast::MemberExpression<'_>, out: &mut LoopEffects) {
    use oxc_ast::ast::MemberExpression as ME;
    match member {
        ME::ComputedMemberExpression(computed) => {
            scan_loop_expr(&computed.object, out);
            scan_loop_expr(&computed.expression, out);
        }
        ME::StaticMemberExpression(static_member) => scan_loop_expr(&static_member.object, out),
        ME::PrivateFieldExpression(private) => scan_loop_expr(&private.object, out),
    }
}

fn scan_loop_expr(expr: &oxc_ast::ast::Expression<'_>, out: &mut LoopEffects) {
    use oxc_ast::ast::{ArrayExpressionElement, ChainElement, Expression as E, ObjectPropertyKind};
    match expr {
        E::BooleanLiteral(_)
        | E::NullLiteral(_)
        | E::NumericLiteral(_)
        | E::BigIntLiteral(_)
        | E::RegExpLiteral(_)
        | E::StringLiteral(_)
        | E::Identifier(_)
        | E::MetaProperty(_)
        | E::Super(_)
        | E::ThisExpression(_) => {}
        E::TemplateLiteral(template) => {
            for expr in &template.expressions {
                scan_loop_expr(expr, out);
            }
        }
        E::ArrayExpression(array) => {
            for element in &array.elements {
                match element {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        scan_loop_expr(&spread.argument, out);
                    }
                    e @ match_expression!(ArrayExpressionElement) => {
                        scan_loop_expr(e.to_expression(), out);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                }
            }
        }
        E::ObjectExpression(object) => {
            for property in &object.properties {
                match property {
                    ObjectPropertyKind::ObjectProperty(property) => {
                        scan_loop_expr(&property.value, out);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        scan_loop_expr(&spread.argument, out);
                    }
                }
            }
        }
        E::ParenthesizedExpression(paren) => scan_loop_expr(&paren.expression, out),
        E::BinaryExpression(binary) => {
            scan_loop_expr(&binary.left, out);
            scan_loop_expr(&binary.right, out);
        }
        E::LogicalExpression(logical) => {
            scan_loop_expr(&logical.left, out);
            scan_loop_expr(&logical.right, out);
        }
        E::UnaryExpression(unary) => scan_loop_expr(&unary.argument, out),
        E::UpdateExpression(update) => {
            if let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) =
                &update.argument
            {
                out.writes.insert(id.name.as_str().to_owned());
            } else {
                out.has_effect = true;
            }
        }
        E::AssignmentExpression(assign) => {
            match assign.left.as_simple_assignment_target() {
                Some(oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id)) => {
                    out.writes.insert(id.name.as_str().to_owned());
                }
                _ => out.has_effect = true,
            }
            scan_loop_expr(&assign.right, out);
        }
        E::CallExpression(call) => {
            out.has_effect = true;
            scan_loop_expr(&call.callee, out);
            for argument in &call.arguments {
                match argument {
                    a @ match_expression!(Argument) => {
                        scan_loop_expr(a.to_expression(), out);
                    }
                    _ => out.has_effect = true,
                }
            }
        }
        E::NewExpression(new_expr) => {
            out.has_effect = true;
            scan_loop_expr(&new_expr.callee, out);
            for argument in &new_expr.arguments {
                match argument {
                    a @ match_expression!(Argument) => {
                        scan_loop_expr(a.to_expression(), out);
                    }
                    _ => out.has_effect = true,
                }
            }
        }
        E::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => {
                out.has_effect = true;
                scan_loop_expr(&call.callee, out);
                for argument in &call.arguments {
                    match argument {
                        a @ match_expression!(Argument) => {
                            scan_loop_expr(a.to_expression(), out);
                        }
                        _ => out.has_effect = true,
                    }
                }
            }
            ChainElement::TSNonNullExpression(non_null) => {
                scan_loop_expr(&non_null.expression, out);
            }
            other @ match_member_expression!(ChainElement) => {
                scan_loop_member(other.to_member_expression(), out);
            }
        },
        E::AwaitExpression(await_expr) => {
            out.has_effect = true;
            scan_loop_expr(&await_expr.argument, out);
        }
        E::YieldExpression(yield_expr) => {
            out.has_effect = true;
            if let Some(argument) = &yield_expr.argument {
                scan_loop_expr(argument, out);
            }
        }
        E::ConditionalExpression(conditional) => {
            scan_loop_expr(&conditional.test, out);
            scan_loop_expr(&conditional.consequent, out);
            scan_loop_expr(&conditional.alternate, out);
        }
        E::SequenceExpression(sequence) => {
            for expr in &sequence.expressions {
                scan_loop_expr(expr, out);
            }
        }
        E::TaggedTemplateExpression(tagged) => {
            out.has_effect = true;
            scan_loop_expr(&tagged.tag, out);
            for expr in &tagged.quasi.expressions {
                scan_loop_expr(expr, out);
            }
        }
        E::ImportExpression(import) => {
            out.has_effect = true;
            scan_loop_expr(&import.source, out);
        }
        E::TSAsExpression(as_expr) => scan_loop_expr(&as_expr.expression, out),
        E::TSSatisfiesExpression(satisfies) => scan_loop_expr(&satisfies.expression, out),
        E::TSTypeAssertion(assertion) => scan_loop_expr(&assertion.expression, out),
        E::TSNonNullExpression(non_null) => scan_loop_expr(&non_null.expression, out),
        E::TSInstantiationExpression(instantiation) => {
            scan_loop_expr(&instantiation.expression, out);
        }
        E::ArrowFunctionExpression(_) | E::FunctionExpression(_) => {}
        E::ClassExpression(_) => out.has_effect = true,
        other @ match_member_expression!(E) => {
            scan_loop_member(other.to_member_expression(), out);
        }
        _ => out.has_effect = true,
    }
}

fn scan_loop_stmt(stmt: &Statement<'_>, out: &mut LoopEffects) {
    use oxc_ast::ast::{Declaration, Statement as S};
    match stmt {
        S::BlockStatement(block) => out.scan_stmts(&block.body),
        S::IfStatement(if_stmt) => {
            out.scan_expr(&if_stmt.test);
            out.scan_stmt(&if_stmt.consequent);
            if let Some(alternate) = &if_stmt.alternate {
                out.scan_stmt(alternate);
            }
        }
        S::WhileStatement(while_stmt) => {
            out.scan_expr(&while_stmt.test);
            out.scan_stmt(&while_stmt.body);
        }
        S::DoWhileStatement(do_while) => {
            out.scan_stmt(&do_while.body);
            out.scan_expr(&do_while.test);
        }
        S::ForStatement(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                out.scan_for_init(init);
            }
            if let Some(test) = &for_stmt.test {
                out.scan_expr(test);
            }
            if let Some(update) = &for_stmt.update {
                out.scan_expr(update);
            }
            out.scan_stmt(&for_stmt.body);
        }
        S::ForInStatement(for_in) => {
            out.scan_for_left(&for_in.left);
            out.scan_expr(&for_in.right);
            out.scan_stmt(&for_in.body);
        }
        S::ForOfStatement(for_of) => {
            out.scan_for_left(&for_of.left);
            out.scan_expr(&for_of.right);
            out.scan_stmt(&for_of.body);
        }
        S::SwitchStatement(switch) => {
            out.scan_expr(&switch.discriminant);
            for case in &switch.cases {
                if let Some(test) = &case.test {
                    out.scan_expr(test);
                }
                out.scan_stmts(&case.consequent);
            }
        }
        S::TryStatement(try_stmt) => {
            out.scan_stmts(&try_stmt.block.body);
            if let Some(handler) = &try_stmt.handler {
                out.scan_stmts(&handler.body.body);
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                out.scan_stmts(&finalizer.body);
            }
        }
        S::LabeledStatement(labeled) => out.scan_stmt(&labeled.body),
        S::ExpressionStatement(expr) => out.scan_expr(&expr.expression),
        S::ReturnStatement(return_stmt) => {
            if let Some(argument) = &return_stmt.argument {
                out.scan_expr(argument);
            }
        }
        S::ThrowStatement(throw_stmt) => out.scan_expr(&throw_stmt.argument),
        S::EmptyStatement(_)
        | S::DebuggerStatement(_)
        | S::BreakStatement(_)
        | S::ContinueStatement(_) => {}
        other => {
            let Some(decl) = other.as_declaration() else {
                out.has_effect = true;
                return;
            };
            match decl {
                Declaration::VariableDeclaration(var_decl) => {
                    for declarator in &var_decl.declarations {
                        if let Some(init) = &declarator.init {
                            out.scan_expr(init);
                        }
                    }
                }
                Declaration::FunctionDeclaration(_) | Declaration::ClassDeclaration(_) => {}
                _ => {}
            }
        }
    }
}

fn collect_binding_names_into(
    pattern: &oxc_ast::ast::BindingPattern<'_>,
    names: &mut HashSet<String>,
) {
    let mut bound: Vec<Atom> = Vec::new();
    crate::util::collect_binding_names(pattern, &mut bound);
    for name in bound {
        names.insert(name.as_str().to_owned());
    }
}

struct GeneratorSourceRefScan<'a> {
    aliases: &'a HashMap<String, LocalGeneratorAlias>,
    found: Option<(String, CoreSpan)>,
}

fn forwarded_generator_implementation(body: &[HirStmt]) -> Option<Atom> {
    let [
        HirStmt::Return {
            value: Some(HirExpr::Call { callee, .. }),
        },
    ] = body
    else {
        return None;
    };
    let HirCallee::Indirect(inner) = callee else {
        return None;
    };
    match inner.as_ref() {
        HirExpr::Global { name, .. } => Some(name.clone()),
        _ => None,
    }
}

impl Visitor for GeneratorSourceRefScan<'_> {
    fn visit_expr(&mut self, expr: &HirExpr) {
        if self.found.is_none()
            && let HirExpr::Global { name, span, .. } = expr
            && self.aliases.contains_key(name.as_str())
        {
            self.found = Some((name.as_str().to_owned(), *span));
        }
        walk_expr(self, expr);
    }
}

fn collect_declared_names(
    stmts: &[Statement<'_>],
    params: &[oxc_ast::ast::FormalParameter<'_>],
) -> HashSet<String> {
    let mut names = HashSet::new();
    for param in params {
        collect_binding_names_into(&param.pattern, &mut names);
    }
    for stmt in stmts {
        collect_declared_names_in_stmt(stmt, &mut names);
    }
    names
}

fn collect_declared_names_in_stmt(stmt: &Statement<'_>, names: &mut HashSet<String>) {
    use oxc_ast::ast::{Declaration, ForStatementInit, ForStatementLeft};
    match stmt {
        Statement::BlockStatement(block) => {
            for stmt in &block.body {
                collect_declared_names_in_stmt(stmt, names);
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_declared_names_in_stmt(&if_stmt.consequent, names);
            if let Some(alternate) = &if_stmt.alternate {
                collect_declared_names_in_stmt(alternate, names);
            }
        }
        Statement::WhileStatement(while_stmt) => {
            collect_declared_names_in_stmt(&while_stmt.body, names);
        }
        Statement::DoWhileStatement(do_while) => {
            collect_declared_names_in_stmt(&do_while.body, names);
        }
        Statement::ForStatement(for_stmt) => {
            if let Some(ForStatementInit::VariableDeclaration(decl)) = &for_stmt.init {
                for declarator in &decl.declarations {
                    collect_binding_names_into(&declarator.id, names);
                }
            }
            collect_declared_names_in_stmt(&for_stmt.body, names);
        }
        Statement::ForInStatement(for_in) => {
            if let ForStatementLeft::VariableDeclaration(decl) = &for_in.left {
                for declarator in &decl.declarations {
                    collect_binding_names_into(&declarator.id, names);
                }
            }
            collect_declared_names_in_stmt(&for_in.body, names);
        }
        Statement::ForOfStatement(for_of) => {
            if let ForStatementLeft::VariableDeclaration(decl) = &for_of.left {
                for declarator in &decl.declarations {
                    collect_binding_names_into(&declarator.id, names);
                }
            }
            collect_declared_names_in_stmt(&for_of.body, names);
        }
        Statement::SwitchStatement(switch) => {
            for case in &switch.cases {
                for stmt in &case.consequent {
                    collect_declared_names_in_stmt(stmt, names);
                }
            }
        }
        Statement::TryStatement(try_stmt) => {
            for stmt in &try_stmt.block.body {
                collect_declared_names_in_stmt(stmt, names);
            }
            if let Some(handler) = &try_stmt.handler {
                if let Some(param) = &handler.param {
                    collect_binding_names_into(&param.pattern, names);
                }
                for stmt in &handler.body.body {
                    collect_declared_names_in_stmt(stmt, names);
                }
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                for stmt in &finalizer.body {
                    collect_declared_names_in_stmt(stmt, names);
                }
            }
        }
        Statement::LabeledStatement(labeled) => {
            collect_declared_names_in_stmt(&labeled.body, names);
        }
        other => {
            let Some(decl) = other.as_declaration() else {
                return;
            };
            match decl {
                Declaration::VariableDeclaration(var_decl) => {
                    for declarator in &var_decl.declarations {
                        collect_binding_names_into(&declarator.id, names);
                    }
                }
                Declaration::FunctionDeclaration(function) => {
                    if let Some(id) = &function.id {
                        names.insert(id.name.as_str().to_owned());
                    }
                }
                Declaration::ClassDeclaration(class) => {
                    if let Some(id) = &class.id {
                        names.insert(id.name.as_str().to_owned());
                    }
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn predeclare_forward_declarations(
    stmts: &[Statement<'_>],
    scope: &mut BodyScope,
    error_ty: TypeId,
) {
    for stmt in stmts {
        if let Some(decl) = stmt.as_declaration() {
            predeclare_in_decl(decl, scope, error_ty);
        }
    }
}

fn predeclare_in_decl(
    decl: &oxc_ast::ast::Declaration<'_>,
    scope: &mut BodyScope,
    error_ty: TypeId,
) {
    match decl {
        oxc_ast::ast::Declaration::FunctionDeclaration(f) => {
            if f.generator {
                return;
            }
            if let Some(id) = f.id.as_ref() {
                scope.predeclare(id.name.as_str(), error_ty);
            }
        }
        oxc_ast::ast::Declaration::VariableDeclaration(v) => {
            let is_var = matches!(v.kind, oxc_ast::ast::VariableDeclarationKind::Var);
            for d in &v.declarations {
                predeclare_binding_pattern(&d.id, scope, error_ty, is_var);
            }
        }
        _ => {}
    }
}

fn predeclare_binding_pattern(
    pattern: &oxc_ast::ast::BindingPattern<'_>,
    scope: &mut BodyScope,
    error_ty: TypeId,
    is_var: bool,
) {
    use oxc_ast::ast::BindingPattern;
    let declare = |scope: &mut BodyScope, name: &str| {
        if is_var {
            scope.predeclare_var(name, error_ty);
        } else {
            scope.predeclare(name, error_ty);
        }
    };
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            declare(scope, id.name.as_str());
        }
        BindingPattern::AssignmentPattern(ap) => {
            predeclare_binding_pattern(&ap.left, scope, error_ty, is_var);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                predeclare_binding_pattern(&prop.value, scope, error_ty, is_var);
            }
            if let Some(rest) = &obj.rest {
                predeclare_binding_pattern(&rest.argument, scope, error_ty, is_var);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for p in arr.elements.iter().flatten() {
                predeclare_binding_pattern(p, scope, error_ty, is_var);
            }
            if let Some(rest) = &arr.rest {
                predeclare_binding_pattern(&rest.argument, scope, error_ty, is_var);
            }
        }
    }
}
