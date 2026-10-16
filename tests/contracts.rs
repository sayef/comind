//! Contracts connect services: a route handler's blast radius reaches clients in other repos.

use std::path::Path;

#[test]
fn ripple_crosses_service_boundary_through_http_contract() {
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    let mut facts = Vec::new();
    for repo in ["skills-svc", "profile-svc"] {
        let out =
            comind::parse::parse_repo(&Path::new("tests/fixtures/org").join(repo), repo).unwrap();
        symbols.extend(out.symbols);
        edges.extend(out.edges);
        facts.extend(out.facts);
    }
    let resolved = comind::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols);
    let g = comind::graph::CodeGraph::build(&symbols, &resolved.edges);

    let get_skill = g.lookup("get_skill").expect("handler indexed");
    let hops = g.ripple(get_skill, 4);
    let names: Vec<String> = hops.iter().map(|h| h.node.name.clone()).collect();
    assert!(
        names.contains(&"http:GET /v1/skills/{}".to_string()),
        "{names:?}"
    );
    assert!(
        hops.iter()
            .any(|h| h.node.name == "fetch" && h.node.repo == "profile-svc"),
        "{names:?}"
    );
    // A consumer's change doesn't break the provider.
    let fetch = g.lookup("fetch").unwrap();
    assert!(!g
        .ripple(fetch, 4)
        .iter()
        .any(|h| h.node.name == "get_skill"));
}
