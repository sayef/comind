//! Repositories from the UI: the list `comindly link` indexes, scanning a folder, cloning, and an
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
    /// The indexer's process group, to stop it.
    pid: Option<u32>,
    cancelled: bool,
    ok: Option<bool>,
    /// The indexer's events: `timestamp`, `level`, `kind`, `step`, `message` and fields.
    events: Vec<Value>,
    started: Option<String>,
    finished: Option<String>,
}

fn fail(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"ok": false, "message": super::error_text(&e)})),
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
    let meta = match super::graph_uri(&s) {
        Some(u) => crate::index::read_indexed_repos(&u)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let commits: std::collections::HashMap<String, String> = meta
        .iter()
        .map(|r| (r.name.clone(), r.commit.clone()))
        .collect();
    let repos = Config::load().repos;
    let symbols = indexed.clone();
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
                    "branch": worktree::default_branch(p).ok(),
                    "managed": r.remote.is_some(),
                    "cloned": p.exists(),
                    "your_branch": worktree::current_branch(p),
                    "remote": r.remote.clone().or_else(|| worktree::remote_url(p)).map(|u| crate::storage::without_userinfo(&u)),
                    "symbols": symbols.get(&name),
                    "indexed_commit": commits.get(&name),
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let listed: Vec<String> = rows
        .iter()
        .filter_map(|r| r["name"].as_str().map(str::to_string))
        .collect();
    let mine: Vec<String> = Config::load()
        .repos
        .iter()
        .filter_map(|r| {
            r.remote
                .clone()
                .or_else(|| worktree::remote_url(Path::new(&r.path)))
        })
        .map(|u| same_repo(&u))
        .collect();
    let team_missing: Vec<String> = crate::team::current()
        .map(|t| t.repos)
        .unwrap_or_default()
        .into_iter()
        .filter(|u| !mine.contains(&same_repo(u)))
        .collect();
    Json(json!({
        "repos": rows,
        "unlisted": unlisted(&indexed, &meta, &listed),
        "team_missing": team_missing,
        "root": s.data.root,
    }))
}

/// Repositories in the index with no folder in your list: name, symbols, commit and remote URL.
fn unlisted(
    indexed: &std::collections::HashMap<String, usize>,
    meta: &[crate::index::IndexedRepo],
    listed: &[String],
) -> Vec<Value> {
    let mut names: Vec<&String> = indexed.keys().collect();
    names.sort();
    names
        .into_iter()
        .filter(|n| !listed.contains(n))
        .map(|n| {
            let m = meta.iter().find(|r| &r.name == n);
            json!({
                "name": n,
                "symbols": indexed.get(n),
                "commit": m.map(|r| &r.commit),
                "remote": m.and_then(|r| r.remote.as_deref()).map(crate::storage::without_userinfo),
            })
        })
        .collect()
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
                    "remote": worktree::remote_url(&p).map(|u| crate::storage::without_userinfo(&u)),
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

/// Add and remove repositories (folders); removed ones lose comindly's worktree, not your folder.
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
                list.push(RepoEntry { path, remote: None });
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
            Json(
                json!({"ok": true, "message": format!("saved; {n} {} in the list", if n == 1 { "repository" } else { "repositories" })}),
            ),
        ),
        Err(e) => fail(e),
    }
}

/// Stop the running indexer and everything it started.
pub(super) async fn cancel(State(app): State<Shared>) -> impl IntoResponse {
    let mut j = app.job.lock().await;
    match j.pid.filter(|_| j.running) {
        Some(pid) => {
            j.cancelled = true;
            super::stop_group(pid);
            (
                StatusCode::OK,
                Json(json!({"ok": true, "message": "indexing cancelled"})),
            )
        }
        None => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": "nothing is indexing"})),
        ),
    }
}

/// An event the server adds to the indexer's own.
fn event(level: &str, kind: &str, message: &str) -> Value {
    json!({
        "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "level": level,
        "kind": kind,
        "message": message,
    })
}

pub(super) async fn job(State(app): State<Shared>) -> Json<Value> {
    let j = app.job.lock().await;
    let from = j.events.len().saturating_sub(400);
    Json(json!({
        "running": j.running,
        "cancelled": j.cancelled,
        "ok": j.ok,
        "events": &j.events[from..],
        "started": j.started,
        "finished": j.finished,
    }))
}

#[derive(Deserialize)]
pub(super) struct IndexQ {
    #[serde(default)]
    llm: bool,
    /// Index without the repositories that have no folder here, dropping them from the index.
    #[serde(default)]
    drop_unlisted: bool,
}

pub(super) async fn start(State(app): State<Shared>, Json(q): Json<IndexQ>) -> impl IntoResponse {
    match begin(&app, q.llm, q.drop_unlisted, None).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": "indexing started"})),
        ),
        Err(e) => fail(e),
    }
}

/// Start `comindly link` over the listed repositories into the loaded index; the page follows
/// `/api/index` and the UI switches to the new index when it succeeds. `why` opens the log.
pub(super) async fn begin(
    app: &Shared,
    llm: bool,
    drop_unlisted: bool,
    why: Option<String>,
) -> anyhow::Result<()> {
    let session = app.current.read().await.clone();
    let root = session.data.root.clone().context("read-only UI")?;
    if !drop_unlisted {
        let indexed = session.data.graph.repos().into_iter().collect();
        let listed: Vec<String> = Config::load()
            .repos
            .iter()
            .map(|r| worktree::name(Path::new(&r.path)))
            .collect();
        let missing: Vec<String> = unlisted(&indexed, &[], &listed)
            .iter()
            .filter_map(|r| r["name"].as_str().map(str::to_string))
            .collect();
        anyhow::ensure!(
            missing.is_empty(),
            "no folder here for {}, which the index has; add them first, or drop them from the index",
            missing.join(", ")
        );
    }
    {
        let mut j = app.job.lock().await;
        anyhow::ensure!(!j.running, "indexing is already running");
        *j = Job {
            running: true,
            started: Some(chrono::Utc::now().to_rfc3339()),
            events: why.into_iter().map(|w| event("info", "note", &w)).collect(),
            ..Default::default()
        };
    }
    let mut cmd = tokio::process::Command::new(std::env::current_exe()?);
    cmd.args(["link", "--incremental", "--index-dir", &root]);
    if !llm {
        cmd.arg("--no-llm");
    }
    // A server's stored keys are read from the same index location.
    cmd.env("COMINDLY_INDEX_DIR", &root)
        .env("COMINDLY_LOG_FORMAT", "json")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    super::own_group(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            app.job.lock().await.running = false;
            return Err(e.into());
        }
    };
    app.job.lock().await.pid = child.id();
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
                        let l = crate::storage::without_userinfo(&l);
                        let mut e = serde_json::from_str::<Value>(&l)
                            .ok()
                            .filter(Value::is_object)
                            .unwrap_or_else(|| event("info", "note", &l));
                        e["source"] = "index".into();
                        // Also in the server's own output, for the platform's logs.
                        if crate::ui::json_logs() {
                            eprintln!("{e}");
                        } else {
                            eprintln!("index: {}", e["message"].as_str().unwrap_or_default());
                        }
                        app.job.lock().await.events.push(e);
                    }
                }
            }
        };
        let out = child.stdout.take().map(|s| Box::new(s) as Pipe);
        let err = child.stderr.take().map(|s| Box::new(s) as Pipe);
        tokio::join!(follow(out), follow(err));
        let cancelled = app2.job.lock().await.cancelled;
        let ok = !cancelled && child.wait().await.map(|s| s.success()).unwrap_or(false);
        if cancelled {
            let _ = child.wait().await;
            app2.job
                .lock()
                .await
                .events
                .push(event("warn", "warning", "Cancelled."));
        }
        if ok {
            if let Err(e) = app2.reload(&root).await {
                let msg = format!("reload failed: {e:#}");
                app2.job
                    .lock()
                    .await
                    .events
                    .push(event("error", "error", &msg));
            }
        }
        let mut j = app2.job.lock().await;
        j.running = false;
        j.pid = None;
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
                match begin(&app, false, false, Some(why.clone())).await {
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
            if !p.exists() {
                out.push(name);
                continue;
            }
            let (_, head) = worktree::remote_default(p)?;
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

/// A clone URL reduced to host and path, so HTTPS and SSH forms of one repository compare equal.
fn same_repo(url: &str) -> String {
    crate::hosting::managed_path(url)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| url.to_string())
}

#[derive(Deserialize)]
pub(super) struct RemoteListQ {
    url: String,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

/// The repositories of a GitHub organisation or GitLab group, with the ones already here marked.
pub(super) async fn remote_list(
    who: Option<axum::Extension<crate::server::Who>>,
    Json(q): Json<RemoteListQ>,
) -> impl IntoResponse {
    let run = async {
        let source = crate::hosting::parse(&q.url)?;
        let given = q
            .token
            .filter(|t| !t.trim().is_empty())
            .map(|t| worktree::Auth {
                username: q
                    .username
                    .filter(|u| !u.trim().is_empty())
                    .unwrap_or_else(|| worktree::token_user(&source.host).into()),
                token: t.trim().to_string(),
            });
        let host = source.host.clone();
        let auth = match given.clone() {
            Some(a) => Some(a),
            None => tokio::task::spawn_blocking(move || worktree::stored_auth(&host)).await?,
        };
        let repos = crate::hosting::list(&source, auth.as_ref()).await?;
        let mut warning = None;
        if let Some(a) = given {
            let host = source.host.clone();
            match (crate::server::store(), who) {
                (Some(store), Some(w)) => {
                    if let Err(e) =
                        store.set_secret(&w.email, &crate::server::git_secret(&host), &a.token)
                    {
                        warning = Some(format!("the token worked but was not kept: {e:#}"));
                    }
                }
                _ => tokio::task::spawn_blocking(move || worktree::approve(&host, &a)).await?,
            }
        }
        let here: Vec<String> = Config::load()
            .repos
            .iter()
            .filter_map(|r| {
                r.remote
                    .clone()
                    .or_else(|| worktree::remote_url(Path::new(&r.path)))
            })
            .map(|u| same_repo(&u))
            .collect();
        let rows: Vec<Value> = repos
            .iter()
            .map(|r| {
                let mut v = json!(r);
                v["listed"] = json!(here.contains(&same_repo(&r.clone_url)));
                v
            })
            .collect();
        anyhow::Ok(json!({
            "ok": true,
            "provider": source.provider,
            "host": source.host,
            "owner": source.owner,
            "repos": rows,
            "warning": warning,
        }))
    };
    match run.await {
        Ok(v) => (StatusCode::OK, Json(v)),
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

#[derive(Deserialize)]
pub(super) struct RemoteAddQ {
    urls: Vec<String>,
    #[serde(default)]
    share: bool,
}

/// Add repositories by clone URL; comindly clones them into its own folder when indexing.
pub(super) async fn remote_add(Cur(s): Cur, Json(q): Json<RemoteAddQ>) -> impl IntoResponse {
    let run = async {
        let mut list = Config::load().repos;
        let mut added = 0;
        for url in &q.urls {
            let path = crate::hosting::managed_path(url)?
                .to_string_lossy()
                .into_owned();
            if !list.iter().any(|r| r.path == path) {
                list.push(RepoEntry {
                    path,
                    remote: Some(url.clone()),
                });
                added += 1;
            }
        }
        crate::config::set_repos(&list)?;
        let mut shared = 0;
        if q.share {
            let root = s.data.root.clone().context("no index to share with")?;
            let stored = crate::team::load(&root).await?;
            let mut settings = stored.settings;
            for url in &q.urls {
                if !settings
                    .repos
                    .iter()
                    .any(|u| same_repo(u) == same_repo(url))
                {
                    settings.repos.push(url.clone());
                    shared += 1;
                }
            }
            if shared > 0 {
                crate::team::save(&root, &settings, stored.version).await?;
                crate::team::set_current(settings);
            }
        }
        anyhow::Ok(match (added, q.share) {
            (_, true) => format!(
                "added {added}, {shared} new for the team; press Index now to clone and index them"
            ),
            _ => format!("added {added}; press Index now to clone and index them"),
        })
    };
    match run.await {
        Ok(m) => (StatusCode::OK, Json(json!({"ok": true, "message": m}))),
        Err(e) => fail(e),
    }
}
