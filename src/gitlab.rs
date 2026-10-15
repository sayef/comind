//! GitLab merge requests as distiller evidence: which MRs carry reasoning worth keeping
//! (reverted, closed unmerged, or discussed), their discussion with people removed, and the
//! files they touched. Uses the REST API with `GITLAB_TOKEN` (or `CI_JOB_TOKEN` in CI).

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::distill::Span;

#[derive(Debug, Clone, Deserialize)]
pub struct MergeRequest {
    pub iid: u64,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// `merged` | `closed` | `opened` | `locked`.
    pub state: String,
    #[serde(default)]
    pub user_notes_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Note {
    pub body: String,
    #[serde(default)]
    pub system: bool,
}

#[derive(Debug, Deserialize)]
struct Diff {
    new_path: String,
}

/// Words that mark a discussion about something that did not work or was decided against.
const SIGNALS: &[&str] = &[
    "tried",
    "doesn't work",
    "does not work",
    "didn't work",
    "won't work",
    "instead",
    "revert",
    "rollback",
    "roll back",
    "broke",
    "breaks",
    "regression",
    "alternative",
    "rejected",
    "abandon",
    "workaround",
    "gotcha",
    "must not",
    "never ",
    "race condition",
    "deadlock",
    "leak",
    "flaky",
    "too slow",
    "timeout",
];

/// Worth sending to the distiller: a revert, or a human discussion that gives a reason (a
/// failure or a choice). Closing an MR "in favour of !528" alone says nothing worth keeping.
pub fn worth_reading(mr: &MergeRequest, notes: Option<&[Note]>) -> bool {
    if mr.title.to_lowercase().starts_with("revert") {
        return true;
    }
    let Some(notes) = notes else {
        // Before fetching notes: only MRs with a discussion can qualify.
        return mr.user_notes_count >= if mr.state == "closed" { 1 } else { 3 };
    };
    let human: Vec<&Note> = notes.iter().filter(|n| !n.system).collect();
    let text = format!(
        "{} {}",
        mr.description.as_deref().unwrap_or(""),
        human
            .iter()
            .map(|n| n.body.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();
    let enough = human.len() >= if mr.state == "closed" { 1 } else { 3 };
    enough && SIGNALS.iter().any(|s| text.contains(s))
}

/// Evidence spans for one MR; mentions of people are removed (records describe code, not people).
pub fn spans(mr: &MergeRequest, notes: &[Note], first: usize) -> Vec<Span> {
    let cite = format!("mr:!{}", mr.iid);
    let mut texts = vec![format!(
        "MR !{} ({}): {}\n{}",
        mr.iid,
        mr.state,
        mr.title,
        clip(mr.description.as_deref().unwrap_or(""), 1500)
    )];
    texts.extend(
        notes
            .iter()
            .filter(|n| !n.system && n.body.trim().len() > 20)
            .take(30)
            .map(|n| format!("review comment on !{}: {}", mr.iid, clip(&n.body, 800))),
    );
    texts
        .into_iter()
        .enumerate()
        .map(|(i, t)| Span {
            id: format!("E{}", first + i),
            text: crate::distill::redact(&depersonalise(&t)),
            cite: cite.clone(),
        })
        .collect()
}

/// `@alice` → `@someone`.
fn depersonalise(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@'
            && chars
                .peek()
                .is_some_and(|n| n.is_alphanumeric() || *n == '_')
        {
            while chars
                .peek()
                .is_some_and(|n| n.is_alphanumeric() || matches!(n, '_' | '-' | '.'))
            {
                chars.next();
            }
            out.push_str("@someone");
        } else {
            out.push(c);
        }
    }
    out
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

pub struct Client {
    http: reqwest::Client,
    base: String,
    project: String,
}

impl Client {
    /// `project` is the path (`group/repo`) or numeric id; base from `GITLAB_URL` (default
    /// gitlab.com, or `CI_API_V4_URL` in GitLab CI).
    pub fn from_env(project: &str) -> Result<Client> {
        let (header, token) = match (std::env::var("GITLAB_TOKEN"), std::env::var("CI_JOB_TOKEN")) {
            (Ok(t), _) if !t.is_empty() => ("PRIVATE-TOKEN", t),
            (_, Ok(t)) if !t.is_empty() => ("JOB-TOKEN", t),
            _ => bail!("set GITLAB_TOKEN (read_api scope) to read merge requests"),
        };
        let base = std::env::var("CI_API_V4_URL").unwrap_or_else(|_| {
            format!(
                "{}/api/v4",
                std::env::var("GITLAB_URL")
                    .unwrap_or_else(|_| "https://gitlab.com".into())
                    .trim_end_matches('/')
            )
        });
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(header, token.parse().context("token header")?);
        Ok(Client {
            http: reqwest::Client::builder()
                .default_headers(headers)
                .build()?,
            base,
            project: project.replace('/', "%2F"),
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}/projects/{}/{path}", self.base, self.project);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        if !resp.status().is_success() {
            bail!("GitLab {path}: {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    /// The most recently updated merged or closed MRs, up to `limit`.
    pub async fn merge_requests(&self, limit: usize) -> Result<Vec<MergeRequest>> {
        let mut out = Vec::new();
        for page in 1..=limit.div_ceil(100).max(1) {
            let batch: Vec<MergeRequest> = self
                .get(&format!(
                    "merge_requests?state=all&order_by=updated_at&per_page=100&page={page}"
                ))
                .await?;
            let done = batch.len() < 100;
            out.extend(batch.into_iter().filter(|m| m.state != "opened"));
            if done || out.len() >= limit {
                break;
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    pub async fn notes(&self, iid: u64) -> Result<Vec<Note>> {
        self.get(&format!(
            "merge_requests/{iid}/notes?per_page=100&sort=asc&order_by=created_at"
        ))
        .await
    }

    pub async fn files(&self, iid: u64) -> Result<Vec<String>> {
        let diffs: Vec<Diff> = self
            .get(&format!("merge_requests/{iid}/diffs?per_page=100"))
            .await?;
        Ok(diffs.into_iter().map(|d| d.new_path).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mr(state: &str, title: &str, notes: u64) -> MergeRequest {
        MergeRequest {
            iid: 412,
            title: title.into(),
            description: Some("Add a per-process cache for competency lookups.".into()),
            state: state.into(),
            user_notes_count: notes,
        }
    }

    fn note(body: &str) -> Note {
        Note {
            body: body.into(),
            system: false,
        }
    }

    #[test]
    fn only_reasoning_heavy_mrs_are_read() {
        assert!(worth_reading(
            &mr("merged", "Revert \"cache competencies\"", 0),
            None
        ));
        assert!(
            worth_reading(&mr("closed", "Cache competencies", 2), None),
            "worth fetching notes"
        );
        let superseded = [note("closing in favor of my PR !528")];
        assert!(!worth_reading(
            &mr("closed", "Cache titles", 1),
            Some(&superseded)
        ));
        let reason = [note("closing: the cache leaked titles across customers")];
        assert!(worth_reading(
            &mr("closed", "Cache titles", 1),
            Some(&reason)
        ));
        assert!(!worth_reading(&mr("merged", "Bump deps", 0), None));
        let talk = [
            note("@alice we tried an LRU here before and it leaked across customers"),
            note("ok, scoping by customer instead"),
            note("LGTM"),
        ];
        assert!(worth_reading(
            &mr("merged", "Speed up lookups", 3),
            Some(&talk)
        ));
        let chat = [note("nice"), note("LGTM"), note("thanks")];
        assert!(!worth_reading(
            &mr("merged", "Speed up lookups", 3),
            Some(&chat)
        ));
    }

    #[test]
    fn spans_drop_people() {
        let notes = [
            note("@alice.b we tried an LRU here before and it leaked across customers"),
            Note {
                body: "added 2 commits".into(),
                system: true,
            },
        ];
        let s = spans(&mr("merged", "Speed up lookups", 1), &notes, 1);
        assert_eq!(s.len(), 2);
        assert!(s[1].text.contains("@someone we tried"));
        assert!(!s[1].text.contains("alice"));
        assert_eq!(s[1].cite, "mr:!412");
        assert_eq!(s[1].id, "E2");
    }
}
