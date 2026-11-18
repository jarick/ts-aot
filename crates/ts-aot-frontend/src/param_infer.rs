use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, CallExpression, ChainElement, Declaration, Expression,
    ForStatementInit, FormalParameter, FunctionBody, MemberExpression, NewExpression,
    ObjectPropertyKind, SimpleAssignmentTarget, Statement, VariableDeclaration,
    VariableDeclarationKind, match_declaration, match_expression, match_member_expression,
};
use oxc_syntax::operator::{AssignmentOperator, UnaryOperator};
use ts_aot_core::{Atom, Type, TypeId};
use ts_aot_ir_hir::HirDecl;

use crate::scope::BodyScope;
use crate::skeleton::{SkeletonBuilder, SkeletonEnv};
use crate::util::{binding_pattern_name, collect_binding_names, struct_field_ty};

pub(crate) type ParamHints = HashMap<String, TypeId>;

const ASSERT_SHIM_METHODS: &[(&str, &str)] = &[
    ("sameValue", "assertSameValue"),
    ("notSameValue", "assertNotSameValue"),
    ("throws", "assertThrows"),
];

#[derive(Default)]
struct HintSet {
    agreed: HashMap<String, TypeId>,
    conflicted: HashSet<String>,
}

impl HintSet {
    fn offer(&mut self, name: &str, ty: TypeId) {
        if self.conflicted.contains(name) {
            return;
        }
        match self.agreed.get(name) {
            Some(existing) if *existing == ty => {}
            Some(_) => {
                self.agreed.remove(name);
                self.conflicted.insert(name.to_owned());
            }
            None => {
                self.agreed.insert(name.to_owned(), ty);
            }
        }
    }
}

struct ParamScanner<'s, 'e> {
    senv: &'s mut SkeletonEnv<'e>,
    hints: HintSet,
    shadowed: HashSet<String>,
    local_bindings: HashSet<String>,
    candidates: HashSet<String>,
    ret_ty: Option<TypeId>,
    self_ty: Option<TypeId>,
}

fn unwrap_expr<'a, 'b>(expr: &'a Expression<'b>) -> &'a Expression<'b> {
    let mut current = expr;
    loop {
        current = match current {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::TSAsExpression(e) => &e.expression,
            Expression::TSSatisfiesExpression(e) => &e.expression,
            Expression::TSNonNullExpression(e) => &e.expression,
            Expression::TSTypeAssertion(e) => &e.expression,
            Expression::TSInstantiationExpression(e) => &e.expression,
            other => return other,
        };
    }
}

fn is_type_asserted(expr: &Expression<'_>) -> bool {
    let mut current = expr;
    loop {
        match current {
            Expression::ParenthesizedExpression(p) => current = &p.expression,
            Expression::TSAsExpression(_)
            | Expression::TSSatisfiesExpression(_)
            | Expression::TSNonNullExpression(_)
            | Expression::TSTypeAssertion(_)
            | Expression::TSInstantiationExpression(_) => return true,
            _ => return false,
        }
    }
}

fn ident_name<'a>(expr: &'a Expression<'_>) -> Option<&'a str> {
    match unwrap_expr(expr) {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn concrete_ty(senv: &SkeletonEnv<'_>, ty: TypeId) -> Option<TypeId> {
    match senv.types.resolve(ty) {
        Some(
            Type::Error
            | Type::Void
            | Type::Null
            | Type::Optional { .. }
            | Type::GenericParam { .. },
        )
        | None => None,
        Some(_) => Some(ty),
    }
}

fn field_ty(senv: &SkeletonEnv<'_>, owner_ty: TypeId, name: &str) -> Option<TypeId> {
    if let Some(fields) = senv.program.class_fields.get(&owner_ty)
        && let Some(field) = fields.iter().find(|f| f.name.as_str() == name)
    {
        return Some(field.ty);
    }
    struct_field_ty(senv, owner_ty, name)
}

pub(crate) fn return_hint_ty(
    senv: &SkeletonEnv<'_>,
    ret: Option<TypeId>,
    is_async: bool,
    is_generator: bool,
) -> Option<TypeId> {
    if is_generator {
        return None;
    }
    let ret = ret?;
    if !is_async {
        return Some(ret);
    }
    match senv.types.resolve(ret) {
        Some(Type::Promise { ok, .. }) => Some(*ok),
        _ => Some(ret),
    }
}

impl SkeletonBuilder {
    pub(crate) fn infer_param_hints(
        senv: &mut SkeletonEnv<'_>,
        body: Option<&FunctionBody<'_>>,
        items: &[FormalParameter<'_>],
        ret_ty: Option<TypeId>,
        self_ty: Option<TypeId>,
        expression_body: bool,
        scope: Option<&BodyScope>,
    ) -> ParamHints {
        let mut candidates: HashSet<String> = HashSet::new();
        for item in items {
            if item.type_annotation.is_none()
                && let Some(name) = binding_pattern_name(&item.pattern)
            {
                candidates.insert(name.as_str().to_owned());
            }
        }
        if candidates.is_empty() {
            return ParamHints::new();
        }
        let mut local_bindings: HashSet<String> = HashSet::new();
        for item in items {
            let mut names: Vec<Atom> = Vec::new();
            collect_binding_names(&item.pattern, &mut names);
            for name in names {
                local_bindings.insert(name.as_str().to_owned());
            }
        }
        if let Some(scope) = scope {
            for name in scope.names() {
                local_bindings.insert(name);
            }
        }
        let mut scanner = ParamScanner {
            senv,
            hints: HintSet::default(),
            shadowed: HashSet::new(),
            local_bindings,
            candidates,
            ret_ty,
            self_ty,
        };
        for item in items {
            if item.type_annotation.is_some() {
                continue;
            }
            let Some(name) = binding_pattern_name(&item.pattern) else {
                continue;
            };
            let Some(init) = item.initializer.as_deref() else {
                continue;
            };
            if let Some(ty) = scanner.literal_ty(init) {
                scanner.offer(name.as_str(), ty);
            }
        }
        if let Some(body) = body {
            scanner.shadow_declared(&body.statements);
            scanner.hoist_function_scoped_names(&body.statements);
            for stmt in &body.statements {
                scanner.scan_stmt(stmt);
            }
            if expression_body
                && let Some(Statement::ExpressionStatement(first)) = body.statements.first()
            {
                scanner.scan_returned(&first.expression);
            }
        }
        scanner.finish()
    }
}

impl ParamScanner<'_, '_> {
    fn finish(mut self) -> ParamHints {
        for name in &self.shadowed {
            self.hints.agreed.remove(name);
        }
        self.hints.agreed
    }

    fn offer(&mut self, name: &str, ty: TypeId) {
        if self.shadowed.contains(name) || !self.candidates.contains(name) {
            return;
        }
        if concrete_ty(self.senv, ty).is_none() {
            return;
        }
        self.hints.offer(name, ty);
    }

    fn shadow_name(&mut self, name: &str) {
        self.local_bindings.insert(name.to_owned());
        if self.candidates.contains(name) {
            self.shadowed.insert(name.to_owned());
        }
    }

    fn shadow_pattern(&mut self, pattern: &oxc_ast::ast::BindingPattern<'_>) {
        let mut names: Vec<Atom> = Vec::new();
        collect_binding_names(pattern, &mut names);
        for name in names {
            self.shadow_name(name.as_str());
        }
    }

    fn hoist_function_scoped_names(&mut self, stmts: &[Statement<'_>]) {
        let mut names: Vec<Atom> = Vec::new();
        collect_function_scoped_var_names(stmts, &mut names);
        for name in names {
            if !self.local_bindings.contains(name.as_str()) {
                self.shadow_name(name.as_str());
            }
        }
    }

    fn shadow_declared(&mut self, stmts: &[Statement<'_>]) {
        for stmt in stmts {
            let Some(decl) = stmt.as_declaration() else {
                continue;
            };
            match decl {
                Declaration::FunctionDeclaration(function) => {
                    if let Some(id) = &function.id {
                        self.shadow_name(id.name.as_str());
                    }
                }
                Declaration::ClassDeclaration(class) => {
                    if let Some(id) = &class.id {
                        self.shadow_name(id.name.as_str());
                    }
                }
                Declaration::VariableDeclaration(decl) => {
                    for declarator in &decl.declarations {
                        self.shadow_pattern(&declarator.id);
                    }
                }
                _ => {}
            }
        }
    }

    fn literal_ty(&mut self, expr: &Expression<'_>) -> Option<TypeId> {
        Some(match unwrap_expr(expr) {
            Expression::NumericLiteral(_) => self.senv.types.intern(&Type::F64),
            Expression::StringLiteral(_) => self.senv.types.intern(&Type::String),
            Expression::BooleanLiteral(_) => self.senv.types.intern(&Type::Bool),
            Expression::BigIntLiteral(_) => self.senv.types.intern(&Type::BigInt),
            Expression::UnaryExpression(unary)
                if unary.operator == UnaryOperator::UnaryNegation =>
            {
                match unwrap_expr(&unary.argument) {
                    Expression::NumericLiteral(_) => self.senv.types.intern(&Type::F64),
                    Expression::BigIntLiteral(_) => self.senv.types.intern(&Type::BigInt),
                    _ => return None,
                }
            }
            _ => return None,
        })
    }

    fn global_ty(&self, name: &str) -> Option<TypeId> {
        if self.local_bindings.contains(name) {
            return None;
        }
        self.senv
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                HirDecl::Global {
                    name: declared, ty, ..
                } if declared.as_str() == name => Some(*ty),
                _ => None,
            })
    }

    fn callee_param_ty(&self, name: &str, index: usize) -> Option<TypeId> {
        if self.local_bindings.contains(name) {
            return None;
        }
        if let Some(func) = self.senv.program.find_function_by_name(&Atom::from(name)) {
            let param = func.params.get(index)?;
            return concrete_ty(self.senv, param.ty);
        }
        for decl in &self.senv.program.declarations {
            if let HirDecl::Global {
                name: declared, ty, ..
            } = decl
                && declared.as_str() == name
                && let Some(Type::Fn {
                    params, optional, ..
                }) = self.senv.types.resolve(*ty)
                && !optional.get(index).copied().unwrap_or(false)
            {
                return concrete_ty(self.senv, *params.get(index)?);
            }
        }
        None
    }
}

fn call_target(callee: &Expression<'_>, shim_receiver_is_shadowed: bool) -> Option<String> {
    match unwrap_expr(callee) {
        Expression::Identifier(id) => Some(id.name.as_str().to_owned()),
        other @ match_member_expression!(Expression) => {
            let MemberExpression::StaticMemberExpression(s) = other.to_member_expression() else {
                return None;
            };
            if shim_receiver_is_shadowed {
                return None;
            }
            if !matches!(
                unwrap_expr(&s.object),
                Expression::Identifier(obj) if obj.name.as_str() == "assert"
            ) {
                return None;
            }
            ASSERT_SHIM_METHODS
                .iter()
                .find(|(method, _)| *method == s.property.name.as_str())
                .map(|(_, target)| (*target).to_owned())
        }
        _ => None,
    }
}

fn collect_var_declarator_names(decl: &VariableDeclaration<'_>, out: &mut Vec<Atom>) {
    if !matches!(decl.kind, VariableDeclarationKind::Var) {
        return;
    }
    for declarator in &decl.declarations {
        collect_binding_names(&declarator.id, out);
    }
}

fn collect_for_left_var_names(left: &oxc_ast::ast::ForStatementLeft<'_>, out: &mut Vec<Atom>) {
    if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) = left {
        collect_var_declarator_names(decl, out);
    }
}

fn collect_function_scoped_var_names(stmts: &[Statement<'_>], out: &mut Vec<Atom>) {
    for stmt in stmts {
        match stmt {
            Statement::BlockStatement(block) => {
                collect_function_scoped_var_names(&block.body, out);
            }
            Statement::IfStatement(it) => {
                collect_function_scoped_var_names(std::slice::from_ref(&it.consequent), out);
                if let Some(alternate) = &it.alternate {
                    collect_function_scoped_var_names(std::slice::from_ref(alternate), out);
                }
            }
            Statement::WhileStatement(while_stmt) => {
                collect_function_scoped_var_names(std::slice::from_ref(&while_stmt.body), out);
            }
            Statement::DoWhileStatement(do_stmt) => {
                collect_function_scoped_var_names(std::slice::from_ref(&do_stmt.body), out);
            }
            Statement::WithStatement(with_stmt) => {
                collect_function_scoped_var_names(std::slice::from_ref(&with_stmt.body), out);
            }
            Statement::ForStatement(for_stmt) => {
                if let Some(ForStatementInit::VariableDeclaration(decl)) = &for_stmt.init {
                    collect_var_declarator_names(decl, out);
                }
                collect_function_scoped_var_names(std::slice::from_ref(&for_stmt.body), out);
            }
            Statement::ForInStatement(for_stmt) => {
                collect_for_left_var_names(&for_stmt.left, out);
                collect_function_scoped_var_names(std::slice::from_ref(&for_stmt.body), out);
            }
            Statement::ForOfStatement(for_stmt) => {
                collect_for_left_var_names(&for_stmt.left, out);
                collect_function_scoped_var_names(std::slice::from_ref(&for_stmt.body), out);
            }
            Statement::SwitchStatement(switch_stmt) => {
                for case in &switch_stmt.cases {
                    collect_function_scoped_var_names(&case.consequent, out);
                }
            }
            Statement::TryStatement(try_stmt) => {
                collect_function_scoped_var_names(&try_stmt.block.body, out);
                if let Some(handler) = &try_stmt.handler {
                    collect_function_scoped_var_names(&handler.body.body, out);
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    collect_function_scoped_var_names(&finalizer.body, out);
                }
            }
            Statement::LabeledStatement(labeled) => {
                collect_function_scoped_var_names(std::slice::from_ref(&labeled.body), out);
            }
            decl @ match_declaration!(Statement) => {
                if let Declaration::VariableDeclaration(var_decl) = decl.to_declaration() {
                    collect_var_declarator_names(var_decl, out);
                }
            }
            _ => {}
        }
    }
}

impl ParamScanner<'_, '_> {
    fn scan_returned(&mut self, value: &Expression<'_>) {
        if !is_type_asserted(value)
            && let Some(name) = ident_name(value)
            && let Some(ty) = self.ret_ty
        {
            self.offer(name, ty);
        }
        self.scan_expr(value);
    }

    fn scan_member(&mut self, member: &MemberExpression<'_>) {
        match member {
            MemberExpression::StaticMemberExpression(s) => self.scan_expr(&s.object),
            MemberExpression::ComputedMemberExpression(c) => {
                self.scan_expr(&c.object);
                self.scan_expr(&c.expression);
            }
            MemberExpression::PrivateFieldExpression(p) => self.scan_expr(&p.object),
        }
    }

    fn member_read_ty(&mut self, expr: &Expression<'_>) -> Option<TypeId> {
        let other = unwrap_expr(expr);
        if !matches!(other, match_member_expression!(Expression)) {
            return None;
        }
        let MemberExpression::StaticMemberExpression(s) = other.to_member_expression() else {
            return None;
        };
        let owner_ty = match unwrap_expr(&s.object) {
            Expression::ThisExpression(_) => self.self_ty,
            Expression::Identifier(id) => self.global_ty(id.name.as_str()),
            _ => None,
        }?;
        field_ty(self.senv, owner_ty, s.property.name.as_str())
    }

    fn scan_write_member(&mut self, member: &MemberExpression<'_>, value: &Expression<'_>) {
        let MemberExpression::StaticMemberExpression(s) = member else {
            return;
        };
        if is_type_asserted(value) {
            return;
        }
        let Some(name) = ident_name(value) else {
            return;
        };
        let owner_ty = match unwrap_expr(&s.object) {
            Expression::ThisExpression(_) => self.self_ty,
            Expression::Identifier(id) => self.global_ty(id.name.as_str()),
            _ => None,
        };
        if let Some(owner_ty) = owner_ty
            && let Some(field_ty) = field_ty(self.senv, owner_ty, s.property.name.as_str())
        {
            self.offer(name, field_ty);
        }
    }

    fn scan_target_object(&mut self, target: &SimpleAssignmentTarget<'_>) {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(_) => {}
            other @ match_member_expression!(SimpleAssignmentTarget) => {
                self.scan_member(other.to_member_expression());
            }
            _ => {}
        }
    }

    fn scan_assignment(&mut self, a: &oxc_ast::ast::AssignmentExpression<'_>) {
        if a.operator == AssignmentOperator::Assign
            && let Some(target) = a.left.as_simple_assignment_target()
        {
            match target {
                SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                    let name = id.name.as_str().to_owned();
                    if let Some(ty) = self.literal_ty(&a.right) {
                        self.offer(&name, ty);
                    }
                    if let Some(ty) = self.member_read_ty(&a.right) {
                        self.offer(&name, ty);
                    }
                }
                other @ match_member_expression!(SimpleAssignmentTarget) => {
                    self.scan_write_member(other.to_member_expression(), &a.right);
                }
                _ => {}
            }
            self.scan_target_object(target);
        }
        self.scan_expr(&a.right);
    }

    fn scan_call(&mut self, call: &CallExpression<'_>) {
        let target = call_target(&call.callee, self.local_bindings.contains("assert"));
        if let Some(target) = target {
            for (index, arg) in call.arguments.iter().enumerate() {
                let arg @ match_expression!(Argument) = arg else {
                    continue;
                };
                let Some(name) = ident_name(arg.to_expression()) else {
                    continue;
                };
                if is_type_asserted(arg.to_expression()) {
                    continue;
                }
                if let Some(ty) = self.callee_param_ty(&target, index) {
                    self.offer(name, ty);
                }
            }
        }
        self.scan_expr(&call.callee);
        for arg in &call.arguments {
            if let arg @ match_expression!(Argument) = arg {
                self.scan_expr(arg.to_expression());
            }
        }
    }

    fn scan_new(&mut self, new_expr: &NewExpression<'_>) {
        self.scan_expr(&new_expr.callee);
        for arg in &new_expr.arguments {
            if let arg @ match_expression!(Argument) = arg {
                self.scan_expr(arg.to_expression());
            }
        }
    }

    fn scan_chain(&mut self, element: &ChainElement<'_>) {
        match element {
            ChainElement::CallExpression(call) => self.scan_call(call),
            ChainElement::TSNonNullExpression(e) => self.scan_expr(&e.expression),
            other @ match_member_expression!(ChainElement) => {
                self.scan_member(other.to_member_expression());
            }
        }
    }

    fn scan_expr(&mut self, expr: &Expression<'_>) {
        match expr {
            Expression::ParenthesizedExpression(p) => self.scan_expr(&p.expression),
            Expression::TSAsExpression(e) => self.scan_expr(&e.expression),
            Expression::TSSatisfiesExpression(e) => self.scan_expr(&e.expression),
            Expression::TSNonNullExpression(e) => self.scan_expr(&e.expression),
            Expression::TSTypeAssertion(e) => self.scan_expr(&e.expression),
            Expression::TSInstantiationExpression(e) => self.scan_expr(&e.expression),
            Expression::AssignmentExpression(a) => self.scan_assignment(a),
            Expression::CallExpression(call) => self.scan_call(call),
            Expression::NewExpression(new_expr) => self.scan_new(new_expr),
            Expression::BinaryExpression(b) => {
                self.scan_expr(&b.left);
                self.scan_expr(&b.right);
            }
            Expression::LogicalExpression(l) => {
                self.scan_expr(&l.left);
                self.scan_expr(&l.right);
            }
            Expression::ConditionalExpression(c) => {
                self.scan_expr(&c.test);
                self.scan_expr(&c.consequent);
                self.scan_expr(&c.alternate);
            }
            Expression::SequenceExpression(s) => {
                for item in &s.expressions {
                    self.scan_expr(item);
                }
            }
            Expression::UnaryExpression(u) => self.scan_expr(&u.argument),
            Expression::AwaitExpression(a) => self.scan_expr(&a.argument),
            Expression::YieldExpression(y) => {
                if let Some(argument) = &y.argument {
                    self.scan_expr(argument);
                }
            }
            Expression::ChainExpression(chain) => self.scan_chain(&chain.expression),
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    if let element @ match_expression!(ArrayExpressionElement) = element {
                        self.scan_expr(element.to_expression());
                    }
                }
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(p) => self.scan_expr(&p.value),
                        ObjectPropertyKind::SpreadProperty(s) => self.scan_expr(&s.argument),
                    }
                }
            }
            Expression::TemplateLiteral(t) => {
                for item in &t.expressions {
                    self.scan_expr(item);
                }
            }
            Expression::TaggedTemplateExpression(t) => {
                self.scan_expr(&t.tag);
                for item in &t.quasi.expressions {
                    self.scan_expr(item);
                }
            }
            Expression::UpdateExpression(u) => self.scan_target_object(&u.argument),
            other @ match_member_expression!(Expression) => {
                self.scan_member(other.to_member_expression());
            }
            _ => {}
        }
    }

    fn scan_var_decl(&mut self, decl: &VariableDeclaration<'_>) {
        for declarator in &decl.declarations {
            self.shadow_pattern(&declarator.id);
            if let Some(init) = &declarator.init {
                self.scan_expr(init);
            }
        }
    }

    fn scan_for_init(&mut self, init: &ForStatementInit<'_>) {
        match init {
            ForStatementInit::VariableDeclaration(decl) => self.scan_var_decl(decl),
            other @ match_expression!(ForStatementInit) => self.scan_expr(other.to_expression()),
        }
    }

    fn scan_stmt(&mut self, stmt: &Statement<'_>) {
        match stmt {
            Statement::BlockStatement(block) => {
                let prior = self.local_bindings.clone();
                let prior_shadowed = self.shadowed.clone();
                self.shadow_declared(&block.body);
                for inner in &block.body {
                    self.scan_stmt(inner);
                }
                self.local_bindings = prior;
                self.shadowed = prior_shadowed;
            }
            Statement::ExpressionStatement(e) => self.scan_expr(&e.expression),
            Statement::ReturnStatement(r) => {
                if let Some(value) = &r.argument {
                    self.scan_returned(value);
                }
            }
            Statement::IfStatement(i) => {
                self.scan_expr(&i.test);
                self.scan_stmt(&i.consequent);
                if let Some(alternate) = &i.alternate {
                    self.scan_stmt(alternate);
                }
            }
            Statement::WhileStatement(w) => {
                self.scan_expr(&w.test);
                self.scan_stmt(&w.body);
            }
            Statement::DoWhileStatement(d) => {
                self.scan_stmt(&d.body);
                self.scan_expr(&d.test);
            }
            Statement::ForStatement(f) => {
                if let Some(init) = &f.init {
                    self.scan_for_init(init);
                }
                if let Some(test) = &f.test {
                    self.scan_expr(test);
                }
                if let Some(update) = &f.update {
                    self.scan_expr(update);
                }
                self.scan_stmt(&f.body);
            }
            Statement::ForInStatement(f) => {
                self.scan_for_init_left(&f.left);
                self.scan_expr(&f.right);
                self.scan_stmt(&f.body);
            }
            Statement::ForOfStatement(f) => {
                self.scan_for_init_left(&f.left);
                self.scan_expr(&f.right);
                self.scan_stmt(&f.body);
            }
            Statement::SwitchStatement(s) => {
                let prior = self.local_bindings.clone();
                let prior_shadowed = self.shadowed.clone();
                self.scan_expr(&s.discriminant);
                for case in &s.cases {
                    self.shadow_declared(&case.consequent);
                }
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.scan_expr(test);
                    }
                    for inner in &case.consequent {
                        self.scan_stmt(inner);
                    }
                }
                self.local_bindings = prior;
                self.shadowed = prior_shadowed;
            }
            Statement::TryStatement(t) => {
                let prior = self.local_bindings.clone();
                let prior_shadowed = self.shadowed.clone();
                self.shadow_declared(&t.block.body);
                for inner in &t.block.body {
                    self.scan_stmt(inner);
                }
                if let Some(handler) = &t.handler {
                    if let Some(param) = &handler.param {
                        self.shadow_pattern(&param.pattern);
                    }
                    self.shadow_declared(&handler.body.body);
                    for inner in &handler.body.body {
                        self.scan_stmt(inner);
                    }
                }
                if let Some(finalizer) = &t.finalizer {
                    self.shadow_declared(&finalizer.body);
                    for inner in &finalizer.body {
                        self.scan_stmt(inner);
                    }
                }
                self.local_bindings = prior;
                self.shadowed = prior_shadowed;
            }
            Statement::LabeledStatement(l) => self.scan_stmt(&l.body),
            Statement::WithStatement(w) => {
                self.scan_expr(&w.object);
                self.scan_stmt(&w.body);
            }
            Statement::ThrowStatement(t) => self.scan_expr(&t.argument),
            d @ match_declaration!(Statement) => match d.to_declaration() {
                Declaration::VariableDeclaration(decl) => self.scan_var_decl(decl),
                Declaration::FunctionDeclaration(f) => {
                    if let Some(id) = &f.id {
                        self.shadow_name(id.name.as_str());
                    }
                }
                Declaration::ClassDeclaration(c) => {
                    if let Some(id) = &c.id {
                        self.shadow_name(id.name.as_str());
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn scan_for_init_left(&mut self, left: &oxc_ast::ast::ForStatementLeft<'_>) {
        if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) = left {
            self.scan_var_decl(decl);
        }
    }
}
