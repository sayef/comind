//! Reasoning ledger: why code is the way it is — decisions, failed attempts, constraints —
//! as typed, symbol-anchored, append-only records in the user's own store
//! (`<index-dir>/_ledger/inbox/<uuid>-<hash>.json`, written create-only so writers never clash).
//! Status (active, moved, stale, orphaned, superseded, retracted) is derived at read time.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::TryStreamExt;
use object_store::path::Path as ObjPath;
use object_store::{ObjectStore, PutMode, PutOptions, PutPayload};
use serde::{Deserialize, Serialize};

use crate::model::{Fingerprint, Symbol, SymbolKind};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Intent,
    Decision,
    Attempt,
    Constraint,
    Gotcha,
    Question,
    /// A relation to another record (supersede, retract, confirm, dispute).
    Relation,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "intent" => Kind::Intent,
            "decision" => Kind::Decision,
            "attempt" => Kind::Attempt,
            "constraint" => Kind::Constraint,
            "gotcha" => Kind::Gotcha,
            "question" => Kind::Question,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Intent => "intent",
            Kind::Decision => "decision",
            Kind::Attempt => "attempt",
            Kind::Constraint => "constraint",
            Kind::Gotcha => "gotcha",
            Kind::Question => "question",
            Kind::Relation => "relation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Supersedes,
    Retracts,
    Confirms,
    Disputes,
}

impl RelationKind {
    pub fn parse(s: &str) -> Option<RelationKind> {
        Some(match s {
            "confirm" | "confirms" => RelationKind::Confirms,
            "retract" | "retracts" => RelationKind::Retracts,
            "supersede" | "supersedes" => RelationKind::Supersedes,
            "dispute" | "disputes" => RelationKind::Disputes,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    Human,
    Agent,
    Harvested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub author: AuthorKind,
    /// Pseudonymous actor id (no names or emails).
    pub actor: String,
    pub source: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
}

/// Where a record points: a symbol as it was when the record was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub symbol: String,
    pub repo: String,
    pub path: String,
    pub name: String,
    pub line: u32,
    #[serde(default)]
    pub fingerprint: Option<Fingerprint>,
}

impl Anchor {
    pub fn of(s: &Symbol) -> Anchor {
        Anchor {
            symbol: s.id.render(),
            repo: s.repo.0.clone(),
            path: s.file_path.clone(),
            name: s.name.clone(),
            line: s.range.start.line,
            fingerprint: s.fingerprint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub schema: u32,
    pub kind: Kind,
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// When a future reader needs this ("relevant when adding caching to auth paths").
    #[serde(default)]
    pub trigger: Option<String>,
    /// Kind-specific fields (`alternatives`, `outcome`, `rule`, `severity`, …).
    #[serde(default)]
    pub fields: BTreeMap<String, serde_json::Value>,
    pub anchors: Vec<Anchor>,
    /// `commit:<sha>`, `mr:!412`, `test:<path>::<name>`, `url:…`, or free text.
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub relation: Option<Relation>,
    pub provenance: Provenance,
    /// RFC 3339, UTC.
    pub created: String,
}

impl Record {
    pub fn new(kind: Kind, title: &str, anchors: Vec<Anchor>, provenance: Provenance) -> Record {
        Record {
            id: uuid::Uuid::now_v7().to_string(),
            schema: SCHEMA,
            kind,
            title: title.trim().to_string(),
            body: String::new(),
            trigger: None,
            fields: BTreeMap::new(),
            anchors,
            evidence: Vec::new(),
            relation: None,
            provenance,
            created: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }
    }

    /// A relation record (`confirm`, `retract`, …) about `target`.
    pub fn relation(
        kind: RelationKind,
        target: &Record,
        reason: &str,
        provenance: Provenance,
    ) -> Record {
        let title = if reason.trim().is_empty() {
            format!("{kind:?}: {}", target.title)
        } else {
            reason.trim().to_string()
        };
        let mut r = Record::new(Kind::Relation, &title, Vec::new(), provenance);
        r.relation = Some(Relation {
            kind,
            target: target.id.clone(),
        });
        r
    }

    /// Reject records that would mislead or carry secrets; caps keep them reviewable.
    pub fn validate(&self) -> Result<()> {
        if self.title.is_empty() || self.title.chars().count() > 160 {
            bail!("title must be 1–160 characters");
        }
        if self.body.chars().count() > 4000 {
            bail!("body is limited to 4000 characters");
        }
        if self.kind != Kind::Relation && self.anchors.is_empty() {
            bail!("a record needs at least one anchor (a symbol it is about)");
        }
        if let Some(rule) = self.fields.get("check").and_then(|v| v.as_str()) {
            if self.kind != Kind::Constraint {
                bail!("only constraints carry a check");
            }
            crate::checks::Rule::parse(rule)?;
        }
        if self.kind == Kind::Attempt && self.evidence.is_empty() {
            bail!("an attempt needs evidence (a revert commit, failing test, error, MR, …)");
        }
        let text = format!("{} {} {}", self.title, self.body, self.evidence.join(" "));
        if let Some(why) = suspicious(&text) {
            bail!("refusing to store record: {why}");
        }
        Ok(())
    }
}

/// Secrets and invisible/bidi characters never enter the shared ledger.
fn suspicious(text: &str) -> Option<&'static str> {
    if text
        .chars()
        .any(|c| matches!(c as u32, 0x202A..=0x202E | 0x2066..=0x2069 | 0xE0000..=0xE007F))
    {
        return Some("contains invisible or bidirectional control characters");
    }
    let lower = text.to_ascii_lowercase();
    let markers = [
        "-----begin",
        "aws_secret_access_key",
        "xoxb-",
        "ghp_",
        "glpat-",
        "sk-ant-",
        "password=",
    ];
    if markers.iter().any(|m| lower.contains(m))
        || text.split_whitespace().any(|w| {
            w.starts_with("AKIA") && w.len() == 20 && w.chars().all(|c| c.is_ascii_alphanumeric())
        })
    {
        return Some("looks like it contains a secret");
    }
    None
}

/// A pseudonymous, stable actor id for this machine and user (no names stored).
pub fn actor() -> String {
    if let Ok(a) = std::env::var("COMIND_ACTOR") {
        return a;
    }
    let who = std::env::var("USER").unwrap_or_default();
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_default();
    let h = blake3::hash(format!("{who}@{host}").as_bytes()).to_hex();
    format!("actor-{}", &h[..10])
}

/// The ledger under an index root (local directory or `s3://bucket/prefix`).
pub struct Ledger {
    store: Arc<dyn ObjectStore>,
    root: ObjPath,
}

impl Ledger {
    pub fn open(index_root: &str) -> Result<Ledger> {
        let root = index_root.trim_end_matches('/');
        if let Some(rest) = root.strip_prefix("s3://") {
            let (bucket, prefix) = rest.split_once('/').unwrap_or((rest, ""));
            let store = object_store::aws::AmazonS3Builder::from_env()
                .with_bucket_name(bucket)
                .build()
                .context("open S3 store")?;
            return Ok(Ledger {
                store: Arc::new(store),
                root: ObjPath::from(format!("{prefix}/_ledger").trim_start_matches('/')),
            });
        }
        let dir = std::path::Path::new(root).join("_ledger");
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let store = object_store::local::LocalFileSystem::new_with_prefix(&dir)
            .context("open local ledger")?;
        Ok(Ledger {
            store: Arc::new(store),
            root: ObjPath::from(""),
        })
    }

    fn inbox(&self) -> ObjPath {
        self.root.clone().join("inbox")
    }

    /// Write one record, create-only: an existing key is an error, never an overwrite.
    pub async fn put(&self, rec: &Record) -> Result<()> {
        rec.validate()?;
        let bytes = serde_json::to_vec_pretty(rec)?;
        let hash = blake3::hash(&bytes).to_hex();
        let key = self
            .inbox()
            .join(format!("{}-{}.json", rec.id, &hash[..16]));
        let opts = PutOptions {
            mode: PutMode::Create,
            ..Default::default()
        };
        self.store
            .put_opts(&key, PutPayload::from(bytes), opts)
            .await
            .with_context(|| format!("write ledger record {key}"))?;
        Ok(())
    }

    pub async fn all(&self) -> Result<Vec<Record>> {
        let metas: Vec<_> = self.store.list(Some(&self.inbox())).try_collect().await?;
        let mut out = Vec::with_capacity(metas.len());
        for m in metas {
            if !m.location.as_ref().ends_with(".json") {
                continue;
            }
            let bytes = self
                .store
                .get_opts(&m.location, object_store::GetOptions::default())
                .await?
                .bytes()
                .await?;
            match serde_json::from_slice::<Record>(&bytes) {
                Ok(r) => out.push(r),
                Err(e) => eprintln!(
                    "comind: skipping unreadable ledger record {}: {e}",
                    m.location
                ),
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }
}

/// Records derived from commit history: reverts become `attempt`s, trailers
/// (`Constraint:`, `Rejected:`, `Decision:`, `Gotcha:`, `Intent:`) become typed records.
/// Ids are deterministic so re-harvesting never duplicates.
pub fn harvest(repo: &str, history: &[crate::git::CommitFiles], symbols: &[Symbol]) -> Vec<Record> {
    let files: HashMap<&str, &Symbol> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::File && s.repo.0 == repo)
        .map(|s| (s.file_path.as_str(), s))
        .collect();
    let subjects: HashMap<&str, &str> = history
        .iter()
        .map(|c| (c.sha.as_str(), c.subject.as_str()))
        .collect();
    let prov = Provenance {
        author: AuthorKind::Harvested,
        actor: "git".into(),
        source: "git".into(),
        agent: None,
        session: None,
    };
    let mut out = Vec::new();
    for c in history {
        let anchors: Vec<Anchor> = c
            .changed
            .iter()
            .filter_map(|p| files.get(p.as_str()))
            .take(4)
            .map(|s| Anchor::of(s))
            .collect();
        if anchors.is_empty() {
            continue;
        }
        let short = &c.sha[..c.sha.len().min(10)];
        let make = |kind: Kind, n: usize, title: &str| {
            let mut r = Record::new(kind, title, anchors.clone(), prov.clone());
            r.id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!("comind-harvest:{repo}:{}:{n}", c.sha).as_bytes(),
            )
            .to_string();
            r.evidence.push(format!("commit:{short}"));
            r
        };
        // A manual revert ("Reverted the X changes") without git's marker.
        let manual = c.subject.to_ascii_lowercase();
        if !c.message.contains("This reverts commit ")
            && (manual.starts_with("revert ") || manual.starts_with("reverted "))
        {
            let what = c.subject.split_once(' ').map_or("", |(_, w)| w).trim();
            let title: String = format!("Reverted: {what}").chars().take(160).collect();
            let mut r = make(Kind::Attempt, 0, &title);
            r.fields.insert(
                "outcome".into(),
                serde_json::Value::String("reverted".into()),
            );
            r.body = c.message.trim().chars().take(1500).collect();
            out.push(r);
        }
        // `git revert`: "Revert \"…\"" + "This reverts commit <sha>."
        if let Some(original) = c
            .message
            .split("This reverts commit ")
            .nth(1)
            .and_then(|r| r.split(|ch: char| !ch.is_ascii_hexdigit()).next())
            .filter(|s| s.len() >= 7)
        {
            let what = subjects
                .iter()
                .find(|(sha, _)| sha.starts_with(original))
                .map(|(_, subj)| subj.to_string())
                .unwrap_or_else(|| {
                    c.subject
                        .trim_start_matches("Revert ")
                        .trim_matches('"')
                        .to_string()
                });
            let title: String = format!("Reverted: {what}").chars().take(160).collect();
            let mut r = make(Kind::Attempt, 0, &title);
            r.fields.insert(
                "outcome".into(),
                serde_json::Value::String("reverted".into()),
            );
            r.evidence
                .push(format!("commit:{}", &original[..original.len().min(10)]));
            r.body = c.message.trim().chars().take(1500).collect();
            out.push(r);
        }
        for (n, line) in c.message.lines().enumerate() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let kind = match key.trim() {
                "Constraint" | "Directive" => Kind::Constraint,
                "Rejected" | "Decision" => Kind::Decision,
                "Gotcha" => Kind::Gotcha,
                "Intent" => Kind::Intent,
                _ => continue,
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            let title: String = value.chars().take(160).collect();
            let mut r = make(kind, n + 1, &title);
            if key.trim() == "Rejected" {
                r.fields
                    .insert("rejected".into(), serde_json::Value::String(value.into()));
            }
            r.body = c.subject.clone();
            out.push(r);
        }
    }
    out.retain(|r| r.validate().is_ok());
    out
}

/// Blocking helpers for the CLI.
pub fn put_blocking(index_root: &str, rec: &Record) -> Result<()> {
    let ledger = Ledger::open(index_root)?;
    runtime()?.block_on(ledger.put(rec))
}

pub fn all_blocking(index_root: &str) -> Result<Vec<Record>> {
    let ledger = Ledger::open(index_root)?;
    runtime()?.block_on(ledger.all())
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    /// The anchored symbol moved or was renamed; its body is unchanged.
    Moved,
    /// The anchored symbol's body changed since the record was written: re-verify.
    Stale,
    /// The anchored symbol no longer exists.
    Orphaned,
    Superseded,
    Retracted,
}

/// T4 constraint enforced by a passing check, T3 human or human-confirmed, T2 agent with evidence,
/// T1 agent assertion, T0 untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Trust {
    T0,
    T1,
    T2,
    T3,
    T4,
}

/// A record with its derived state against the current index.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub record: Record,
    pub status: Status,
    pub trust: Trust,
    /// Current symbol ids the record's anchors map to (after moves/renames).
    pub current: Vec<String>,
    /// Ids of records that supersede, retract, confirm or dispute this one.
    pub related: Vec<(RelationKind, String)>,
}

/// Derive status and trust for every non-relation record.
pub fn derive(records: &[Record], symbols: &[Symbol]) -> Vec<Entry> {
    let by_id: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.render(), s)).collect();
    let mut by_shape: HashMap<(&str, u64), Vec<&Symbol>> = HashMap::new();
    for s in symbols.iter().filter(|s| s.kind != SymbolKind::File) {
        if let Some(fp) = s.fingerprint {
            by_shape
                .entry((s.repo.0.as_str(), fp.shape))
                .or_default()
                .push(s);
        }
    }
    let mut related: HashMap<&str, Vec<(RelationKind, String, AuthorKind)>> = HashMap::new();
    for r in records {
        if let Some(rel) = &r.relation {
            related.entry(rel.target.as_str()).or_default().push((
                rel.kind,
                r.id.clone(),
                r.provenance.author,
            ));
        }
    }
    records
        .iter()
        .filter(|r| r.kind != Kind::Relation)
        .map(|r| {
            let rels = related.get(r.id.as_str()).cloned().unwrap_or_default();
            let mut status = Status::Active;
            let mut current = Vec::new();
            for a in &r.anchors {
                let (st, now) = anchor_status(a, &by_id, &by_shape);
                current.extend(now);
                status = status.max(st);
            }
            if rels.iter().any(|(k, _, _)| *k == RelationKind::Supersedes) {
                status = Status::Superseded;
            }
            if rels.iter().any(|(k, _, _)| *k == RelationKind::Retracts) {
                status = Status::Retracted;
            }
            let confirmed_by_human = rels
                .iter()
                .any(|(k, _, a)| *k == RelationKind::Confirms && *a == AuthorKind::Human);
            let trust = match r.provenance.author {
                AuthorKind::Human => Trust::T3,
                _ if confirmed_by_human => Trust::T3,
                // Evidence for agents and harvesting is a commit/test the record cites.
                AuthorKind::Agent | AuthorKind::Harvested if !r.evidence.is_empty() => Trust::T2,
                AuthorKind::Agent => Trust::T1,
                AuthorKind::Harvested => Trust::T0,
            };
            Entry {
                record: r.clone(),
                status,
                trust,
                current,
                related: rels.into_iter().map(|(k, id, _)| (k, id)).collect(),
            }
        })
        .collect()
}

fn anchor_status(
    a: &Anchor,
    by_id: &HashMap<String, &Symbol>,
    by_shape: &HashMap<(&str, u64), Vec<&Symbol>>,
) -> (Status, Option<String>) {
    match (by_id.get(&a.symbol), a.fingerprint) {
        (Some(s), Some(fp)) => match s.fingerprint {
            Some(now) if now.body != fp.body => (Status::Stale, Some(a.symbol.clone())),
            _ => (Status::Active, Some(a.symbol.clone())),
        },
        (Some(_), None) => (Status::Active, Some(a.symbol.clone())),
        (None, Some(fp)) => match by_shape
            .get(&(a.repo.as_str(), fp.shape))
            .map(Vec::as_slice)
        {
            // Renamed or moved with an unchanged shape: follow it.
            Some([only]) => (Status::Moved, Some(only.id.render())),
            _ => (Status::Orphaned, None),
        },
        (None, None) => (Status::Orphaned, None),
    }
}

/// Entries whose anchors map to any of `symbols` (current ids), most relevant first.
pub fn about<'e>(entries: &'e [Entry], symbols: &[String]) -> Vec<&'e Entry> {
    let mut v: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.current.iter().any(|c| symbols.contains(c)))
        .collect();
    v.sort_by(|a, b| {
        (
            a.status,
            std::cmp::Reverse(a.trust),
            kind_rank(a.record.kind),
        )
            .cmp(&(
                b.status,
                std::cmp::Reverse(b.trust),
                kind_rank(b.record.kind),
            ))
            .then(b.record.id.cmp(&a.record.id))
    });
    v
}

/// The review inbox (stale, orphaned, or unconfirmed agent records) or everything; most urgent first.
pub fn inbox(entries: &[Entry], only_attention: bool) -> Vec<&Entry> {
    let mut v: Vec<&Entry> = entries
        .iter()
        .filter(|e| {
            !only_attention
                || matches!(e.status, Status::Stale | Status::Orphaned)
                || (e.trust < Trust::T3 && matches!(e.status, Status::Active | Status::Moved))
        })
        .collect();
    v.sort_by_key(|e| (attention(e.status), e.trust));
    v
}

fn attention(s: Status) -> u8 {
    match s {
        Status::Stale => 0,
        Status::Orphaned => 1,
        Status::Active | Status::Moved => 2,
        Status::Superseded | Status::Retracted => 3,
    }
}

/// Constraints and failed attempts first: they are what prevents repeated mistakes.
fn kind_rank(k: Kind) -> u8 {
    match k {
        Kind::Constraint => 0,
        Kind::Attempt => 1,
        Kind::Gotcha => 2,
        Kind::Decision => 3,
        Kind::Intent => 4,
        Kind::Question => 5,
        Kind::Relation => 6,
    }
}

/// Render entries as quoted, attributed facts with a verify pointer (never as instructions).
pub fn markdown(title: &str, entries: &[&Entry]) -> String {
    let mut o = format!("## {title}\n");
    if entries.is_empty() {
        o.push_str("\n_no recorded reasoning yet — add one with `remember`._\n");
        return o;
    }
    o.push_str("\n> Recorded facts from the team ledger. Verify against the code before relying on them.\n");
    for e in entries {
        let r = &e.record;
        o.push_str(&format!(
            "\n### [{} · {:?} · {}] {}\n",
            r.kind.as_str(),
            e.trust,
            status_label(e.status),
            r.title
        ));
        if !r.body.is_empty() {
            o.push_str(&format!("{}\n", r.body));
        }
        for (k, v) in &r.fields {
            o.push_str(&format!("- {k}: {}\n", value_text(v)));
        }
        if let Some(t) = &r.trigger {
            o.push_str(&format!("- relevant when: {t}\n"));
        }
        if !r.evidence.is_empty() {
            o.push_str(&format!("- evidence: {}\n", r.evidence.join(", ")));
        }
        let at: Vec<String> = r
            .anchors
            .iter()
            .map(|a| format!("`{}` ({}:{})", a.name, a.path, a.line))
            .collect();
        o.push_str(&format!("- about: {}\n", at.join(", ")));
        o.push_str(&format!(
            "- by {} via {} on {} · id `{}`\n",
            match r.provenance.author {
                AuthorKind::Human => "a human",
                AuthorKind::Agent => "an agent",
                AuthorKind::Harvested => "harvesting",
            },
            r.provenance.source,
            &r.created[..r.created.len().min(10)],
            r.id
        ));
    }
    o
}

/// One compact line for embedding in other tool results.
pub fn line(e: &Entry) -> String {
    let r = &e.record;
    let at = r
        .anchors
        .first()
        .map(|a| format!(" — about `{}` ({}:{})", a.name, a.path, a.line))
        .unwrap_or_default();
    let ev = if r.evidence.is_empty() {
        String::new()
    } else {
        format!(" · evidence {}", r.evidence.join(", "))
    };
    format!(
        "[{} · {:?} · {}] {}{at}{ev} · id `{}`",
        r.kind.as_str(),
        e.trust,
        status_label(e.status),
        r.title,
        &r.id[..r.id.len().min(13)]
    )
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::Active => "active",
        Status::Moved => "active, symbol moved",
        Status::Stale => "STALE — code changed since",
        Status::Orphaned => "orphaned — symbol gone",
        Status::Superseded => "superseded",
        Status::Retracted => "retracted",
    }
}

fn value_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().map(value_text).collect::<Vec<_>>().join("; "),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GlobalSymbolId, Language, Position, Range, RepoId};

    fn sym(name: &str, body: u64, shape: u64) -> Symbol {
        Symbol {
            id: GlobalSymbolId {
                scheme: "t".into(),
                package_manager: ".".into(),
                package: "r".into(),
                version: ".".into(),
                descriptor: format!("m/{name}()."),
            },
            name: name.into(),
            kind: SymbolKind::Function,
            language: Language::Python,
            repo: RepoId("r".into()),
            file_path: "m.py".into(),
            range: Range {
                start: Position { line: 1, column: 0 },
                end: Position { line: 3, column: 0 },
            },
            signature: None,
            docstring: None,
            fingerprint: Some(Fingerprint {
                sig: 1,
                body,
                shape,
            }),
        }
    }

    fn prov(author: AuthorKind) -> Provenance {
        Provenance {
            author,
            actor: "actor-test".into(),
            source: "test".into(),
            agent: None,
            session: None,
        }
    }

    #[test]
    fn status_follows_the_code() {
        let before = sym("check", 10, 7);
        let mut rec = Record::new(
            Kind::Constraint,
            "no caching here",
            vec![Anchor::of(&before)],
            prov(AuthorKind::Human),
        );
        rec.body = "grants change".into();
        let records = vec![rec.clone()];

        // Unchanged → active.
        let e = derive(&records, std::slice::from_ref(&before));
        assert_eq!((e[0].status, e[0].trust), (Status::Active, Trust::T3));
        // Body changed → stale.
        let e = derive(&records, &[sym("check", 11, 8)]);
        assert_eq!(e[0].status, Status::Stale);
        // Renamed, same shape → moved, re-anchored.
        let e = derive(&records, &[sym("verify", 99, 7)]);
        assert_eq!(e[0].status, Status::Moved);
        assert!(e[0].current[0].contains("verify"));
        // Gone → orphaned.
        let e = derive(&records, &[sym("other", 1, 1)]);
        assert_eq!(e[0].status, Status::Orphaned);

        // A retraction wins.
        let mut retract = Record::new(
            Kind::Relation,
            "no longer true",
            vec![],
            prov(AuthorKind::Human),
        );
        retract.relation = Some(Relation {
            kind: RelationKind::Retracts,
            target: rec.id.clone(),
        });
        let e = derive(&[rec, retract], &[before]);
        assert_eq!(e[0].status, Status::Retracted);
    }

    #[test]
    fn validation_guards_the_shared_ledger() {
        let s = sym("f", 1, 1);
        let attempt = Record::new(
            Kind::Attempt,
            "tried redis",
            vec![Anchor::of(&s)],
            prov(AuthorKind::Agent),
        );
        assert!(attempt.validate().is_err(), "attempt without evidence");
        let mut leak = Record::new(
            Kind::Gotcha,
            "token",
            vec![Anchor::of(&s)],
            prov(AuthorKind::Agent),
        );
        leak.body = "use ghp_abcdefghijklmnop".into();
        assert!(leak.validate().is_err());
        let mut bidi = Record::new(
            Kind::Gotcha,
            "x",
            vec![Anchor::of(&s)],
            prov(AuthorKind::Agent),
        );
        bidi.body = "safe\u{202E}evil".into();
        assert!(bidi.validate().is_err());
    }

    #[tokio::test]
    async fn create_only_inbox_roundtrip() {
        let dir = std::env::temp_dir().join(format!("comind-ledger-{}", std::process::id()));
        let ledger = Ledger::open(dir.to_str().unwrap()).unwrap();
        let s = sym("f", 1, 1);
        let mut rec = Record::new(
            Kind::Decision,
            "chose pypika",
            vec![Anchor::of(&s)],
            prov(AuthorKind::Human),
        );
        rec.fields.insert(
            "alternatives".into(),
            serde_json::json!(["sqlalchemy core"]),
        );
        ledger.put(&rec).await.unwrap();
        assert!(
            ledger.put(&rec).await.is_err(),
            "same record twice must not overwrite"
        );
        let back = ledger.all().await.unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(back, vec![rec]);
    }
}
