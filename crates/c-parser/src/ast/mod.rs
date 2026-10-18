//! AST model: `NodeId`, `Span`, `NodeKind`, `NodeData`, `Node`, `Ast`.
//!
//! This module owns the core data types of the abstract syntax tree — the
//! 32-bit arena index [`NodeId`], the half-open byte range [`Span`], the
//! tagged union [`NodeData`], and the top-level [`Ast`] container together
//! with its invariants.

pub mod node;
pub mod node_data;
pub mod text_slice;

pub use node::{Ast, Node};
pub use node_data::NodeData;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// 32-bit index into the node arena.
///
/// Using `u32` halves the node size relative to 64-bit indices while still
/// allowing up to ~4.29 billion nodes — far beyond any realistic C code base.
pub type NodeId = u32;

// ---------------------------------------------------------------------------
// Span
// ---------------------------------------------------------------------------

/// Half-open byte range `[start, end)` into the original source buffer.
///
/// Invariants (enforced by the parser and the custom validator):
/// `start <= end` and `end <= source_length`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

// ---------------------------------------------------------------------------
// NodeKind
// ---------------------------------------------------------------------------

/// Structural kind of the AST node.
///
/// Every variant maps 1:1 to the `NodeKind` enum in `docs/ast-schema.json`
/// §$defs.  It is serialized as the JSON string (e.g. `"IfStatement"`,
/// `"BinaryExpr"`, `"TranslationUnit"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    // Structural (no payload)
    TranslationUnit,
    CompoundStatement,
    IfStatement,
    WhileStatement,
    DoWhileStatement,
    ForStatement,
    SwitchStatement,
    CaseStatement,
    DefaultStatement,
    ReturnStatement,
    BreakStatement,
    ContinueStatement,
    GotoStatement,
    LabelStatement,
    ExpressionStatement,
    NullStatement,
    ParenExpr,
    TypeSpecifier,
    PointerType,
    ArrayType,
    FunctionType,
    InitializerList,

    // Declarations
    FunctionDefinition,
    VariableDeclaration,
    ParameterDeclaration,
    TypedefDeclaration,
    StructDeclaration,
    UnionDeclaration,
    EnumDeclaration,
    Enumerator,

    // Expressions / literals
    Identifier,
    IntegerLiteral,
    FloatLiteral,
    StringLiteral,
    CharLiteral,
    BinaryExpr,
    UnaryExpr,
    AssignmentExpr,
    CallExpr,
    MemberExpr,
    IndexExpr,
    CastExpr,
    TernaryExpr,
    SizeofExpr,

    // Preprocessor directives (SingleTu mode)
    PreprocessorInclude,
    PreprocessorDefine,
    PreprocessorUndef,
    PreprocessorIf,
    PreprocessorIfdef,
    PreprocessorIfndef,
    PreprocessorElse,
    PreprocessorElif,
    PreprocessorEndif,
    PreprocessorPragma,
    PreprocessorLine,
    PreprocessorError,
    PreprocessorWarning,

    // Error recovery
    ErrorNode,
}

impl NodeKind {
    /// Returns `true` for structural node kinds that must have `data: None`
    /// (serialized as `"data": null`).
    pub fn is_structural(self) -> bool {
        matches!(
            self,
            NodeKind::TranslationUnit
                | NodeKind::CompoundStatement
                | NodeKind::IfStatement
                | NodeKind::WhileStatement
                | NodeKind::DoWhileStatement
                | NodeKind::ForStatement
                | NodeKind::SwitchStatement
                | NodeKind::CaseStatement
                | NodeKind::DefaultStatement
                | NodeKind::ReturnStatement
                | NodeKind::BreakStatement
                | NodeKind::ContinueStatement
                | NodeKind::GotoStatement
                | NodeKind::LabelStatement
                | NodeKind::ExpressionStatement
                | NodeKind::NullStatement
                | NodeKind::ParenExpr
                | NodeKind::TypeSpecifier
                | NodeKind::PointerType
                | NodeKind::ArrayType
                | NodeKind::FunctionType
                | NodeKind::InitializerList
        )
    }
}