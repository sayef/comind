//! Code-aware ranking signals for hybrid search.
//!
//! Ideas borrowed from semble (re-implemented, not copied): adaptive semantic/lexical
//! weighting by query type, identifier sub-token splitting, definition/exact-name boosts,
//! and structured noise penalties. comind adds a dependency-graph centrality signal on top
//! (applied by the caller).

use std::collections::BTreeSet;

/// Split an identifier into lowercased sub-tokens, camelCase + snake_case aware, keeping the
/// compound form. `getHTTPResponse` -> {gethttpresponse, get, http, response};
/// `my_func` -> {my_func, my, func}.
pub fn split_identifier(ident: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![ident.to_lowercase()];
    // snake_case
    let snake: Vec<&str> = ident.split('_').filter(|s| !s.is_empty()).collect();
    let mut parts: Vec<String> = Vec::new();
    for seg in snake {
        parts.extend(split_camel(seg));
    }
    for p in parts {
        let p = p.to_lowercase();
        if p.len() >= 2 && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Split a camelCase/PascalCase run into words: `getHTTPResponse` -> [get, HTTP, Response].
fn split_camel(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut start = 0;
    for i in 1..chars.len() {
        let (prev, cur) = (chars[i - 1], chars[i]);
        // boundary: lower->UPPER, or UPPER->UPPER followed by lower (acronym end), or letter<->digit
        let boundary = (prev.is_lowercase() && cur.is_uppercase())
            || (prev.is_uppercase()
                && cur.is_uppercase()
                && i + 1 < chars.len()
                && chars[i + 1].is_lowercase())
            || (prev.is_alphabetic() != cur.is_alphabetic());
        if boundary {
            words.push(chars[start..i].iter().collect());
            start = i;
        }
    }
    if start < chars.len() {
        words.push(chars[start..].iter().collect());
    }
    words
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "in", "and", "or", "for", "is", "how", "do", "does", "get",
    "that", "this", "with", "on", "by",
];

/// Extract meaningful lowercased query keywords (>2 chars, non-stopword).
pub fn query_keywords(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .flat_map(|w| split_identifier(w))
        .filter(|w| w.len() > 2 && !STOPWORDS.contains(&w.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Symbol-like query? (identifier path, snake/camel case, punctuation, or a single word)
/// Such queries weight lexical matching higher (α smaller).
pub fn is_symbol_query(query: &str) -> bool {
    let q = query.trim();
    if q.contains("::") || q.contains('.') || q.contains('_') || q.contains('/') {
        return true;
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.len() == 1 {
        let w = words[0];
        // camelCase / PascalCase / has an uppercase interior letter
        return w.chars().any(|c| c.is_uppercase()) || w.len() <= 24;
    }
    false
}

/// Weight on the *semantic* signal (the rest goes to lexical). Symbol queries favor lexical.
pub fn resolve_alpha(query: &str) -> f32 {
    if is_symbol_query(query) {
        0.3
    } else {
        0.5
    }
}

/// Lexical overlap in `[0,1]`: fraction of query keywords matching the symbol's identifier
/// sub-tokens (prefix match allowed for stems ≥4 chars).
pub fn lexical_score(keywords: &[String], name: &str, descriptor_tail: &str) -> f32 {
    if keywords.is_empty() {
        return 0.0;
    }
    let mut toks: BTreeSet<String> = BTreeSet::new();
    toks.extend(split_identifier(name));
    toks.extend(split_identifier(descriptor_tail));
    let hits = keywords
        .iter()
        .filter(|k| {
            toks.contains(*k) || (k.len() >= 4 && toks.iter().any(|t| t.starts_with(k.as_str())))
        })
        .count();
    hits as f32 / keywords.len() as f32
}

/// Path-based noise penalty multiplier (semble's structured penalties).
pub fn path_penalty(path: &str) -> f32 {
    let p = path.to_lowercase();
    let is_test = p.contains("/test")
        || p.contains("test_")
        || p.contains("_test.")
        || p.contains(".test.")
        || p.contains(".spec.")
        || p.contains("/tests/")
        || p.contains("/spec/")
        || p.contains("__tests__");
    if is_test
        || p.contains("/examples")
        || p.contains("/legacy")
        || p.contains("/compat")
        || p.contains("/migration")
        || p.contains("/revision")
    {
        return 0.3;
    }
    if p.ends_with("/__init__.py") || p.ends_with("package-info.java") || p.ends_with("mod.rs") {
        return 0.5; // re-export barrels
    }
    if p.ends_with(".d.ts") {
        return 0.7;
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_identifiers() {
        let t = split_identifier("AsyncPostgresQueryExecutor");
        assert!(t.contains(&"async".to_string()));
        assert!(t.contains(&"postgres".to_string()));
        assert!(t.contains(&"executor".to_string()));
        let s = split_identifier("get_user_id");
        assert!(s.contains(&"user".to_string()) && s.contains(&"get_user_id".to_string()));
    }

    #[test]
    fn detects_symbol_queries() {
        assert!(is_symbol_query("AsyncPostgresQueryExecutor"));
        assert!(is_symbol_query("cobrainer.const.NamedOwner"));
        assert!(is_symbol_query("get_config"));
        assert!(!is_symbol_query("how do we connect to the database"));
    }

    #[test]
    fn lexical_and_penalty() {
        let kw = query_keywords("postgres executor");
        assert!(lexical_score(&kw, "AsyncPostgresQueryExecutor", "cobrainer/database/executors") > 0.9);
        assert_eq!(path_penalty("app/tests/test_api.py"), 0.3);
        assert_eq!(path_penalty("cobrainer/db.py"), 1.0);
    }
}
