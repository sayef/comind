//! AWS sign-in from the app: say when the SSO session for an S3 index has expired, and run
//! `aws sso login` on request (or by itself with `sso_auto_login`); you still approve it in the browser.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use super::Shared;
use crate::config::Config;

/// The current or last sign-in.
#[derive(Default, Clone, serde::Serialize)]
pub(super) struct SignIn {
    running: bool,
    pid: Option<u32>,
    cancelled: bool,
    ok: Option<bool>,
    message: String,
    started: Option<String>,
}

fn profile() -> Option<String> {
    std::env::var("AWS_PROFILE").ok().filter(|p| !p.is_empty())
}

pub(super) async fn status(State(app): State<Shared>) -> Json<Value> {
    let root = app
        .current
        .read()
        .await
        .data
        .root
        .clone()
        .unwrap_or_default();
    if !root.starts_with("s3://") {
        return Json(json!({ "needed": false }));
    }
    let (ok, message) = match tokio::task::spawn_blocking(crate::s3::check).await {
        Ok(Ok(m)) => (true, m),
        Ok(Err(m)) => (false, m),
        Err(_) => (false, "credential check failed".to_string()),
    };
    let signin = !ok && message.contains("aws sso login");
    let auto = Config::load().sso_auto_login == Some(true);
    if signin && auto {
        let recent = app.signin.lock().await.started.as_deref().is_some_and(|t| {
            chrono::DateTime::parse_from_rfc3339(t)
                .is_ok_and(|t| chrono::Utc::now() - t.to_utc() < chrono::Duration::minutes(10))
        });
        if !recent {
            let _ = login(&app).await;
        }
    }
    let run = app.signin.lock().await.clone();
    Json(json!({
        "needed": true,
        "ok": ok,
        "message": message,
        "signin": signin,
        "profile": profile(),
        "auto": auto,
        "run": run,
    }))
}

pub(super) async fn start(State(app): State<Shared>) -> impl IntoResponse {
    match login(&app).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"ok": true, "message": "approve the sign-in in your browser"})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "message": super::error_text(&e)})),
        ),
    }
}

/// Stop a sign-in that is waiting for approval.
pub(super) async fn cancel(State(app): State<Shared>) -> impl IntoResponse {
    let mut s = app.signin.lock().await;
    if let Some(pid) = s.pid.filter(|_| s.running) {
        s.cancelled = true;
        super::stop_group(pid);
    }
    (
        StatusCode::OK,
        Json(json!({"ok": true, "message": "sign-in cancelled"})),
    )
}

/// Run `aws sso login` for the active profile in the background.
async fn login(app: &Shared) -> anyhow::Result<()> {
    {
        let mut s = app.signin.lock().await;
        anyhow::ensure!(!s.running, "a sign-in is already waiting for approval");
        *s = SignIn {
            running: true,
            started: Some(chrono::Utc::now().to_rfc3339()),
            ..Default::default()
        };
    }
    let mut cmd = tokio::process::Command::new("aws");
    cmd.args(["sso", "login"]);
    if let Some(p) = profile() {
        cmd.args(["--profile", &p]);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    super::own_group(&mut cmd);
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let mut s = app.signin.lock().await;
            s.running = false;
            s.ok = Some(false);
            s.message = "the AWS CLI (`aws`) is needed to sign in".into();
            anyhow::bail!("the AWS CLI (`aws`) is needed to sign in: {e}");
        }
    };
    app.signin.lock().await.pid = child.id();
    let app = app.clone();
    tokio::spawn(async move {
        let out = child.wait_with_output().await;
        let signed_in = matches!(&out, Ok(o) if o.status.success());
        // An index that could not be read before the sign-in is read again now.
        let session = app.current.read().await.clone();
        if signed_in && session.data.problem.is_some() {
            if let Some(root) = session.data.root.clone() {
                let _ = app.reload(&root).await;
            }
        }
        let mut s = app.signin.lock().await;
        s.running = false;
        s.pid = None;
        if s.cancelled {
            s.ok = Some(false);
            s.message = "sign-in cancelled".into();
            return;
        }
        match out {
            Ok(o) if o.status.success() => {
                s.ok = Some(true);
                s.message = "signed in".into();
            }
            Ok(o) => {
                s.ok = Some(false);
                let err = String::from_utf8_lossy(&o.stderr);
                s.message = err.lines().last().unwrap_or("sign-in failed").to_string();
            }
            Err(e) => {
                s.ok = Some(false);
                s.message = e.to_string();
            }
        }
    });
    Ok(())
}
