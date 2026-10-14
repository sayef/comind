//! Per-file syntactic facts the parser extracts and the resolver binds: imports, references
//! (as dotted paths) and type hints. Scope ids are rendered `GlobalSymbolId`s.

use serde::{Deserialize, Serialize};

use crate::model::Language;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileFacts {
    pub repo: String,
    pub path: String,
    pub language: Language,
    /// Rendered id of the file symbol.
    pub file_id: String,
    /// Names other files import this one by (`acme.common.http`, `@acme/ui-kit`).
    pub modules: Vec<String>,
    pub imports: Vec<Import>,
    pub refs: Vec<Ref>,
    pub types: Vec<TypeHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    /// Python dotted module (without leading dots) or a TS module specifier.
    pub module: String,
    /// Python relative-import level (number of leading dots); 0 for absolute and TS.
    pub level: u32,
    pub items: Vec<ImportItem>,
    pub line: u32,
    /// Repo-relative file a non-relative TS specifier maps to via tsconfig `paths`/`baseUrl`.
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportItem {
    /// `from m import name as alias` / `import { name as alias } from "m"`.
    Name { name: String, alias: Option<String> },
    /// The module itself: `import a.b [as x]`, `import * as x from "m"`, `import "m"`.
    Module { alias: Option<String> },
    /// `from m import *` / `export * from "m"`.
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ref {
    pub kind: RefKind,
    /// Scope the reference occurs in (callable, class for bases, or file).
    pub from: String,
    /// Dotted path of the referenced expression, e.g. `["self", "api", "get"]`.
    pub path: Vec<String>,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefKind {
    Call,
    New,
    Inherits,
    Implements,
}

/// `name` has a type in `scope`: a parameter/variable, or `self.x`/`this.x` on a class scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeHint {
    pub scope: String,
    pub name: String,
    pub source: TypeSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeSource {
    /// Declared type path: `x: Client`.
    Annotation(Vec<String>),
    /// Assigned from a call or `new`: `x = Client(...)`.
    Construct(Vec<String>),
    /// Copied from another name in another scope: `self.api = api` (param of `__init__`).
    Alias { name: String, scope: String },
}
