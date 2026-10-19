//! Settings from the UI: read and write `config.toml`, and switch the
//! loaded index without a restart.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{Cur, Shared};
use crate::config::{Config, Setting};

fn fail(e: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"ok": false, "message": format!("{e:#}")})),
    )
}

pub(super) async fn get(Cur(s): Cur) -> Json<Value> {
    let cfg = Config::load();
    let root = s.data.root.clone().unwrap_or_default();
    let remote = root.starts_with("s3://");
    let (aws, key) = tokio::task::spawn_blocking(move || {
        let aws = remote.then(crate::s3::check);
        let key = if std::env::var_os("OPENAI_API_KEY").is_some() {
            "environment"
        } else {
            "none"
        };
        (aws, key)
    })
    .await
    .unwrap_or((None, "none"));
    Json(json!({
        "config_path": crate::config::config_path(),
        "index_dir": cfg.index_dir,
        "index_effective": root,
        "aws_profile": cfg.aws_profile,
        "aws_profile_env": std::env::var("AWS_PROFILE").ok(),
        "aws_profiles": crate::s3::profiles(),
        "aws_status": aws.map(|r| match r { Ok(m) => json!({"ok": true, "message": m}), Err(m) => json!({"ok": false, "message": m}) }),
        "author_name": cfg.author_name,
        "author_effective": crate::ledger::author_name(),
        "record_names": cfg.record_names.unwrap_or(true),
        "llm_base_url": cfg.llm_base_url,
        "llm_model": cfg.llm_model(),
        "defaults": super::team::defaults(),
        "llm_key": key,
        "cache_ttl_secs": cfg.cache_ttl_secs.unwrap_or(crate::cache::DEFAULT_TTL),
        "auto_index": cfg.auto_index.unwrap_or(false),
        "sso_auto_login": cfg.sso_auto_login.unwrap_or(false),
    }))
}

#[derive(Deserialize)]
pub(super) struct Changes {
    index_dir: Option<String>,
    aws_profile: Option<String>,
    author_name: Option<String>,
    record_names: Option<bool>,
    llm_base_url: Option<String>,
    llm_model: Option<String>,
    cache_ttl_secs: Option<u64>,
    auto_index: Option<bool>,
    sso_auto_login: Option<bool>,
}

/// Save the given settings; a new index location is loaded before it is saved.
pub(super) async fn save(State(app): State<Shared>, Json(c): Json<Changes>) -> impl IntoResponse {
    let run = async {
        let text = |v: &Option<String>| match v.as_deref().map(str::trim) {
            Some("") => Setting::Remove,
            Some(t) => Setting::Text(t.to_string()),
            None => unreachable!(),
        };
        let mut changes: Vec<(&str, Setting)> = Vec::new();
        for (key, v) in [
            ("aws_profile", &c.aws_profile),
            ("author_name", &c.author_name),
            ("llm_base_url", &c.llm_base_url),
            ("llm_model", &c.llm_model),
        ] {
            if v.is_some() {
                changes.push((key, text(v)));
            }
        }
        for (key, v) in [
            ("record_names", c.record_names),
            ("auto_index", c.auto_index),
            ("sso_auto_login", c.sso_auto_login),
        ] {
            if let Some(v) = v {
                changes.push((key, Setting::Flag(v)));
            }
        }
        if let Some(v) = c.cache_ttl_secs {
            changes.push(("cache_ttl_secs", Setting::Number(v.max(30) as i64)));
        }
        let mut message = "saved".to_string();
        if let Some(dir) = c
            .index_dir
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            let dir = crate::config::expand_tilde(dir);
            app.reload(&dir).await?;
            changes.push(("index_dir", Setting::Text(dir.clone())));
            message = format!("saved; now showing {dir}");
        }
        crate::config::update(&changes)?;
        if c.aws_profile.is_some() {
            message.push_str(" (the AWS profile applies when comind starts next)");
        }
        anyhow::Ok(message)
    };
    match run.await {
        Ok(m) => (StatusCode::OK, Json(json!({"ok": true, "message": m}))),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
pub(super) struct Probe {
    index_dir: String,
}

/// Whether an index location can be read, without switching to it.
pub(super) async fn test(Json(p): Json<Probe>) -> impl IntoResponse {
    let root = crate::config::expand_tilde(p.index_dir.trim());
    let graph = format!("{}/_graph", root.trim_end_matches('/'));
    match crate::index::graph_versions(&graph).await {
        Ok(v) if !v.is_empty() => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": format!("index found: {} version(s)", v.len())})),
        ),
        Ok(_) | Err(_) if !root.contains("://") && !std::path::Path::new(&root).exists() => (
            StatusCode::OK,
            Json(
                json!({"ok": true, "message": "empty location: an index will be created here when you index repositories"}),
            ),
        ),
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": "reachable, no index yet"})),
        ),
        Err(e) => fail(e),
    }
}
