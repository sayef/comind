//! Per-language syntax adapters. The generic walk in [`crate::parse`] asks each question of the
//! file's language; adding a language means implementing [`LangSpec`] for its grammar.

pub mod go;
pub mod python;
pub mod typescript;

use tree_sitter::Node;

use crate::facts::{Import, RefKind, TypeSource};
use crate::model::{Language, SymbolKind};

/// What an assignment's left-hand side names.
pub enum Target {
    /// A local or module variable.
    Var(String),
    /// An attribute on the instance (`self.x` / `this.x`).
    Attr(String),
}

pub trait LangSpec: Sync {
    /// Receiver name inside methods (`self`, `this`).
    fn receiver(&self) -> &'static str;

    /// A definition node: its kind and name. `in_class` is true directly inside a class body.
    fn def(&self, node: Node, src: &[u8], in_class: bool) -> Option<(SymbolKind, String)>;

    /// The type a definition declared outside it belongs to (Go method receivers).
    fn owner(&self, _def: Node, _src: &[u8]) -> Option<String> {
        None
    }

    /// A module-level variable binding's name (only asked at module scope).
    fn variable(&self, node: Node, src: &[u8]) -> Option<String>;

    /// `Some` for an import/re-export statement (not descended into), possibly empty.
    fn import(&self, node: Node, src: &[u8]) -> Option<Vec<Import>>;

    /// A call or construction and the dotted path of its callee.
    fn reference(&self, node: Node, src: &[u8]) -> Option<(RefKind, Vec<String>)>;

    /// An assignment (or field declaration) whose target type can be inferred.
    fn assignment(&self, node: Node, src: &[u8]) -> Option<(Target, TypeSource)>;

    /// Base classes / implemented interfaces of a class-like definition.
    fn bases(&self, def: Node, src: &[u8]) -> Vec<(RefKind, Vec<String>)>;

    /// Declared return type of a callable.
    fn returns(&self, def: Node, src: &[u8]) -> Option<Vec<String>>;

    /// Typed parameters of a callable; `true` marks constructor parameter properties (TS).
    fn params(&self, def: Node, src: &[u8]) -> Vec<(String, Vec<String>, bool)>;
}

pub fn spec(lang: &Language) -> Option<&'static dyn LangSpec> {
    match lang {
        Language::Python => Some(&python::Python),
        Language::TypeScript | Language::JavaScript => Some(&typescript::TypeScript),
        Language::Go => Some(&go::Go),
        _ => None,
    }
}

pub(crate) fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

pub(crate) fn field_text<'a>(node: Node, field: &str, src: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name(field).map(|n| text(n, src))
}

pub(crate) fn named_children(node: Node) -> Vec<Node> {
    let mut c = node.walk();
    node.named_children(&mut c).collect()
}

pub(crate) fn line(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}
