//! Contracts connect services: a route handler's blast radius reaches clients in other repos.

use std::path::Path;

#[test]
fn ripple_crosses_service_boundary_through_http_contract() {
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    let mut facts = Vec::new();
    for repo in ["courses-svc", "profile-svc"] {
        let out =
            comindly::parse::parse_repo(&Path::new("tests/fixtures/org").join(repo), repo).unwrap();
        symbols.extend(out.symbols);
        edges.extend(out.edges);
        facts.extend(out.facts);
    }
    let resolved = comindly::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols);
    let g = comindly::graph::CodeGraph::build(&symbols, &resolved.edges);

    let get_course = g.lookup("get_course").expect("handler indexed");
    let hops = g.ripple(get_course, 4);
    let names: Vec<String> = hops.iter().map(|h| h.node.name.clone()).collect();
    assert!(
        names.contains(&"http:GET /v1/courses/{}".to_string()),
        "{names:?}"
    );
    assert!(
        hops.iter()
            .any(|h| h.node.name == "CoursesClient.fetch" && h.node.repo == "profile-svc"),
        "{names:?}"
    );
    // A consumer's change doesn't break the provider.
    let fetch = g
        .lookup("CoursesClient.fetch")
        .expect("qualified names resolve");
    assert!(!g
        .ripple(fetch, 4)
        .iter()
        .any(|h| h.node.name == "get_course"));
}
