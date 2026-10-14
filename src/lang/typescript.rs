//! TypeScript / JavaScript syntax adapter (tree-sitter-typescript, TS and TSX grammars).

use tree_sitter::Node;

use super::{field_text, line, named_children, text, LangSpec, Target};
use crate::facts::{Import, ImportItem, RefKind, TypeSource};
use crate::model::SymbolKind;

pub struct TypeScript;

impl LangSpec for TypeScript {
    fn receiver(&self) -> &'static str {
        "this"
    }

    fn def(&self, node: Node, src: &[u8], _in_class: bool) -> Option<(SymbolKind, String)> {
        let kind = match node.kind() {
            "function_declaration" | "generator_function_declaration" => SymbolKind::Function,
            "method_definition" => SymbolKind::Method,
            "class_declaration" | "abstract_class_declaration" => SymbolKind::Class,
            "interface_declaration" => SymbolKind::Interface,
            "type_alias_declaration" => SymbolKind::TypeAlias,
            "enum_declaration" => SymbolKind::Enum,
            // const f = () => … / function () {…}
            "variable_declarator"
                if node.child_by_field_name("value").is_some_and(|v| {
                    matches!(
                        v.kind(),
                        "arrow_function" | "function_expression" | "function"
                    )
                }) =>
            {
                SymbolKind::Function
            }
            _ => return None,
        };
        let name = node.child_by_field_name("name")?;
        (name.kind() != "object_pattern" && name.kind() != "array_pattern")
            .then(|| (kind, text(name, src).to_string()))
    }

    fn variable(&self, node: Node, src: &[u8]) -> Option<String> {
        let name =
            (node.kind() == "variable_declarator").then(|| node.child_by_field_name("name"))??;
        (name.kind() == "identifier").then(|| text(name, src).to_string())
    }

    fn import(&self, node: Node, src: &[u8]) -> Option<Vec<Import>> {
        if !matches!(node.kind(), "import_statement" | "export_statement") {
            return None;
        }
        // `export function f` etc. have no source: not an import, keep walking.
        let source = node.child_by_field_name("source")?;
        let module = text(source, src)
            .trim_matches(|c| c == '"' || c == '\'' || c == '`')
            .to_string();
        let mut items = Vec::new();
        for c in named_children(node) {
            match c.kind() {
                "import_clause" => {
                    for k in named_children(c) {
                        match k.kind() {
                            "identifier" => items.push(ImportItem::Name {
                                name: "default".into(),
                                alias: Some(text(k, src).to_string()),
                            }),
                            "named_imports" => items.extend(specifiers(k, src)),
                            "namespace_import" => items.push(ImportItem::Module {
                                alias: named_children(k).first().map(|i| text(*i, src).to_string()),
                            }),
                            _ => {}
                        }
                    }
                }
                "export_clause" => items.extend(specifiers(c, src)),
                "namespace_export" => items.push(ImportItem::Module {
                    alias: named_children(c).first().map(|i| text(*i, src).to_string()),
                }),
                _ => {}
            }
        }
        if node.kind() == "export_statement" && items.is_empty() {
            items.push(ImportItem::All); // export * from "m"
        }
        if items.is_empty() {
            items.push(ImportItem::Module { alias: None }); // import "m"
        }
        Some(vec![Import {
            module,
            level: 0,
            items,
            line: line(node),
            file: None,
        }])
    }

    fn reference(&self, node: Node, src: &[u8]) -> Option<(RefKind, Vec<String>)> {
        match node.kind() {
            "call_expression" => {
                let f = node.child_by_field_name("function")?;
                (!matches!(f.kind(), "import" | "super"))
                    .then(|| path(f, src))
                    .flatten()
                    .map(|p| (RefKind::Call, p))
            }
            "new_expression" => Some((
                RefKind::New,
                path(node.child_by_field_name("constructor")?, src)?,
            )),
            // <Button …> renders a component; lowercase tags are DOM elements.
            "jsx_opening_element" | "jsx_self_closing_element" => {
                let p = path(node.child_by_field_name("name")?, src)?;
                p.last()?
                    .starts_with(|c: char| c.is_ascii_uppercase())
                    .then_some((RefKind::Call, p))
            }
            _ => None,
        }
    }

    fn assignment(&self, node: Node, src: &[u8]) -> Option<(Target, TypeSource)> {
        let (target, ty, value) = match node.kind() {
            "variable_declarator" => {
                let name = node.child_by_field_name("name")?;
                if name.kind() != "identifier" {
                    return None;
                }
                (
                    Target::Var(text(name, src).to_string()),
                    node.child_by_field_name("type"),
                    node.child_by_field_name("value"),
                )
            }
            "public_field_definition" => (
                Target::Attr(field_text(node, "name", src)?.to_string()),
                node.child_by_field_name("type"),
                node.child_by_field_name("value"),
            ),
            "assignment_expression" => {
                let left = node.child_by_field_name("left")?;
                if left.kind() != "member_expression"
                    || left.child_by_field_name("object")?.kind() != "this"
                {
                    return None;
                }
                (
                    Target::Attr(field_text(left, "property", src)?.to_string()),
                    None,
                    node.child_by_field_name("right"),
                )
            }
            _ => return None,
        };
        if let Some(t) = ty.and_then(|t| type_path(t, src)) {
            return Some((target, TypeSource::Annotation(t)));
        }
        let mut v = value?;
        if v.kind() == "await_expression" {
            v = named_children(v).into_iter().next()?;
        }
        let source = match v.kind() {
            "new_expression" => {
                TypeSource::Construct(path(v.child_by_field_name("constructor")?, src)?)
            }
            "call_expression" => {
                TypeSource::Construct(path(v.child_by_field_name("function")?, src)?)
            }
            "identifier" => TypeSource::Alias {
                name: text(v, src).to_string(),
                scope: String::new(),
            },
            _ => return None,
        };
        Some((target, source))
    }

    fn bases(&self, def: Node, src: &[u8]) -> Vec<(RefKind, Vec<String>)> {
        let mut out = Vec::new();
        for c in named_children(def) {
            match c.kind() {
                "class_heritage" => {
                    for h in named_children(c) {
                        let kind = match h.kind() {
                            "extends_clause" => RefKind::Inherits,
                            "implements_clause" => RefKind::Implements,
                            _ => continue,
                        };
                        for t in named_children(h) {
                            if let Some(p) = type_path(t, src) {
                                out.push((kind, p));
                            }
                        }
                    }
                }
                "extends_type_clause" => {
                    for t in named_children(c) {
                        if let Some(p) = type_path(t, src) {
                            out.push((RefKind::Inherits, p));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn returns(&self, def: Node, src: &[u8]) -> Option<Vec<String>> {
        let r = def.child_by_field_name("return_type").or_else(|| {
            def.child_by_field_name("value")
                .and_then(|v| v.child_by_field_name("return_type"))
        })?;
        type_path(r, src)
    }

    fn params(&self, def: Node, src: &[u8]) -> Vec<(String, Vec<String>, bool)> {
        let ps = def.child_by_field_name("parameters").or_else(|| {
            def.child_by_field_name("value")
                .and_then(|v| v.child_by_field_name("parameters"))
        });
        let Some(ps) = ps else {
            return Vec::new();
        };
        named_children(ps)
            .into_iter()
            .filter(|p| matches!(p.kind(), "required_parameter" | "optional_parameter"))
            .filter_map(|p| {
                let name = p.child_by_field_name("pattern")?;
                if name.kind() != "identifier" {
                    return None;
                }
                let ty = type_path(p.child_by_field_name("type")?, src)?;
                let property = named_children(p)
                    .iter()
                    .any(|c| matches!(c.kind(), "accessibility_modifier" | "override_modifier"))
                    || (0..p.child_count())
                        .filter_map(|i| p.child(i))
                        .any(|c| c.kind() == "readonly");
                Some((text(name, src).to_string(), ty, property))
            })
            .collect()
    }
}

fn specifiers(list: Node, src: &[u8]) -> Vec<ImportItem> {
    named_children(list)
        .into_iter()
        .filter(|s| matches!(s.kind(), "import_specifier" | "export_specifier"))
        .filter_map(|s| {
            Some(ImportItem::Name {
                name: field_text(s, "name", src)?.to_string(),
                alias: field_text(s, "alias", src).map(str::to_string),
            })
        })
        .collect()
}

/// Dotted path of an identifier/member chain; `None` for anything dynamic.
fn path(node: Node, src: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "identifier" | "type_identifier" | "property_identifier" => {
            Some(vec![text(node, src).to_string()])
        }
        "this" => Some(vec!["this".into()]),
        "member_expression" => {
            let mut p = path(node.child_by_field_name("object")?, src)?;
            p.push(field_text(node, "property", src)?.to_string());
            Some(p)
        }
        "nested_type_identifier" => {
            let mut p = path(node.child_by_field_name("module")?, src)?;
            p.push(field_text(node, "name", src)?.to_string());
            Some(p)
        }
        "generic_type" => path(node.child_by_field_name("name")?, src),
        "non_null_expression" => path(named_children(node).into_iter().next()?, src),
        _ => None,
    }
}

/// Path of a type (or `: type` annotation), taking the first named member of a union.
fn type_path(node: Node, src: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "type_annotation" => type_path(named_children(node).into_iter().next()?, src),
        "union_type" => named_children(node)
            .into_iter()
            .find_map(|t| type_path(t, src)),
        "predefined_type" | "literal_type" => None,
        _ => path(node, src),
    }
}
