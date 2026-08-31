use crate::decl::MirParam;
use ts_aot_core::{Atom, FieldId, FunctionId, LocalId, StructId, TypeId};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MirBlock {
    pub stmts: Vec<MirStmt>,
}

impl MirBlock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(stmt: MirStmt) -> Self {
        Self { stmts: vec![stmt] }
    }

    pub fn push(&mut self, stmt: MirStmt) {
        self.stmts.push(stmt);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.stmts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stmts.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MirBody {
    pub locals: Vec<MirLocalDecl>,
    pub block: MirBlock,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MirLocalDecl {
    pub id: LocalId,
    pub name: Atom,
    pub ty: TypeId,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirStmt {
    Let {
        local: LocalId,
        ty: TypeId,
        init: Option<MirExpr>,
        mutable: bool,
    },
    Assign {
        target: MirPlace,
        value: MirExpr,
    },
    Expr(MirExpr),
    Return(Option<MirExpr>),
    ReturnResultErr {
        error: MirExpr,
        err_ty: TypeId,
    },
    Throw {
        error: MirExpr,
        error_ty: TypeId,
    },
    If {
        cond: MirExpr,
        then_block: MirBlock,
        else_block: Option<MirBlock>,
    },
    While {
        cond: MirExpr,
        body: MirBlock,
    },
    DoWhile {
        body: MirBlock,
        cond: MirExpr,
    },
    ForOf {
        item: LocalId,
        iterable: MirExpr,
        iter_ty: TypeId,
        body: MirBlock,
    },
    ForAwaitOf {
        item: LocalId,
        iterable: MirExpr,
        iter_ty: TypeId,
        body: MirBlock,
    },
    ForIn {
        key: LocalId,
        object: MirExpr,
        body: MirBlock,
    },
    Break,
    Continue,
    Runtime {
        op: RuntimeOp,
        args: Vec<MirExpr>,
        dest: Option<LocalId>,
        /// Result type of the runtime call — always the type of the destination
        /// local (when `dest` is `Some`). For generic runtime ops that need a
        /// separate serde turbofish argument, store it in `target_ty` instead.
        ty: TypeId,
        /// Override for the serde turbofish argument used by generic runtime
        /// calls. `None` for all non-generic ops and for generic ops where the
        /// serde argument type equals the result type. `Some` only when the
        /// serde argument type differs from the result type (e.g. `JSON.stringify`,
        /// where the result is always `JsString` but the serde target is the
        /// value's static type, e.g. `i64`).
        target_ty: Option<TypeId>,
    },
    Switch {
        disc: Box<MirExpr>,
        cases: Vec<SwitchCase>,
        default: Option<MirBlock>,
    },
    Try {
        body: MirBlock,
        catch_param: Option<LocalId>,
        catch: Option<MirBlock>,
        finally: Option<MirBlock>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub value: ConstValue,
    pub body: MirBlock,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i128),
    String(Atom),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirPlace {
    Local {
        id: LocalId,
    },
    Field {
        base: Box<MirPlaceBase>,
        field: FieldId,
        ty: TypeId,
    },
    Index {
        base: Box<MirExpr>,
        index: Box<MirExpr>,
        ty: TypeId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirPlaceBase {
    Local(LocalId),
    Field {
        base: Box<MirPlaceBase>,
        field: FieldId,
        ty: TypeId,
    },
    Index {
        base: Box<MirExpr>,
        index: Box<MirExpr>,
        ty: TypeId,
    },
    Chain {
        base: Box<MirExpr>,
        ty: TypeId,
    },
}

impl MirPlace {
    #[must_use]
    pub fn ty(&self) -> Option<TypeId> {
        match self {
            MirPlace::Local { .. } => None,
            MirPlace::Field { ty, .. } | MirPlace::Index { ty, .. } => Some(*ty),
        }
    }
}

impl MirPlaceBase {
    #[must_use]
    pub fn ty(&self) -> Option<TypeId> {
        match self {
            MirPlaceBase::Local(_) => None,
            MirPlaceBase::Field { ty, .. }
            | MirPlaceBase::Index { ty, .. }
            | MirPlaceBase::Chain { ty, .. } => Some(*ty),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirExpr {
    Unit,
    Bool(bool),
    Int {
        value: i128,
        ty: TypeId,
    },
    Float {
        value: f64,
        ty: TypeId,
    },
    String {
        id: Atom,
        ty: TypeId,
    },
    Null {
        ty: TypeId,
    },
    Local(LocalId),
    Global(Atom),
    Field {
        base: Box<MirExpr>,
        field: FieldId,
        ty: TypeId,
    },
    Index {
        base: Box<MirExpr>,
        index: Box<MirExpr>,
        ty: TypeId,
    },
    Call {
        callee: FunctionId,
        args: Vec<MirExpr>,
        ty: TypeId,
    },
    IndirectCall {
        callee: Box<MirExpr>,
        args: Vec<MirExpr>,
        ty: TypeId,
    },
    StructLiteral {
        struct_id: StructId,
        fields: Vec<(FieldId, MirExpr)>,
        ty: TypeId,
    },
    ResultOk {
        value: Box<MirExpr>,
        ty: TypeId,
    },
    ResultErr {
        error: Box<MirExpr>,
        ty: TypeId,
    },
    Binary {
        op: BinaryOp,
        left: Box<MirExpr>,
        right: Box<MirExpr>,
        ty: TypeId,
    },
    Unary {
        op: UnaryOp,
        expr: Box<MirExpr>,
        ty: TypeId,
    },
    Await {
        expr: Box<MirExpr>,
        ty: TypeId,
    },
    Yield {
        expr: Option<Box<MirExpr>>,
        ty: TypeId,
    },
    Closure {
        params: Vec<MirParam>,
        captures: Vec<LocalId>,
        locals: Vec<MirLocalDecl>,
        body: MirBlock,
        ret_ty: TypeId,
        fn_ty: TypeId,
    },
    OptionalChain {
        base: Box<MirExpr>,
        ty: TypeId,
    },
    TypeOf {
        expr: Box<MirExpr>,
        ty: TypeId,
    },
    Cast {
        expr: Box<MirExpr>,
        ty: TypeId,
    },
    TemplateStringsArray {
        cooked: Vec<Atom>,
        ty: TypeId,
    },
    RegExp {
        pattern: String,
        flags: String,
        ty: TypeId,
    },
    BigInt {
        value: String,
        ty: TypeId,
    },
    Import {
        source: Box<MirExpr>,
        ty: TypeId,
    },
}

impl MirExpr {
    #[must_use]
    pub fn ty(&self) -> Option<TypeId> {
        match self {
            MirExpr::Unit | MirExpr::Bool(_) | MirExpr::Local(_) | MirExpr::Global(_) => None,
            MirExpr::Int { ty, .. }
            | MirExpr::Float { ty, .. }
            | MirExpr::String { ty, .. }
            | MirExpr::Null { ty }
            | MirExpr::Field { ty, .. }
            | MirExpr::Index { ty, .. }
            | MirExpr::Call { ty, .. }
            | MirExpr::IndirectCall { ty, .. }
            | MirExpr::StructLiteral { ty, .. }
            | MirExpr::ResultOk { ty, .. }
            | MirExpr::ResultErr { ty, .. }
            | MirExpr::Binary { ty, .. }
            | MirExpr::Unary { ty, .. }
            | MirExpr::Await { ty, .. }
            | MirExpr::Yield { ty, .. }
            | MirExpr::OptionalChain { ty, .. }
            | MirExpr::TypeOf { ty, .. }
            | MirExpr::Cast { ty, .. }
            | MirExpr::TemplateStringsArray { ty, .. }
            | MirExpr::RegExp { ty, .. }
            | MirExpr::BigInt { ty, .. }
            | MirExpr::Import { ty, .. } => Some(*ty),
            MirExpr::Closure { fn_ty, .. } => Some(*fn_ty),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum RuntimeOp {
    StringConcat,
    StringEquals,
    StringLen,
    StringIndexOf,
    StringCharAt,
    StringFromCharCode,
    StringFromCodePoint,
    ArrayCreate,
    ArrayCreateWithLen,
    ArrayGet,
    ArraySet,
    ArrayLen,
    ArrayPush,
    ArrayFrom,
    ArrayFromString,
    ArrayFromMapped,
    ArrayFromLengthMapped,
    MapGet,
    MapSet,
    ResultOk,
    ResultErr,
    ResultUnwrapOk,
    PromiseCreate,
    PromiseResolve,
    PromiseAll,
    PromiseRace,
    PromiseAllSettled,
    PromiseAny,
    PromiseResolveStatic,
    PromiseRejectStatic,
    PromiseThenInstance,
    PromiseCatchInstance,
    PromiseFinallyInstance,
    HostConsoleLog,
    MathSqrt,
    MathAbs,
    MathFloor,
    MathCeil,
    MathRound,
    MathTrunc,
    MathSign,
    MathPow,
    MathLog,
    MathExp,
    MathSin,
    MathCos,
    MathTan,
    MathAsin,
    MathAcos,
    MathAtan,
    MathAtan2,
    MathMax,
    MathMin,
    MathRandom,
    TypeOf,
    OpIn,
    OpInstanceof,
    ObjectKeys,
    ArrayIsArray,
    ArrayIsArrayFalse,
    DateNow,
    DateNewFromMs,
    DateParse,
    DateValueOf,
    DateGetTime,
    DateGetFullYear,
    DateGetMonth,
    DateGetDate,
    DateGetHours,
    DateGetMinutes,
    DateGetSeconds,
    DateGetMilliseconds,
    DateToIsoString,
    DateIsInvalid,
    JsonParse,
    JsonParseString,
    JsonStringify,
    JsonStringifyString,
    SymbolNew,
    SymbolFor,
    SymbolKeyFor,
    ArrayBufferNew,
    ArrayBufferSlice,
    TypedArrayNew,
    WeakMapNew,
    WeakMapSet,
    WeakMapGet,
    WeakMapHas,
    WeakMapDelete,
    WeakMapClear,
    ArrayGetOrDefault,
    ArrayConcat,
    ArrayHole,
    GeneratorNext,
}

impl RuntimeOp {
    #[must_use]
    pub fn mutably_borrowed_arg_index(&self) -> Option<usize> {
        match self {
            RuntimeOp::ArrayPush
            | RuntimeOp::ArraySet
            | RuntimeOp::MapSet
            | RuntimeOp::WeakMapSet
            | RuntimeOp::GeneratorNext => Some(0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
