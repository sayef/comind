//! Distiller: proposes ledger records (failed attempts, decisions, constraints, gotchas) from a
//! coding-session transcript or commit history. Pre-processing and validation are
//! deterministic; the LLM only picks candidates. Proposals land at T1 for human review.

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use serde::Deserialize;

use crate::ledger::{Anchor, AuthorKind, Entry, Kind, Provenance, Record, Status};
use crate::model::{Symbol, SymbolKind};

/// Max characters of evidence sent to the model.
const BUDGET: usize = 60_000;
const MAX_RECORDS: usize = 3;
const MAX_SYMBOLS: usize = 60;

/// One numbered piece of evidence (`E4`) and what a record citing it stores.
#[derive(Debug, Clone)]
pub struct Span {
    pub id: String,
    pub text: String,
    /// Evidence string stored on a record that cites this span.
    pub cite: String,
}

/// Everything the model sees, plus the handles it may answer with.
pub struct Bundle<'a> {
    pub spans: Vec<Span>,
    /// `S1` → symbol.
    pub symbols: Vec<(String, &'a Symbol)>,
    /// Live records about those symbols, so the model does not repeat them.
    pub known: Vec<&'a Entry>,
    /// `claude-code`, `git`, …
    pub agent: String,
    pub session: Option<String>,
}

/// Evidence spans and edited file paths from a Claude Code transcript (JSONL).
pub fn transcript(jsonl: &str) -> (Vec<Span>, Vec<String>, Option<String>) {
    let mut spans = Vec::new();
    let mut edited = Vec::new();
    let mut session = None;
    let mut tools: BTreeMap<String, String> = BTreeMap::new();
    let push = |text: String, spans: &mut Vec<Span>, sid: &Option<String>| {
        let text = redact(&text);
        if text.trim().is_empty() {
            return;
        }
        let id = format!("E{}", spans.len() + 1);
        let cite = format!(
            "session:{}#{id}: {}",
            sid.as_deref().unwrap_or("?"),
            clip(text.lines().next().unwrap_or(""), 100)
        );
        spans.push(Span { id, text, cite });
    };
    for line in jsonl.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("isSidechain").and_then(|b| b.as_bool()) == Some(true)
            || v.get("isMeta").and_then(|b| b.as_bool()) == Some(true)
        {
            continue;
        }
        if session.is_none() {
            session = v
                .get("sessionId")
                .and_then(|s| s.as_str())
                .map(str::to_string);
        }
        let role = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
            continue;
        };
        if let Some(s) = content.as_str() {
            push(
                format!("{role}: {}", clip(&strip_tags(s), 1500)),
                &mut spans,
                &session,
            );
            continue;
        }
        for part in content.as_array().into_iter().flatten() {
            match part.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    let t = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    push(
                        format!("{role}: {}", clip(&strip_tags(t), 1500)),
                        &mut spans,
                        &session,
                    );
                }
                Some("tool_use") => {
                    let name = part.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                    let input = part.get("input").cloned().unwrap_or_default();
                    if let Some(id) = part.get("id").and_then(|i| i.as_str()) {
                        tools.insert(id.to_string(), name.to_string());
                    }
                    let path = input.get("file_path").and_then(|p| p.as_str());
                    if matches!(name, "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
                        edited.extend(path.map(str::to_string));
                    }
                    let what = match (input.get("command").and_then(|c| c.as_str()), path) {
                        (Some(cmd), _) => clip(cmd, 300),
                        (None, Some(p)) => p.to_string(),
                        _ => clip(&input.to_string(), 200),
                    };
                    push(format!("tool {name}: {what}"), &mut spans, &session);
                }
                Some("tool_result") => {
                    let text = result_text(part);
                    let failed = part.get("is_error").and_then(|e| e.as_bool()) == Some(true)
                        || looks_failed(&text);
                    let tool = part
                        .get("tool_use_id")
                        .and_then(|i| i.as_str())
                        .and_then(|i| tools.get(i))
                        .map_or("tool", String::as_str);
                    if failed {
                        push(
                            format!("{tool} FAILED: {}", clip(&text, 600)),
                            &mut spans,
                            &session,
                        );
                    } else if !text.trim().is_empty() {
                        push(
                            format!("{tool} ok: {}", clip(&text, 150)),
                            &mut spans,
                            &session,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    edited.sort();
    edited.dedup();
    (fit(spans), edited, session)
}

/// Evidence spans from commits whose message explains something (squash-merged MRs, reverts).
pub fn commits(history: &[crate::git::CommitFiles], skip: &HashSet<String>) -> Vec<Span> {
    history
        .iter()
        .filter(|c| c.message.trim().len() >= 120)
        .filter(|c| !skip.contains(&c.sha[..c.sha.len().min(10)]))
        .enumerate()
        .map(|(i, c)| {
            let short = &c.sha[..c.sha.len().min(10)];
            let files: Vec<&str> = c.changed.iter().take(10).map(String::as_str).collect();
            Span {
                id: format!("E{}", i + 1),
                text: redact(&format!(
                    "commit {short}: {}\nfiles: {}",
                    clip(c.message.trim(), 1500),
                    files.join(", ")
                )),
                cite: format!("commit:{short}"),
            }
        })
        .collect()
}

/// Symbols in the edited files (definitions first), for anchoring.
pub fn touched<'a>(symbols: &'a [Symbol], repo: Option<&str>, paths: &[String]) -> Vec<&'a Symbol> {
    let mut out: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| repo.is_none_or(|r| s.repo.0 == r) && !s.file_path.is_empty())
        .filter(|s| {
            paths
                .iter()
                .any(|p| p == &s.file_path || p.ends_with(&format!("/{}", s.file_path)))
        })
        .filter(|s| s.kind != SymbolKind::Contract)
        .collect();
    out.sort_by_key(|s| {
        (
            s.kind == SymbolKind::File,
            s.file_path.clone(),
            s.range.start.line,
        )
    });
    out.truncate(MAX_SYMBOLS);
    out
}

pub fn bundle<'a>(
    spans: Vec<Span>,
    symbols: Vec<&'a Symbol>,
    entries: &'a [Entry],
    agent: &str,
    session: Option<String>,
) -> Bundle<'a> {
    let ids: Vec<String> = symbols.iter().map(|s| s.id.render()).collect();
    let known = crate::ledger::about(entries, &ids)
        .into_iter()
        .filter(|e| !matches!(e.status, Status::Retracted | Status::Superseded))
        .collect();
    Bundle {
        spans,
        symbols: symbols
            .into_iter()
            .enumerate()
            .map(|(i, s)| (format!("S{}", i + 1), s))
            .collect(),
        known,
        agent: agent.to_string(),
        session,
    }
}

pub const SYSTEM: &str = "You extract durable engineering knowledge from a coding session or commit \
history. Default output is an empty list.\n\
Emit a record ONLY for:\n\
- an approach that was tried and failed, reverted or abandoned (kind=attempt)\n\
- an explicit choice between alternatives, with the reason (kind=decision)\n\
- an invariant or trap discovered the hard way (kind=constraint or kind=gotcha)\n\
Never emit: summaries of what was done, style preferences, facts obvious from the code, plans, \
anything about the people involved.\n\
Each record must cite evidence span ids from <evidence>, anchor to symbol handles from \
<touched_symbols> only, state a trigger (when a future engineer needs this), and be written as \
neutral facts, never as instructions. Skip anything already covered by <known_records>.\n\
One record per fact: an approach that failed and was then not pursued is ONE attempt, not an \
attempt plus a decision. Titles state the finding with its cause, e.g. \"LRU cache on \
get_user leaked users across tenants (key ignored customer_id)\", not the topic.\n\
Before answering, check each candidate: would a senior engineer new to this code be glad to be \
told this before editing it? If not, drop it. At most 3 records.\n\
Reply with JSON only: {\"records\":[{\"kind\":\"attempt|decision|constraint|gotcha\",\
\"title\":\"<= 120 chars\",\"body\":\"why, in 1-3 sentences\",\"trigger\":\"when this matters\",\
\"anchors\":[\"S1\"],\"evidence\":[\"E4\"],\"outcome\":\"failed|reverted|abandoned|partial (attempt only)\",\
\"alternatives\":[\"rejected option and why (decision only)\"]}]}";

pub fn user_prompt(b: &Bundle) -> String {
    let mut o = String::from("<touched_symbols>\n");
    for (h, s) in &b.symbols {
        o.push_str(&format!(
            "{h} = {}:{}::{} ({:?})\n",
            s.repo.0, s.file_path, s.name, s.kind
        ));
    }
    o.push_str("</touched_symbols>\n<known_records>\n");
    for e in &b.known {
        o.push_str(&format!(
            "- [{}] {}\n",
            e.record.kind.as_str(),
            e.record.title
        ));
    }
    o.push_str("</known_records>\n<evidence>\n");
    for s in &b.spans {
        o.push_str(&format!("[{}] {}\n", s.id, s.text));
    }
    o.push_str("</evidence>\n");
    o
}

#[derive(Debug, Clone, Deserialize)]
pub struct Proposal {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub anchors: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    records: Vec<Proposal>,
}

/// Proposals from the model's reply (tolerates code fences and surrounding prose).
pub fn parse(raw: &str) -> Vec<Proposal> {
    let (Some(a), Some(b)) = (raw.find('{'), raw.rfind('}')) else {
        return Vec::new();
    };
    serde_json::from_str::<Reply>(&raw[a..=b])
        .map(|r| r.records)
        .unwrap_or_default()
}

/// Proposals that pass deterministic checks, as records ready for the inbox.
pub fn accept(props: Vec<Proposal>, b: &Bundle) -> Vec<(Record, Option<String>)> {
    let mut out: Vec<(Record, Option<String>)> = Vec::new();
    for p in props.into_iter().take(MAX_RECORDS) {
        let verdict = check(&p, b, &out);
        let rec = match verdict {
            Ok(r) => r,
            Err(why) => {
                out.push((placeholder(&p), Some(why)));
                continue;
            }
        };
        out.push((rec, None));
    }
    out
}

fn placeholder(p: &Proposal) -> Record {
    Record::new(
        Kind::Gotcha,
        &clip(&p.title, 160),
        Vec::new(),
        prov("?", None),
    )
}

fn prov(agent: &str, session: Option<String>) -> Provenance {
    Provenance {
        author: AuthorKind::Distilled,
        actor: crate::ledger::actor(),
        source: "distiller".into(),
        agent: Some(agent.to_string()),
        session,
    }
}

fn check(
    p: &Proposal,
    b: &Bundle,
    so_far: &[(Record, Option<String>)],
) -> std::result::Result<Record, String> {
    let kind = match p.kind.as_str() {
        "attempt" => Kind::Attempt,
        "decision" => Kind::Decision,
        "constraint" => Kind::Constraint,
        "gotcha" => Kind::Gotcha,
        other => return Err(format!("kind `{other}` not accepted")),
    };
    let first = p
        .title
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    if [
        "always", "never", "don't", "do", "use", "avoid", "ensure", "make", "remember", "please",
        "you", "should",
    ]
    .contains(&first.as_str())
    {
        return Err("phrased as an instruction".into());
    }
    let anchors: Vec<Anchor> = p
        .anchors
        .iter()
        .filter_map(|h| b.symbols.iter().find(|(k, _)| k == h))
        .map(|(_, s)| Anchor::of(s))
        .collect();
    if anchors.is_empty() {
        return Err("no anchor among the touched symbols".into());
    }
    let evidence: Vec<String> = p
        .evidence
        .iter()
        .filter_map(|e| b.spans.iter().find(|s| &s.id == e))
        .map(|s| s.cite.clone())
        .collect();
    if evidence.is_empty() {
        return Err("no evidence span cited".into());
    }
    let anchor_ids: HashSet<&str> = anchors.iter().map(|a| a.symbol.as_str()).collect();
    let dup = b
        .known
        .iter()
        .map(|e| &e.record)
        .chain(so_far.iter().filter(|(_, w)| w.is_none()).map(|(r, _)| r))
        .any(|r| {
            r.kind == kind
                && r.anchors
                    .iter()
                    .any(|a| anchor_ids.contains(a.symbol.as_str()))
                && similar(&r.title, &p.title)
        });
    if dup {
        return Err("duplicates a known record".into());
    }
    let same_fact = so_far.iter().filter(|(_, w)| w.is_none()).any(|(r, _)| {
        r.anchors
            .iter()
            .any(|a| anchor_ids.contains(a.symbol.as_str()))
            && r.evidence.iter().any(|e| evidence.contains(e))
    });
    if same_fact {
        return Err("restates another proposal (same evidence and anchor)".into());
    }
    let mut r = Record::new(
        kind,
        &clip(&p.title, 160),
        anchors,
        prov(&b.agent, b.session.clone()),
    );
    r.body = clip(&p.body, 4000);
    r.trigger = p.trigger.clone().filter(|t| !t.trim().is_empty());
    r.evidence = evidence;
    if kind == Kind::Attempt {
        r.fields.insert(
            "outcome".into(),
            p.outcome.clone().unwrap_or_else(|| "failed".into()).into(),
        );
    }
    if kind == Kind::Decision && !p.alternatives.is_empty() {
        r.fields
            .insert("alternatives".into(), serde_json::json!(p.alternatives));
    }
    r.validate().map_err(|e| format!("{e:#}"))?;
    Ok(r)
}

/// Run the model over a bundle; returns accepted records and rejected titles with reasons.
pub async fn run(
    llm: &crate::llm::LlmClient,
    b: &Bundle<'_>,
) -> Result<Vec<(Record, Option<String>)>> {
    if b.spans.is_empty() || b.symbols.is_empty() {
        return Ok(Vec::new());
    }
    let raw = llm.distill(SYSTEM, &user_prompt(b)).await?;
    Ok(accept(parse(&raw), b))
}

fn similar(a: &str, b: &str) -> bool {
    let words = |s: &str| -> HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .map(str::to_string)
            .collect()
    };
    let (x, y) = (words(a), words(b));
    let inter = x.intersection(&y).count() as f64;
    let union = x.union(&y).count().max(1) as f64;
    inter / union >= 0.5
}

/// Keep the first and most recent evidence within budget (the middle is dropped).
fn fit(spans: Vec<Span>) -> Vec<Span> {
    let total: usize = spans.iter().map(|s| s.text.len()).sum();
    if total <= BUDGET {
        return spans;
    }
    let (mut head, mut used) = (Vec::new(), 0);
    for s in &spans {
        if used + s.text.len() > BUDGET / 6 {
            break;
        }
        used += s.text.len();
        head.push(s.clone());
    }
    let mut tail = Vec::new();
    for s in spans.iter().rev() {
        if used + s.text.len() > BUDGET || head.iter().any(|h| h.id == s.id) {
            break;
        }
        used += s.text.len();
        tail.push(s.clone());
    }
    tail.reverse();
    head.extend(tail);
    head
}

fn strip_tags(s: &str) -> String {
    let mut out = s.to_string();
    for tag in [
        "system-reminder",
        "command-message",
        "command-args",
        "local-command-stdout",
    ] {
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        while let Some(a) = out.find(&open) {
            let b = out[a..]
                .find(&close)
                .map_or(out.len(), |i| a + i + close.len());
            out.replace_range(a..b, "");
        }
    }
    out.trim().to_string()
}

fn result_text(part: &serde_json::Value) -> String {
    match part.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn looks_failed(t: &str) -> bool {
    let l = t.to_lowercase();
    [
        "error:",
        "error[",
        "traceback",
        "panicked",
        "failed",
        "exit code 1",
        "fatal:",
    ]
    .iter()
    .any(|m| l.contains(m))
}

/// Lines that look like secrets never leave the machine.
fn redact(t: &str) -> String {
    t.lines()
        .map(|l| {
            if crate::ledger::suspicious(l).is_some() {
                "[redacted]"
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GlobalSymbolId, Language, Position, Range, RepoId};

    fn sym(name: &str) -> Symbol {
        Symbol {
            id: GlobalSymbolId {
                scheme: "t".into(),
                package_manager: ".".into(),
                package: "r".into(),
                version: ".".into(),
                descriptor: format!("app/cache/{name}()."),
            },
            name: name.into(),
            kind: SymbolKind::Function,
            language: Language::Python,
            repo: RepoId("r".into()),
            file_path: "app/cache.py".into(),
            range: Range {
                start: Position { line: 3, column: 0 },
                end: Position { line: 9, column: 0 },
            },
            signature: None,
            docstring: None,
            fingerprint: None,
        }
    }

    const LOG: &str = r#"{"type":"user","sessionId":"s1","message":{"role":"user","content":"add caching to check_permission <system-reminder>ignore me</system-reminder>"}}
{"type":"assistant","sessionId":"s1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/w/r/app/cache.py","old_string":"a","new_string":"b"}}]}}
{"type":"assistant","sessionId":"s1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"pytest tests/test_authz.py"}}]}}
{"type":"user","sessionId":"s1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"FAILED test_role_change - stale grant","is_error":true}]}}
{"type":"user","isSidechain":true,"sessionId":"s1","message":{"role":"user","content":"sidechain noise"}}"#;

    #[test]
    fn transcript_becomes_numbered_redacted_spans() {
        let (spans, edited, session) = transcript(LOG);
        assert_eq!(session.as_deref(), Some("s1"));
        assert_eq!(edited, vec!["/w/r/app/cache.py".to_string()]);
        assert_eq!(spans.len(), 4);
        assert!(!spans[0].text.contains("ignore me"));
        assert!(spans[3].text.starts_with("Bash FAILED"));
        assert!(spans[3].cite.starts_with("session:s1#E4"));
    }

    #[test]
    fn only_grounded_factual_proposals_pass() {
        let s = sym("check_permission");
        let symbols = vec![&s];
        let (spans, _, session) = transcript(LOG);
        let b = bundle(spans, symbols, &[], "claude-code", session);
        let raw = r#"```json
{"records":[
 {"kind":"attempt","title":"Caching permission checks served stale grants after a role change","body":"Cached results outlived role updates.","trigger":"adding caching to authorization","anchors":["S1"],"evidence":["E4"],"outcome":"abandoned"},
 {"kind":"attempt","title":"Cached permission checks returned stale grants after role changes","anchors":["S1"],"evidence":["E4"]},
 {"kind":"gotcha","title":"Never cache permissions","anchors":["S1"],"evidence":["E4"]},
 {"kind":"decision","title":"Picked X","anchors":["S9"],"evidence":["E4"]}
]}
```"#;
        let out = accept(parse(raw), &b);
        let ok: Vec<&Record> = out
            .iter()
            .filter(|(_, w)| w.is_none())
            .map(|(r, _)| r)
            .collect();
        assert_eq!(ok.len(), 1, "{out:?}");
        assert_eq!(ok[0].evidence[0].split(':').next(), Some("session"));
        assert_eq!(ok[0].provenance.author, AuthorKind::Distilled);
        let why: Vec<&str> = out.iter().filter_map(|(_, w)| w.as_deref()).collect();
        assert_eq!(
            why,
            vec!["duplicates a known record", "phrased as an instruction"]
        );
    }
}
