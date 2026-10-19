//! Team settings from the UI. Secrets never leave the server: responses only say whether a key is
//! set and whether this machine can unlock it; a generated team password is returned once.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::Shared;
use crate::team::{self, TeamSettings};

fn fail(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"ok": false, "message": format!("{e:#}")})),
    )
}

async fn root(app: &Shared) -> anyhow::Result<String> {
    use anyhow::Context as _;
    app.current
        .read()
        .await
        .data
        .root
        .clone()
        .context("read-only UI")
}

/// comind's built-in values, shown where a setting is left empty.
pub(super) fn defaults() -> Value {
    json!({
        "llm_model": crate::llm::DEFAULT_MODEL,
        "llm_base_url": crate::llm::DEFAULT_BASE_URL,
        "embed_model": crate::embed::DEFAULT_MODEL,
    })
}

/// Make `settings` the ones this process uses, unlocked when the password is known.
fn activate(settings: &TeamSettings) {
    team::set_current(team::unlock(settings, team::password(settings).as_deref()));
}

pub(super) async fn get(State(app): State<Shared>) -> impl IntoResponse {
    let run = async {
        let root = root(&app).await?;
        let stored = team::load(&root).await?;
        let s = stored.settings.clone();
        let active =
            tokio::task::spawn_blocking(move || team::unlock(&s, team::password(&s).as_deref()))
                .await?;
        let mut shown = stored.settings.clone();
        shown.sealed = None;
        anyhow::Ok(json!({
            "settings": shown,
            "has_key": stored.settings.sealed.is_some(),
            "unlocked": active.secrets.is_some(),
            "locked": active.locked,
            "version": stored.version,
            "defaults": defaults(),
            "changed_by": stored.changed_by,
            "changed_at": stored.changed_at,
        }))
    };
    match run.await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
pub(super) struct Save {
    settings: TeamSettings,
    version: Option<u64>,
}

/// Save the shared settings (secrets are kept as they are).
pub(super) async fn save(State(app): State<Shared>, Json(b): Json<Save>) -> impl IntoResponse {
    let run = async {
        let root = root(&app).await?;
        let current = team::load(&root).await?;
        let mut next = b.settings;
        next.sealed = current.settings.sealed.clone();
        let blank = |v: &mut Option<String>| {
            if v.as_deref().is_some_and(|s| s.trim().is_empty()) {
                *v = None;
            }
        };
        for v in [
            &mut next.name,
            &mut next.llm_base_url,
            &mut next.llm_model,
            &mut next.embed_model,
        ] {
            blank(v);
        }
        let reindex = next.embed_model != current.settings.embed_model;
        team::save(&root, &next, b.version).await?;
        tokio::task::spawn_blocking(move || activate(&next)).await?;
        anyhow::Ok(if reindex {
            "saved; the new embedding model needs a full re-index"
        } else {
            "saved for everyone using this index"
        })
    };
    match run.await {
        Ok(m) => (StatusCode::OK, Json(json!({"ok": true, "message": m}))),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
pub(super) struct Key {
    key: String,
    #[serde(default)]
    password: Option<String>,
}

/// Store the team's LLM key; the first time, a team password is generated and returned once.
pub(super) async fn key(State(app): State<Shared>, Json(b): Json<Key>) -> impl IntoResponse {
    let run = async {
        let root = root(&app).await?;
        let stored = team::load(&root).await?;
        let mut s = stored.settings.clone();
        let (s, generated) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let (mut secrets, pw, generated) = match &s.sealed {
                Some(sealed) => {
                    let pw = b
                        .password
                        .filter(|p| !p.is_empty())
                        .or_else(|| team::password(&s));
                    let pw = pw.ok_or_else(|| anyhow::anyhow!("enter the team password"))?;
                    (team::open(sealed, &pw)?, pw, None)
                }
                None => {
                    let pw = team::passphrase()?;
                    (team::Secrets::default(), pw.clone(), Some(pw))
                }
            };
            anyhow::ensure!(!b.key.trim().is_empty(), "enter the API key");
            anyhow::ensure!(
                s.name.as_deref().is_some_and(|n| !n.trim().is_empty()),
                "give the team a name first (it is how members' machines find the password)"
            );
            secrets.llm_api_key = Some(b.key.trim().to_string());
            s.sealed = Some(team::seal(&secrets, &pw)?);
            team::remember_password(&s, &pw)?;
            Ok((s, generated))
        })
        .await??;
        team::save(&root, &s, stored.version).await?;
        tokio::task::spawn_blocking(move || activate(&s)).await?;
        anyhow::Ok(generated)
    };
    match run.await {
        Ok(generated) => (
            StatusCode::OK,
            Json(
                json!({"ok": true, "message": "team key saved, encrypted", "password": generated}),
            ),
        ),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
pub(super) struct Unlock {
    password: String,
}

/// Check the team password and remember it on this machine.
pub(super) async fn unlock(State(app): State<Shared>, Json(b): Json<Unlock>) -> impl IntoResponse {
    let run = async {
        let root = root(&app).await?;
        let stored = team::load(&root).await?;
        let sealed = stored
            .settings
            .sealed
            .clone()
            .ok_or_else(|| anyhow::anyhow!("this team has no key yet"))?;
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            team::open(&sealed, b.password.trim())?;
            team::remember_password(&stored.settings, b.password.trim())?;
            activate(&stored.settings);
            Ok(())
        })
        .await??;
        anyhow::Ok(())
    };
    match run.await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": "unlocked on this machine"})),
        ),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
pub(super) struct Join {
    index: String,
    #[serde(default)]
    password: Option<String>,
}

/// Use a team's index: check it, unlock its key when a password is given, remember and load it.
pub(super) async fn join(State(app): State<Shared>, Json(b): Json<Join>) -> impl IntoResponse {
    let run = async {
        let root = crate::config::expand_tilde(b.index.trim());
        let graph = format!("{}/_graph", root.trim_end_matches('/'));
        let versions = crate::index::graph_versions(&graph).await?;
        anyhow::ensure!(!versions.is_empty(), "no index at {root}");
        let stored = team::load(&root).await?;
        if let (Some(sealed), Some(pw)) = (
            &stored.settings.sealed,
            b.password.filter(|p| !p.trim().is_empty()),
        ) {
            let (sealed, s) = (sealed.clone(), stored.settings.clone());
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                team::open(&sealed, pw.trim())?;
                team::remember_password(&s, pw.trim())
            })
            .await??;
        }
        crate::config::update(&[("index_dir", crate::config::Setting::Text(root.clone()))])?;
        let s = stored.settings.clone();
        tokio::task::spawn_blocking(move || activate(&s)).await?;
        app.reload(&root).await?;
        anyhow::Ok(stored.settings.name.unwrap_or(root))
    };
    match run.await {
        Ok(name) => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": format!("joined {name}")})),
        ),
        Err(e) => fail(e),
    }
}
