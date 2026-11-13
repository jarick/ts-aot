use std::collections::HashSet;

use oxc_ast::ast::{
    Argument, FunctionBody, Statement, match_assignment_target, match_expression,
    match_member_expression,
};
use ts_aot_core::{Atom, LocalId, Type, TypeId};
use ts_aot_ir_hir::{HirExpr, HirParam, HirStmt};

use crate::scope::BodyScope;
use crate::skeleton::{SkeletonBuilder, SkeletonEnv};

pub(crate) struct FunctionBodyInput<'a, 'b> {
    pub body: Option<&'a FunctionBody<'b>>,
    pub params: &'a [HirParam],
    pub extra_locals: &'a [(Atom, TypeId, HirExpr)],
    /// The source parameter list, aligned with `params` after skipping
    /// `item_offset` synthetic leading parameters (`this`).
    pub items: &'a [oxc_ast::ast::FormalParameter<'b>],
    /// How many leading entries of `params` have no source parameter
    /// (a method's synthetic `this`).
    pub item_offset: usize,
    pub param_base: u32,
}

impl SkeletonBuilder {
    /// Declares the parameters in `scope` and returns the prelude that binds the
    /// names the body uses, in parameter order:
    ///
    /// * a defaulted parameter (`x: T = <default>`) arrives as the hidden
    ///   `Option<T>` ABI parameter and becomes `let x: T = param ?? <default>;`.
    ///   The outer `Option` is the "argument supplied?" marker, so `??` applies
    ///   the default only when the argument is missing (`undefined`) and a
    ///   supplied `null` is preserved - exactly JavaScript's rule. The default
    ///   is evaluated per call, only when the argument is missing, and may use
    ///   the parameters that come before it (JavaScript evaluates defaults left
    ///   to right). The local keeps the *declared* type `T` (nullable included),
    ///   so `null` remains a valid value for it;
    /// * a destructured parameter binds its names from the hidden parameter.
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
                // A fresh local of the same name: the scope now resolves `x` to
                // it, so the body never sees the `Option<T>` ABI parameter.
                let id = LocalId::from_raw(scope.peek_next());
                scope.set_next(id.raw().saturating_add(1));
                scope.declare_param(name.as_str(), id, inner_ty);
                prelude.push(HirStmt::Let {
                    id,
                    name: Atom::from(name),
                    ty: inner_ty,
                    init: Some(init_expr),
                });
            } else if param.name.as_str() == "_" {
                // A destructured parameter: bind the names it introduced.
                let mut names: Vec<Atom> = Vec::new();
                crate::util::collect_binding_names(&item.pattern, &mut names);
                for name in names {
                    let Some(pos) = destructured
                        .iter()
                        .position(|(existing, _, _)| existing.as_str() == name.as_str())
                    else {
                        continue;
                    };
                    // The recorded initializer is the *property* access built by
                    // `destructure` (`param.x`), not the parameter itself.
                    let (name, ty, init) = destructured.remove(pos);
                    let id = scope.declare(name.as_str(), ty);
                    prelude.push(HirStmt::Let {
                        id,
                        name,
                        ty,
                        init: Some(init),
                    });
                }
            }
        }
        for (name, ty, init) in destructured {
            let id = scope.declare(name.as_str(), ty);
            prelude.push(HirStmt::Let {
                id,
                name,
                ty,
                init: Some(init),
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
        // Local ids restart in every function body, so a class value recorded by an earlier
        // body must not classify a same-numbered local here. An arrow function keeps the
        // mapping: its locals continue the enclosing body's ids and it captures them.
        let saved_class_values = if fresh_class_values {
            Some(std::mem::take(&mut self.class_value_locals))
        } else {
            None
        };
        let saved_static = self.in_static_method;
        if !inherit_this_scope {
            self.in_static_method = false;
        }
        self.is_generator_stack.push(is_generator);
        self.narrowed_stack.push(std::collections::HashMap::new());
        self.captured_stack.push(std::collections::HashSet::new());
        self.declared_names_stack
            .push(collect_declared_names(&body.statements, input.items));
        let mut scope = BodyScope::new(base);
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
        if !prelude.is_empty() {
            prelude.append(&mut result);
            result = prelude;
        }
        if let Some(saved) = saved_class_values {
            self.class_value_locals = saved;
        }
        let used = scope.peek_next();
        (result, used)
    }

    pub(crate) fn walk_block_with_predeclare(
        &mut self,
        senv: &mut SkeletonEnv,
        stmts: &[oxc_ast::ast::Statement<'_>],
        scope: &mut BodyScope,
    ) -> Vec<HirStmt> {
        predeclare_forward_declarations(stmts, scope, senv.error_ty());
        self.walk_stmts(senv, stmts, scope)
    }

    pub(crate) fn current_function_is_generator(&self) -> bool {
        self.is_generator_stack
            .last()
            .copied()
            .expect("is_generator_stack must be non-empty at every call site (walk_function_body pushes before invoking the walker)")
    }
}

/// The effects a loop's *own* code has on the frame around it.
///
/// A narrowing created before a loop is a copy materialised once, outside the
/// loop, so a name the loop writes must lose it: every later iteration (and the
/// reads after the loop) would otherwise observe the copy taken before the
/// first iteration. A name the loop never writes keeps its narrowing - the copy
/// is then the variable's value on every iteration, exactly as after a plain
/// assignment.
///
/// A call (or another effectful expression) can run a closure that writes a
/// captured variable, and a closure created *inside* the loop is not visible to
/// the capture machinery before the walk, so any effect withdraws every
/// narrowing of the frame instead of guessing which one it writes.
#[derive(Default)]
pub(crate) struct LoopEffects {
    /// The names the loop region assigns to (`x = ...`, `x += ...`, `x++`,
    /// `for (x of ...)`), including the ones written by its nested statements.
    pub(crate) writes: HashSet<String>,
    /// Whether the loop region can run code the walker cannot attribute (a
    /// call, `new`, `await`/`yield`, a tagged template, a dynamic import).
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
            // The loop binding is a *new* local in the walker (`for_binding`),
            // so it does not write the name it shadows.
            ForStatementLeft::VariableDeclaration(_) => {}
            t @ match_assignment_target!(ForStatementLeft) => {
                let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(id)) =
                    t.to_assignment_target().as_simple_assignment_target()
                else {
                    // A member target evaluates its object/index (possibly a
                    // call); a destructuring target is not lowered to a write.
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
            // The target is recorded for every compound operator: `&&=`/`||=`/
            // `??=` may leave the variable untouched, and keeping a narrowing
            // that a write *may* have invalidated is the unsound direction.
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
        // A nested function body runs only when the closure is called; the
        // writes it performs are attributed when a call is seen instead.
        E::ArrowFunctionExpression(_) | E::FunctionExpression(_) => {}
        // A class body is walked separately (`build_class`); treat an inline
        // definition as an effect rather than guess what its static side runs.
        E::ClassExpression(_) => out.has_effect = true,
        other @ match_member_expression!(E) => {
            scan_loop_member(other.to_member_expression(), out);
        }
        // Anything not modelled here (JSX, V8 intrinsics, ...) may do anything:
        // withdraw every narrowing of the frame rather than guess.
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
                // An unsupported statement form may do anything.
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
                // A nested function/class body runs only when it is called.
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

/// Every name the function body binds, anywhere in its statement tree: the
/// parameters, and `let`/`const`/`var`, function/class declarations, catch and
/// loop bindings, including the ones in nested blocks.
///
/// The walk of a narrowed statement needs this because a generated `_narrowed_N`
/// local must not be shadowed by a *later* declaration of the same name inside a
/// nested block: at the point the narrowing is emitted those declarations are
/// not in scope yet, but the generated Rust `let` would be shadowed by them.
/// Nested function bodies are not descended into: they open their own scope in
/// the generated Rust, and a narrowed local is never captured by a closure, so
/// their bindings cannot shadow it where it is used.
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

fn predeclare_forward_declarations(
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
            if let Some(id) = f.id.as_ref() {
                scope.predeclare(id.name.as_str(), error_ty);
            }
        }
        oxc_ast::ast::Declaration::VariableDeclaration(v) => {
            for d in &v.declarations {
                predeclare_binding_pattern(&d.id, scope, error_ty);
            }
        }
        _ => {}
    }
}

fn predeclare_binding_pattern(
    pattern: &oxc_ast::ast::BindingPattern<'_>,
    scope: &mut BodyScope,
    error_ty: TypeId,
) {
    use oxc_ast::ast::BindingPattern;
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            scope.predeclare(id.name.as_str(), error_ty);
        }
        BindingPattern::AssignmentPattern(ap) => {
            predeclare_binding_pattern(&ap.left, scope, error_ty);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                predeclare_binding_pattern(&prop.value, scope, error_ty);
            }
            if let Some(rest) = &obj.rest {
                predeclare_binding_pattern(&rest.argument, scope, error_ty);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for p in arr.elements.iter().flatten() {
                predeclare_binding_pattern(p, scope, error_ty);
            }
            if let Some(rest) = &arr.rest {
                predeclare_binding_pattern(&rest.argument, scope, error_ty);
            }
        }
    }
}
