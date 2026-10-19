//! [`NodeData`] — the tagged union of AST node payloads.
//!
//! The enum variants map 1:1 to the `Data*` schemas in
//! `docs/ast-schema.json` §$defs.  The `#[serde(tag = "type")]` attribute
//! produces `{"type": "Identifier", "name": "x"}` as required by the schema.
//!
//! ## Shortened discriminators
//!
//! Several kinds use a shortened `data.type` (see `ast-schema.json`
//! `NodeKind` description):
//!
//! | `kind` | `data.type` |
//! |---|---|
//! | `FunctionDefinition` | `FunctionDef` |
//! | `VariableDeclaration` | `VariableDecl` |
//! | `ParameterDeclaration` | `ParameterDecl` |
//! | `TypedefDeclaration` | `TypedefDecl` |
//! | `StructDeclaration` | `StructDef` |
//! | `UnionDeclaration` | `UnionDef` |
//! | `EnumDeclaration` | `EnumDef` |
//!
//! The `#[serde(rename = "...")]` attributes below implement this mapping.

use crate::ast::text_slice::TextSlice;
use serde::Deserialize;

/// Tagged union of AST node payloads.
///
/// The `#[serde(tag = "type")]` attribute ensures the JSON representation
/// uses a `"type"` field as the discriminator, per `ast-schema.json`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum NodeData {
    // ---- Leaf / expression payloads ----
    #[serde(rename = "Identifier")]
    Identifier { name: TextSlice },
    #[serde(rename = "IntegerLiteral")]
    IntegerLiteral {
        value: TextSlice,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        suffix: Option<TextSlice>,
    },
    #[serde(rename = "FloatLiteral")]
    FloatLiteral {
        value: TextSlice,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        suffix: Option<TextSlice>,
    },
    #[serde(rename = "StringLiteral")]
    StringLiteral {
        value: TextSlice,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<TextSlice>,
    },
    #[serde(rename = "CharLiteral")]
    CharLiteral {
        value: TextSlice,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<TextSlice>,
    },
    #[serde(rename = "BinaryExpr")]
    BinaryExpr { op: TextSlice },
    #[serde(rename = "UnaryExpr")]
    UnaryExpr { op: TextSlice, prefix: bool },
    #[serde(rename = "AssignmentExpr")]
    AssignmentExpr { op: TextSlice },
    #[serde(rename = "CallExpr")]
    CallExpr {
        /// Tri-state: `None` = unknown / not distinguished (MVP).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_macro: Option<bool>,
    },
    #[serde(rename = "MemberExpr")]
    MemberExpr { op: TextSlice },
    #[serde(rename = "IndexExpr")]
    IndexExpr,
    #[serde(rename = "CastExpr")]
    CastExpr,
    #[serde(rename = "TernaryExpr")]
    TernaryExpr,
    #[serde(rename = "SizeofExpr")]
    SizeofExpr { is_variadic: bool },

    // ---- Declarations (shortened discriminators) ----
    #[serde(rename = "FunctionDef")]
    FunctionDef {
        name: TextSlice,
        storage_class: TextSlice,
    },
    #[serde(rename = "VariableDecl")]
    VariableDecl {
        name: TextSlice,
        storage_class: TextSlice,
    },
    #[serde(rename = "ParameterDecl")]
    ParameterDecl { name: TextSlice },
    #[serde(rename = "TypedefDecl")]
    TypedefDecl { name: TextSlice },
    #[serde(rename = "StructDef")]
    StructDef {
        /// `None` for anonymous structs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<TextSlice>,
    },
    #[serde(rename = "UnionDef")]
    UnionDef {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<TextSlice>,
    },
    #[serde(rename = "EnumDef")]
    EnumDef {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<TextSlice>,
    },
    #[serde(rename = "Enumerator")]
    Enumerator {
        name: TextSlice,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<TextSlice>,
    },

    // ---- Preprocessor ----
    /// All `Preprocessor*` kinds share this payload.
    #[serde(rename = "PreprocessorDirective")]
    PreprocessorDirective {
        /// Normalized directive name (e.g. `"include"`, `"define"`) — not
        /// from source.
        directive: String,
        /// Raw argument text (always present; `""` for directives without
        /// arguments).  See `ast-schema.json` `DataPreprocessorDirective`.
        argument: TextSlice,
    },

    // ---- Error ----
    #[serde(rename = "ErrorNode")]
    ErrorNode { error_index: u32 },
}
