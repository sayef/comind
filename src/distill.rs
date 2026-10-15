//! Distiller: proposes ledger records (failed attempts, decisions, constraints, gotchas) from
//! commit history. Agent session transcripts are never read. Pre-processing and validation are
//! deterministic; the LLM only picks candidates. Proposals land at T1 for human review.

use std::collections::HashSet;

use anyhow::Result;
use serde::Deserialize;

use crate::ledger::{Anchor, AuthorKind, Entry, Kind, Provenance, Record, Status};
use crate::model::{Symbol, SymbolKind};

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
    /// Where the evidence came from (`git`).
    pub agent: String,
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
    }
}

pub const SYSTEM: &str = "You extract durable engineering knowledge from commit messages \
Default output is an empty list.\n\
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
    Record::new(Kind::Gotcha, &clip(&p.title, 160), Vec::new(), prov("?"))
}

fn prov(agent: &str) -> Provenance {
    Provenance {
        author: AuthorKind::Distilled,
        actor: crate::ledger::actor(),
        source: "distiller".into(),
        agent: Some(agent.to_string()),
        session: None,
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
    let mut r = Record::new(kind, &clip(&p.title, 160), anchors, prov(&b.agent));
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

    fn history() -> Vec<crate::git::CommitFiles> {
        vec![crate::git::CommitFiles {
            sha: "abcdef1234567".into(),
            subject: "Revert permission cache".into(),
            message:
                "Revert permission cache\n\nCaching check_permission results served stale grants \
                      after a role change (test_role_change failed in CI), so the cache is removed."
                    .into(),
            time: 0,
            added: Vec::new(),
            changed: vec!["app/cache.py".into()],
        }]
    }

    #[test]
    fn commits_become_cited_redacted_spans() {
        let spans = commits(&history(), &HashSet::new());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].cite, "commit:abcdef1234");
        assert!(spans[0].text.contains("files: app/cache.py"));
        let skip: HashSet<String> = ["abcdef1234".to_string()].into();
        assert!(commits(&history(), &skip).is_empty());
    }

    #[test]
    fn only_grounded_factual_proposals_pass() {
        let s = sym("check_permission");
        let symbols = vec![&s];
        let spans = commits(&history(), &HashSet::new());
        let b = bundle(spans, symbols, &[], "git");
        let raw = r#"```json
{"records":[
 {"kind":"attempt","title":"Caching permission checks served stale grants after a role change","body":"Cached results outlived role updates.","trigger":"adding caching to authorization","anchors":["S1"],"evidence":["E1"],"outcome":"abandoned"},
 {"kind":"attempt","title":"Cached permission checks returned stale grants after role changes","anchors":["S1"],"evidence":["E1"]},
 {"kind":"gotcha","title":"Never cache permissions","anchors":["S1"],"evidence":["E1"]},
 {"kind":"decision","title":"Picked X","anchors":["S9"],"evidence":["E1"]}
]}
```"#;
        let out = accept(parse(raw), &b);
        let ok: Vec<&Record> = out
            .iter()
            .filter(|(_, w)| w.is_none())
            .map(|(r, _)| r)
            .collect();
        assert_eq!(ok.len(), 1, "{out:?}");
        assert_eq!(ok[0].evidence[0], "commit:abcdef1234");
        assert_eq!(ok[0].provenance.author, AuthorKind::Distilled);
        let why: Vec<&str> = out.iter().filter_map(|(_, w)| w.as_deref()).collect();
        assert_eq!(
            why,
            vec!["duplicates a known record", "phrased as an instruction"]
        );
    }
}
