//! Repositories from the UI: the list `comind link` indexes, scanning a folder, cloning, and an
//! indexing job whose output the page follows.

use std::path::Path;

use anyhow::Context as _;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{Cur, Shared};
use crate::config::{Config, RepoEntry};
use crate::worktree;

/// The current or last indexing run.
#[derive(Default)]
pub(super) struct Job {
    running: bool,
    ok: Option<bool>,
    lines: Vec<String>,
    started: Option<String>,
    finished: Option<String>,
}

fn fail(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"ok": false, "message": format!("{e:#}")})),
    )
}

fn absolute(p: &str) -> String {
    let p = crate::config::expand_tilde(p.trim());
    std::fs::canonicalize(&p)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or(p)
}

pub(super) async fn list(Cur(s): Cur) -> Json<Value> {
    let indexed: std::collections::HashMap<String, usize> =
        s.data.graph.repos().into_iter().collect();
    let commits = match super::graph_uri(&s) {
        Some(u) => crate::index::read_repo_commits(&u)
            .await
            .unwrap_or_default(),
        None => Default::default(),
    };
    let repos = Config::load().repos;
    let rows = tokio::task::spawn_blocking(move || {
        repos
            .iter()
            .map(|r| {
                let p = Path::new(&r.path);
                let name = worktree::name(p);
                json!({
                    "path": r.path,
                    "name": name,
                    "exists": p.join(".git").exists(),
                    "branch": r.branch.clone().or_else(|| worktree::default_branch(p).ok()),
                    "your_branch": worktree::current_branch(p),
                    "remote": worktree::remote_url(p),
                    "symbols": indexed.get(&name),
                    "indexed_commit": commits.get(&name),
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let others: Vec<String> = s
        .data
        .graph
        .repos()
        .into_iter()
        .map(|(r, _)| r)
        .filter(|r| !rows.iter().any(|x| x["name"] == *r))
        .collect();
    Json(json!({ "repos": rows, "unlisted": others, "root": s.data.root }))
}

#[derive(Deserialize)]
pub(super) struct ScanQ {
    dir: String,
    #[serde(default)]
    depth: Option<usize>,
}

/// Git repositories under a folder, marked when already listed.
pub(super) async fn scan(Json(q): Json<ScanQ>) -> impl IntoResponse {
    let dir = absolute(&q.dir);
    if !Path::new(&dir).is_dir() {
        return fail(anyhow::anyhow!("{dir} is not a folder"));
    }
    let listed: Vec<String> = Config::load()
        .repos
        .iter()
        .map(|r| absolute(&r.path))
        .collect();
    let found = tokio::task::spawn_blocking(move || {
        worktree::scan(Path::new(&dir), q.depth.unwrap_or(2).clamp(1, 4))
            .into_iter()
            .map(|p| {
                let path = p.to_string_lossy().into_owned();
                json!({
                    "name": worktree::name(&p),
                    "listed": listed.contains(&path),
                    "remote": worktree::remote_url(&p),
                    "path": path,
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    (StatusCode::OK, Json(json!({"ok": true, "found": found})))
}

#[derive(Deserialize)]
pub(super) struct Change {
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
}

/// Add and remove repositories (folders); removed ones lose comind's worktree, not your folder.
pub(super) async fn change(Json(c): Json<Change>) -> impl IntoResponse {
    let run = tokio::task::spawn_blocking(move || {
        let mut list = Config::load().repos;
        for p in &c.add {
            let path = absolute(p);
            anyhow::ensure!(
                Path::new(&path).join(".git").exists(),
                "{path} is not a git repository"
            );
            if !list.iter().any(|r| absolute(&r.path) == path) {
                list.push(RepoEntry { path, branch: None });
            }
        }
        let gone: Vec<String> = c.remove.iter().map(|p| absolute(p)).collect();
        list.retain(|r| {
            let drop = gone.contains(&absolute(&r.path));
            if drop {
                let _ = worktree::remove(Path::new(&r.path));
            }
            !drop
        });
        crate::config::set_repos(&list)?;
        anyhow::Ok(list.len())
    })
    .await;
    match run.map_err(anyhow::Error::from).and_then(|r| r) {
        Ok(n) => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": format!("{n} repositories")})),
        ),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
pub(super) struct CloneQ {
    url: String,
    dir: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

/// Clone a repository with your git credentials and add it.
pub(super) async fn clone(Json(q): Json<CloneQ>) -> impl IntoResponse {
    let run = tokio::task::spawn_blocking(move || {
        let auth = q.token.filter(|t| !t.trim().is_empty()).map(|t| worktree::Auth {
            username: q.username.filter(|u| !u.trim().is_empty()).unwrap_or_else(|| "oauth2".into()),
            token: t.trim().to_string(),
        });
        let given = auth.is_some();
        let dest = worktree::clone(q.url.trim(), Path::new(&absolute(&q.dir)), auth)?;
        let mut list = Config::load().repos;
        list.push(RepoEntry {
            path: dest.to_string_lossy().into_owned(),
            branch: None,
        });
        crate::config::set_repos(&list)?;
        let note = if given && !worktree::has_credential_helper() {
            "; no git credential helper is set up, so the token was not saved and fetching later will need it again"
        } else if given {
            "; the token was saved by your git credential helper"
        } else {
            ""
        };
        anyhow::Ok(format!("cloned into {}{note}", dest.display()))
    })
    .await;
    match run.map_err(anyhow::Error::from).and_then(|r| r) {
        Ok(m) => (StatusCode::OK, Json(json!({"ok": true, "message": m}))),
        Err(e) => match e.downcast_ref::<worktree::NeedsToken>() {
            Some(n) => (
                StatusCode::UNAUTHORIZED,
                Json(
                    json!({"ok": false, "message": e.to_string(), "needs_token": {"host": n.host, "username": n.username}}),
                ),
            ),
            None => fail(e),
        },
    }
}

pub(super) async fn job(State(app): State<Shared>) -> Json<Value> {
    let j = app.job.lock().await;
    let from = j.lines.len().saturating_sub(400);
    Json(json!({
        "running": j.running,
        "ok": j.ok,
        "lines": &j.lines[from..],
        "started": j.started,
        "finished": j.finished,
    }))
}

#[derive(Deserialize)]
pub(super) struct IndexQ {
    #[serde(default)]
    llm: bool,
}

pub(super) async fn start(State(app): State<Shared>, Json(q): Json<IndexQ>) -> impl IntoResponse {
    match begin(&app, q.llm, None).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": "indexing started"})),
        ),
        Err(e) => fail(e),
    }
}

/// Start `comind link` over the listed repositories into the loaded index; the page follows
/// `/api/index` and the UI switches to the new index when it succeeds. `why` opens the log.
pub(super) async fn begin(app: &Shared, llm: bool, why: Option<String>) -> anyhow::Result<()> {
    let root = app
        .current
        .read()
        .await
        .data
        .root
        .clone()
        .context("read-only UI")?;
    {
        let mut j = app.job.lock().await;
        anyhow::ensure!(!j.running, "indexing is already running");
        *j = Job {
            running: true,
            started: Some(chrono::Utc::now().to_rfc3339()),
            lines: why.into_iter().collect(),
            ..Default::default()
        };
    }
    let mut cmd = tokio::process::Command::new(std::env::current_exe()?);
    cmd.args(["link", "--incremental", "--index-dir", &root]);
    if !llm {
        cmd.arg("--no-llm");
    }
    cmd.env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            app.job.lock().await.running = false;
            return Err(e.into());
        }
    };
    let app2 = app.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        type Pipe = Box<dyn tokio::io::AsyncRead + Send + Unpin>;
        let follow = |r: Option<Pipe>| {
            let app = app2.clone();
            async move {
                let Some(r) = r else { return };
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    let l = strip_ansi(&l);
                    if !l.trim().is_empty() {
                        app.job.lock().await.lines.push(l);
                    }
                }
            }
        };
        let out = child.stdout.take().map(|s| Box::new(s) as Pipe);
        let err = child.stderr.take().map(|s| Box::new(s) as Pipe);
        tokio::join!(follow(out), follow(err));
        let ok = child.wait().await.map(|s| s.success()).unwrap_or(false);
        if ok {
            if let Err(e) = app2.reload(&root).await {
                let msg = format!("reload failed: {e:#}");
                app2.job.lock().await.lines.push(msg);
            }
        }
        let mut j = app2.job.lock().await;
        j.running = false;
        j.ok = Some(ok);
        j.finished = Some(chrono::Utc::now().to_rfc3339());
    });
    Ok(())
}

/// What the automatic re-index last found.
#[derive(Default, Clone, serde::Serialize)]
pub(super) struct Watch {
    checked: Option<String>,
    result: String,
}

/// With `auto_index` on, check every freshness interval whether a default branch moved on the
/// remote (`git ls-remote`, no download) and re-index when one did.
pub(super) async fn watch(app: Shared) {
    loop {
        let cfg = Config::load();
        tokio::time::sleep(cfg.fresh().max(std::time::Duration::from_secs(60))).await;
        if Config::load().auto_index != Some(true) {
            continue;
        }
        let result = match moved(&app).await {
            Ok(m) if m.is_empty() => "every default branch is indexed".to_string(),
            Ok(m) => {
                let why = format!("Automatic re-index: {} moved", m.join(", "));
                match begin(&app, false, Some(why.clone())).await {
                    Ok(()) => why,
                    Err(e) => format!("{why}, not started: {e:#}"),
                }
            }
            Err(e) => format!("check failed: {e:#}"),
        };
        *app.watch.lock().await = Watch {
            checked: Some(chrono::Utc::now().to_rfc3339()),
            result,
        };
    }
}

/// Listed repositories whose remote default branch differs from what the index holds.
async fn moved(app: &Shared) -> anyhow::Result<Vec<String>> {
    let uri = super::graph_uri(&app.current.read().await.clone()).context("no index")?;
    let indexed = crate::index::read_repo_commits(&uri)
        .await
        .unwrap_or_default();
    let repos = Config::load().repos;
    tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for r in &repos {
            let p = Path::new(&r.path);
            let name = worktree::name(p);
            let branch = match &r.branch {
                Some(b) => b.clone(),
                None => worktree::default_branch(p)?,
            };
            let head = worktree::remote_head(p, &branch)?;
            if indexed
                .get(&name)
                .is_none_or(|c| !head.starts_with(c.as_str()) && !c.starts_with(&head))
            {
                out.push(name);
            }
        }
        anyhow::Ok(out)
    })
    .await?
}

pub(super) async fn watch_state(State(app): State<Shared>) -> Json<Value> {
    Json(json!({
        "enabled": Config::load().auto_index == Some(true),
        "last": *app.watch.lock().await,
    }))
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}
