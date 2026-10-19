//! Team settings from the UI, plus your own API key in your credentials file (never in the index).

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

pub(super) async fn get(State(app): State<Shared>) -> impl IntoResponse {
    let run = async {
        let root = root(&app).await?;
        let stored = team::load(&root).await?;
        let key = crate::credentials::llm_key(stored.settings.name.as_deref()).1;
        anyhow::Ok(json!({
            "settings": stored.settings,
            "key_source": key,
            "version": stored.version,
            "changed_by": stored.changed_by,
            "changed_at": stored.changed_at,
            "defaults": defaults(),
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

/// Save the shared settings for everyone using this index.
pub(super) async fn save(State(app): State<Shared>, Json(b): Json<Save>) -> impl IntoResponse {
    let run = async {
        let root = root(&app).await?;
        let current = team::load(&root).await?;
        let mut next = b.settings;
        for v in [
            &mut next.name,
            &mut next.llm_base_url,
            &mut next.llm_model,
            &mut next.embed_model,
        ] {
            if v.as_deref().is_some_and(|s| s.trim().is_empty()) {
                *v = None;
            }
        }
        let reindex = next.embed_model != current.settings.embed_model;
        team::save(&root, &next, b.version).await?;
        team::set_current(next);
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
}

/// Store your API key for this team (or as your default) in your credentials file.
pub(super) async fn key(State(app): State<Shared>, Json(b): Json<Key>) -> impl IntoResponse {
    let run = async {
        let root = root(&app).await?;
        let name = team::load(&root).await?.settings.name;
        let (n, k) = (name.clone(), b.key.clone());
        tokio::task::spawn_blocking(move || crate::credentials::set_llm_key(n.as_deref(), &k))
            .await??;
        anyhow::Ok(match name {
            Some(n) if !b.key.trim().is_empty() => format!("saved as your key for {n}"),
            None if !b.key.trim().is_empty() => "saved as your default key".to_string(),
            _ => "removed".to_string(),
        })
    };
    match run.await {
        Ok(m) => (StatusCode::OK, Json(json!({"ok": true, "message": m}))),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
pub(super) struct Join {
    index: String,
    /// AWS profile for an S3 index (credentials stay in ~/.aws).
    #[serde(default)]
    aws_profile: Option<String>,
}

/// Use a team's index: check it, remember it (and the AWS profile) and load it.
pub(super) async fn join(State(app): State<Shared>, Json(b): Json<Join>) -> impl IntoResponse {
    let run = async {
        let root = crate::config::expand_tilde(b.index.trim());
        let profile = b.aws_profile.filter(|p| !p.trim().is_empty());
        if let Some(p) = &profile {
            crate::s3::use_profile(p.trim());
        }
        let graph = format!("{}/_graph", root.trim_end_matches('/'));
        let versions = crate::index::graph_versions(&graph).await?;
        anyhow::ensure!(!versions.is_empty(), "no index at {root}");
        let stored = team::load(&root).await?;
        let mut personal = vec![("index_dir", crate::config::Setting::Text(root.clone()))];
        if let Some(p) = profile {
            personal.push((
                "aws_profile",
                crate::config::Setting::Text(p.trim().to_string()),
            ));
        }
        crate::config::update(&personal)?;
        team::set_current(stored.settings.clone());
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
