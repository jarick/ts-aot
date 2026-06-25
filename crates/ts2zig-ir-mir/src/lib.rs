mod decl;
mod program;

pub use decl::{
    FunctionEffects, FunctionKind, MirDecl, MirFieldDecl, MirFunctionDecl, MirGlobalDecl, MirParam,
    MirStructDecl,
};
pub use program::{MirExport, MirImport, MirProgram};
