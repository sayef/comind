//! Go syntax adapter (tree-sitter-go). A package is a directory; methods attach to receivers.

use tree_sitter::Node;

use super::{field_text, line, named_children, text, LangSpec, Target};
use crate::facts::{Import, ImportItem, RefKind, TypeSource};
use crate::model::SymbolKind;

pub struct Go;

impl LangSpec for Go {
    fn receiver(&self) -> &'static str {
        "self"
    }

    fn def(&self, node: Node, src: &[u8], _in_class: bool) -> Option<(SymbolKind, String)> {
        let kind = match node.kind() {
            "function_declaration" => SymbolKind::Function,
            "method_declaration" | "method_elem" => SymbolKind::Method,
            "type_spec" => match node.child_by_field_name("type")?.kind() {
                "struct_type" => SymbolKind::Struct,
                "interface_type" => SymbolKind::Interface,
                _ => SymbolKind::TypeAlias,
            },
            "type_alias" => SymbolKind::TypeAlias,
            _ => return None,
        };
        Some((kind, field_text(node, "name", src)?.to_string()))
    }

    fn owner(&self, def: Node, src: &[u8]) -> Option<String> {
        if def.kind() != "method_declaration" {
            return None;
        }
        let recv = named_children(def.child_by_field_name("receiver")?)
            .into_iter()
            .find(|p| p.kind() == "parameter_declaration")?;
        type_path(recv.child_by_field_name("type")?, src)?.pop()
    }

    fn variable(&self, node: Node, src: &[u8]) -> Option<String> {
        matches!(node.kind(), "var_spec" | "const_spec")
            .then(|| field_text(node, "name", src).map(str::to_string))?
    }

    fn import(&self, node: Node, src: &[u8]) -> Option<Vec<Import>> {
        if node.kind() != "import_declaration" {
            return None;
        }
        let mut specs = Vec::new();
        for c in named_children(node) {
            match c.kind() {
                "import_spec" => specs.push(c),
                "import_spec_list" => specs.extend(
                    named_children(c)
                        .into_iter()
                        .filter(|s| s.kind() == "import_spec"),
                ),
                _ => {}
            }
        }
        let imports = specs
            .into_iter()
            .filter_map(|s| {
                let module = field_text(s, "path", src)?
                    .trim_matches(|c| c == '"' || c == '`')
                    .to_string();
                let item = match s
                    .child_by_field_name("name")
                    .map(|n| (n.kind(), text(n, src)))
                {
                    Some(("dot", _)) => ImportItem::All,
                    Some(("blank_identifier", _)) => ImportItem::Module { alias: None },
                    Some((_, alias)) => ImportItem::Module {
                        alias: Some(alias.to_string()),
                    },
                    None => ImportItem::Module {
                        alias: Some(default_name(&module).to_string()),
                    },
                };
                Some(Import {
                    module,
                    level: 0,
                    items: vec![item],
                    line: line(s),
                    file: None,
                })
            })
            .collect();
        Some(imports)
    }

    fn reference(&self, node: Node, src: &[u8]) -> Option<(RefKind, Vec<String>)> {
        match node.kind() {
            "call_expression" => Some((
                RefKind::Call,
                path(node.child_by_field_name("function")?, src)?,
            )),
            "composite_literal" => Some((
                RefKind::New,
                type_path(node.child_by_field_name("type")?, src)?,
            )),
            _ => None,
        }
    }

    fn assignment(&self, node: Node, src: &[u8]) -> Option<(Target, TypeSource)> {
        match node.kind() {
            "short_var_declaration" => {
                let left = named_children(node.child_by_field_name("left")?)
                    .into_iter()
                    .next()?;
                if left.kind() != "identifier" {
                    return None;
                }
                let right = named_children(node.child_by_field_name("right")?)
                    .into_iter()
                    .next()?;
                Some((
                    Target::Var(text(left, src).to_string()),
                    value_type(right, src)?,
                ))
            }
            "var_spec" => {
                let name = field_text(node, "name", src)?.to_string();
                if let Some(t) = node
                    .child_by_field_name("type")
                    .and_then(|t| type_path(t, src))
                {
                    return Some((Target::Var(name), TypeSource::Annotation(t)));
                }
                let v = named_children(node.child_by_field_name("value")?)
                    .into_iter()
                    .next()?;
                Some((Target::Var(name), value_type(v, src)?))
            }
            "field_declaration" => Some((
                Target::Attr(field_text(node, "name", src)?.to_string()),
                TypeSource::Annotation(type_path(node.child_by_field_name("type")?, src)?),
            )),
            _ => None,
        }
    }

    fn bases(&self, def: Node, src: &[u8]) -> Vec<(RefKind, Vec<String>)> {
        let Some(ty) = (def.kind() == "type_spec")
            .then(|| def.child_by_field_name("type"))
            .flatten()
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut stack = vec![ty];
        while let Some(n) = stack.pop() {
            match n.kind() {
                // An embedded field (type only, no name) promotes the embedded type's methods.
                "field_declaration" if n.child_by_field_name("name").is_none() => {
                    if let Some(p) = n
                        .child_by_field_name("type")
                        .and_then(|t| type_path(t, src))
                    {
                        out.push((RefKind::Inherits, p));
                    }
                }
                "type_elem" => out.extend(
                    named_children(n)
                        .into_iter()
                        .filter_map(|t| type_path(t, src))
                        .map(|p| (RefKind::Inherits, p)),
                ),
                "struct_type" | "interface_type" | "field_declaration_list" => {
                    stack.extend(named_children(n))
                }
                _ => {}
            }
        }
        out
    }

    fn returns(&self, def: Node, src: &[u8]) -> Option<Vec<String>> {
        let r = def.child_by_field_name("result")?;
        if r.kind() == "parameter_list" {
            let first = named_children(r)
                .into_iter()
                .find(|p| p.kind() == "parameter_declaration")?;
            return type_path(first.child_by_field_name("type")?, src);
        }
        type_path(r, src)
    }

    fn params(&self, def: Node, src: &[u8]) -> Vec<(String, Vec<String>, bool)> {
        let lists = ["receiver", "parameters"]
            .into_iter()
            .filter_map(|f| def.child_by_field_name(f));
        let mut out = Vec::new();
        for list in lists {
            for p in named_children(list)
                .into_iter()
                .filter(|p| p.kind() == "parameter_declaration")
            {
                let Some(ty) = p
                    .child_by_field_name("type")
                    .and_then(|t| type_path(t, src))
                else {
                    continue;
                };
                let mut cur = p.walk();
                for n in p.children_by_field_name("name", &mut cur) {
                    out.push((text(n, src).to_string(), ty.clone(), false));
                }
            }
        }
        out
    }
}

/// Package name an import binds by default: the last path segment, skipping `/vN`.
fn default_name(module: &str) -> &str {
    let mut segs = module.rsplit('/');
    let last = segs.next().unwrap_or(module);
    let is_version =
        last.len() > 1 && last.starts_with('v') && last[1..].chars().all(|c| c.is_ascii_digit());
    let name = if is_version {
        segs.next().unwrap_or(last)
    } else {
        last
    };
    // gopkg.in/yaml.v3 → yaml
    name.split('.').next().unwrap_or(name)
}

/// Type of an initializer: a call (its return type), a composite literal, or `&T{}`.
fn value_type(v: Node, src: &[u8]) -> Option<TypeSource> {
    match v.kind() {
        "call_expression" => Some(TypeSource::Construct(path(
            v.child_by_field_name("function")?,
            src,
        )?)),
        "composite_literal" => Some(TypeSource::Construct(type_path(
            v.child_by_field_name("type")?,
            src,
        )?)),
        "unary_expression" => value_type(v.child_by_field_name("operand")?, src),
        "identifier" => Some(TypeSource::Alias {
            name: text(v, src).to_string(),
            scope: String::new(),
        }),
        _ => None,
    }
}

fn path(node: Node, src: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" | "package_identifier" => {
            Some(vec![text(node, src).to_string()])
        }
        "selector_expression" => {
            let mut p = path(node.child_by_field_name("operand")?, src)?;
            p.push(field_text(node, "field", src)?.to_string());
            Some(p)
        }
        _ => None,
    }
}

fn type_path(node: Node, src: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "qualified_type" => Some(vec![
            field_text(node, "package", src)?.to_string(),
            field_text(node, "name", src)?.to_string(),
        ]),
        "pointer_type" | "parenthesized_type" => {
            type_path(named_children(node).into_iter().next()?, src)
        }
        "generic_type" => type_path(node.child_by_field_name("type")?, src),
        _ => path(node, src),
    }
}

#[cfg(test)]
mod tests {
    use super::default_name;

    #[test]
    fn go_default_import_names() {
        assert_eq!(default_name("github.com/acme/golib/store"), "store");
        assert_eq!(default_name("github.com/jackc/pgx/v5"), "pgx");
        assert_eq!(default_name("gopkg.in/yaml.v3"), "yaml");
        assert_eq!(default_name("fmt"), "fmt");
    }
}
