//! Setup checks shared by `comind doctor` and the web UI's setup page.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub ok: bool,
    /// Optional extras (signing, SCIP) do not count as problems.
    pub optional: bool,
    pub message: String,
}

/// Check the index, ledger, remote access, hooks, LLM, signing and SCIP indexers for `root`.
pub fn run(cfg: &crate::config::Config, root: &str) -> Vec<Check> {
    let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty());
    let mut out = Vec::new();
    let mut check = |good: bool, optional: bool, ok: &str, bad: &str| {
        out.push(Check {
            ok: good,
            optional,
            message: if good { ok } else { bad }.to_string(),
        })
    };
    let graph = format!("{}/_graph", root.trim_end_matches('/'));
    match crate::index::graph_versions_blocking(&graph) {
        Ok(v) if !v.is_empty() => {
            let (_, _, at) = v.last().cloned().unwrap_or_default();
            let msg = format!(
                "index readable: {} version(s), latest {}",
                v.len(),
                at.get(..16).unwrap_or(&at)
            );
            check(true, false, &msg, "");
        }
        _ => check(
            false,
            false,
            "",
            "no index here yet: run `comind link <repos> --index-dir …`",
        ),
    }
    match crate::ledger::all_blocking(root) {
        Ok(r) => check(
            true,
            false,
            &format!("ledger readable: {} record(s)", r.len()),
            "",
        ),
        Err(e) => check(false, false, "", &format!("ledger not readable: {e:#}")),
    }
    if crate::cache::is_remote(root) {
        let fresh = crate::cache::dir(root).is_some_and(|d| d.join("_graph").is_dir());
        check(
            fresh,
            false,
            "local mirror present (hooks read it)",
            "no local mirror yet: `comind cache refresh` (hooks create it on first use)",
        );
        check(
            set("AWS_ACCESS_KEY_ID") || set("AWS_PROFILE") || set("AWS_WEB_IDENTITY_TOKEN_FILE"),
            false,
            "AWS credentials in the environment",
            "no AWS credentials in the environment (SSO: `aws sso login` + export)",
        );
    }
    let settings = std::env::var_os("HOME")
        .map(|h| Path::new(&h).join(".claude/settings.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let local = std::fs::read_to_string(".claude/settings.json").unwrap_or_default();
    check(
        settings.contains("comind hook") || local.contains("comind hook"),
        true,
        "Claude Code hooks configured",
        "Claude Code hooks not found: `comind init-hooks claude`",
    );
    check(
        set("OPENAI_API_KEY") || cfg.llm_base_url().is_some(),
        true,
        &format!("LLM configured (model {})", cfg.llm_model()),
        "no LLM key: enrichment and flows are off (indexing works without)",
    );
    check(
        crate::signing::signing_key().is_some(),
        true,
        "ledger records are signed",
        "records are unsigned (optional: `signing_key` or git SSH signing)",
    );
    check(
        crate::signing::allowed_signers().is_some(),
        true,
        "signatures are verified against allowed_signers",
        "no allowed_signers: signatures are not checked (optional)",
    );
    let indexers: Vec<&str> = [
        "scip-python",
        "scip-typescript",
        "scip-go",
        "rust-analyzer",
        "scip-java",
    ]
    .into_iter()
    .filter(|b| crate::scip::which(b).is_some())
    .collect();
    check(
        !indexers.is_empty(),
        true,
        &format!("SCIP indexers on PATH: {}", indexers.join(", ")),
        "no SCIP indexers on PATH (optional: compiler-grade edges)",
    );
    out
}
