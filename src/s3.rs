//! S3 and S3-compatible storage through the standard AWS chain: environment, `~/.aws` profiles
//! (keys, SSO, `credential_process`, assumed roles), web identity and instance roles. Region and
//! endpoint come from the same places (`AWS_REGION`, `AWS_ENDPOINT_URL`, a profile's `region` and
//! `endpoint_url`), so MinIO or R2 need nothing comind-specific.

use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use object_store::aws::{AmazonS3Builder, AwsCredential};
use object_store::CredentialProvider;

/// What the AWS chain resolved for this process.
pub struct Aws {
    pub region: Option<String>,
    pub endpoint: Option<String>,
    credentials: Option<SharedCredentialsProvider>,
}

/// The chain's view, loaded once (on its own thread, so any caller context works).
fn cache() -> &'static RwLock<Option<Arc<Aws>>> {
    static AWS: OnceLock<RwLock<Option<Arc<Aws>>>> = OnceLock::new();
    AWS.get_or_init(|| RwLock::new(None))
}

pub fn aws() -> Arc<Aws> {
    if let Some(a) = cache().read().ok().and_then(|g| g.clone()) {
        return a;
    }
    let a = Arc::new(load());
    if let Ok(mut g) = cache().write() {
        *g = Some(a.clone());
    }
    a
}

/// Use AWS profile `name` from now on in this process (e.g. after joining a team in the app).
pub fn use_profile(name: &str) {
    std::env::set_var("AWS_PROFILE", name);
    if let Ok(mut g) = cache().write() {
        *g = None;
    }
}

fn load() -> Aws {
    {
        std::thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()?;
            let sdk = rt.block_on(aws_config::defaults(BehaviorVersion::latest()).load());
            Some(Aws {
                region: sdk.region().map(|r| r.to_string()),
                endpoint: sdk.endpoint_url().map(str::to_string),
                credentials: sdk.credentials_provider(),
            })
        })
        .join()
        .ok()
        .flatten()
        .unwrap_or(Aws {
            region: None,
            endpoint: None,
            credentials: None,
        })
    }
}

/// LanceDB storage options for `uri`: region and endpoint from the chain (credentials it finds itself).
pub fn storage_options(uri: &str) -> Vec<(String, String)> {
    if !uri.starts_with("s3://") {
        return Vec::new();
    }
    let aws = aws();
    let mut out = Vec::new();
    if let Some(r) = &aws.region {
        out.push(("aws_region".into(), r.clone()));
    }
    if let Some(e) = &aws.endpoint {
        out.push(("aws_endpoint".into(), e.clone()));
        out.push(("allow_http".into(), e.starts_with("http://").to_string()));
    }
    out
}

/// A LanceDB connection builder with the chain's region and endpoint applied.
pub fn connect(uri: &str) -> lancedb::connection::ConnectBuilder {
    let mut b = lancedb::connect(uri);
    for (k, v) in storage_options(uri) {
        b = b.storage_option(k, v);
    }
    b
}

/// An object store for an S3 bucket, authenticated through the chain.
pub fn bucket(name: &str) -> Result<object_store::aws::AmazonS3> {
    let aws = aws();
    let mut b = AmazonS3Builder::new().with_bucket_name(name);
    if let Some(r) = &aws.region {
        b = b.with_region(r);
    }
    if let Some(e) = &aws.endpoint {
        b = b.with_endpoint(e).with_allow_http(e.starts_with("http://"));
    }
    if let Some(p) = &aws.credentials {
        b = b.with_credentials(Arc::new(Chain {
            provider: p.clone(),
            cached: Mutex::new(None),
        }));
    }
    b.build().context("open S3 store")
}

/// Hands the chain's (refreshing) credentials to `object_store`.
#[derive(Debug)]
struct Chain {
    provider: SharedCredentialsProvider,
    cached: Mutex<Option<(Arc<AwsCredential>, Option<SystemTime>)>>,
}

#[async_trait::async_trait]
impl CredentialProvider for Chain {
    type Credential = AwsCredential;

    async fn get_credential(&self) -> object_store::Result<Arc<AwsCredential>> {
        let fresh = |exp: &Option<SystemTime>| {
            exp.is_none_or(|t| t > SystemTime::now() + Duration::from_secs(300))
        };
        if let Some((c, exp)) = self.cached.lock().ok().and_then(|g| g.clone()) {
            if fresh(&exp) {
                return Ok(c);
            }
        }
        let c = self.provider.provide_credentials().await.map_err(|e| {
            object_store::Error::Generic {
                store: "S3",
                source: Box::new(e),
            }
        })?;
        let cred = Arc::new(AwsCredential {
            key_id: c.access_key_id().to_string(),
            secret_key: c.secret_access_key().to_string(),
            token: c.session_token().map(str::to_string),
        });
        if let Ok(mut g) = self.cached.lock() {
            *g = Some((cred.clone(), c.expiry()));
        }
        Ok(cred)
    }
}

/// When keys in the environment carry an expiry that has passed, the hint to drop them.
fn expired_env_keys() -> Option<String> {
    std::env::var_os("AWS_ACCESS_KEY_ID")?;
    let at = std::env::var("AWS_CREDENTIAL_EXPIRATION").ok()?;
    let t = chrono::DateTime::parse_from_rfc3339(&at).ok()?;
    (t < chrono::Utc::now()).then(|| {
        format!(
            "the AWS keys in comind's environment (AWS_ACCESS_KEY_ID) expired at {at} and win over your \
             profile; start comind from a shell without them"
        )
    })
}

/// A short, secret-free explanation of an error, with S3 error replies turned into what to do.
pub fn explain(raw: &str) -> String {
    let code = raw
        .split("<Code>")
        .nth(1)
        .and_then(|r| r.split("</Code>").next())
        .map(str::to_string);
    let hint = match code.as_deref() {
        Some("ExpiredToken") | Some("TokenRefreshRequired") => {
            Some(expired_env_keys().unwrap_or_else(|| {
                "your AWS credentials have expired; sign in again (`aws sso login`)".into()
            }))
        }
        Some("AccessDenied") => Some(
            "AWS denied access; check the AWS profile and its permissions on the bucket".into(),
        ),
        Some("NoSuchBucket") => {
            Some("the S3 bucket does not exist (check the name and region)".into())
        }
        Some("InvalidAccessKeyId") | Some("SignatureDoesNotMatch") | Some("InvalidToken") => {
            Some(expired_env_keys().unwrap_or_else(|| {
                "AWS rejected the credentials; check the AWS profile or sign in again".into()
            }))
        }
        _ => None,
    };
    match (hint, code) {
        (Some(h), Some(c)) => format!("{h} (S3: {c})"),
        _ => redact(raw),
    }
}

/// Removes session tokens and other long credential-like strings.
fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut word = String::new();
    let flush = |w: &mut String, out: &mut String| {
        let has = |f: fn(&char) -> bool| w.chars().any(|c| f(&c));
        let secretish = w.len() >= 60
            && has(char::is_ascii_uppercase)
            && has(char::is_ascii_lowercase)
            && has(char::is_ascii_digit);
        out.push_str(if secretish { "[redacted]" } else { w.as_str() });
        w.clear();
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "+/=_-%".contains(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Whether the chain can produce credentials now, with a hint when it cannot.
pub fn check() -> Result<String, String> {
    if let Some(hint) = expired_env_keys() {
        return Err(hint);
    }
    let Some(p) = aws().credentials.clone() else {
        return Err("no AWS credentials found (environment, ~/.aws profiles, SSO)".into());
    };
    let profile = std::env::var("AWS_PROFILE").unwrap_or_else(|_| "default".into());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(p.provide_credentials())
            .map(|_| format!("AWS credentials resolve (profile `{profile}`)"))
            .map_err(|e| {
                let mut msg = e.to_string();
                let mut cause = std::error::Error::source(&e);
                while let Some(c) = cause {
                    msg.push_str(": ");
                    msg.push_str(&c.to_string());
                    cause = c.source();
                }
                let lower = msg.to_lowercase();
                if lower.contains("sso") || lower.contains("token") {
                    format!("AWS sign-in needed: `aws sso login --profile {profile}`")
                } else {
                    format!("AWS credentials for profile `{profile}` do not resolve: {msg}")
                }
            })
    })
    .join()
    .unwrap_or_else(|_| Err("AWS credential check failed".into()))
}

/// Profile names from the AWS config and credentials files (nothing else is read from them).
pub fn profiles() -> Vec<String> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let file = |var: &str, default: &str| {
        std::env::var_os(var)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".aws").join(default))
    };
    let mut names = std::collections::BTreeSet::new();
    for (path, prefixed) in [
        (file("AWS_CONFIG_FILE", "config"), true),
        (file("AWS_SHARED_CREDENTIALS_FILE", "credentials"), false),
    ] {
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            let Some(section) = line
                .trim()
                .strip_prefix('[')
                .and_then(|l| l.strip_suffix(']'))
            else {
                continue;
            };
            let name = match (prefixed, section.strip_prefix("profile ")) {
                (true, Some(n)) => n.trim(),
                (true, None) if section.trim() == "default" => "default",
                (false, None) => section.trim(),
                _ => continue,
            };
            names.insert(name.to_string());
        }
    }
    names.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_s3_replies_without_secrets() {
        let token = &"ExampleSessionToken0123456789".repeat(3);
        let raw = format!(
            "ListRequest: <Error><Code>ExpiredToken</Code><Token-0>{token}</Token-0></Error>"
        );
        let e = explain(&raw);
        assert!(
            e.contains("(S3: ExpiredToken)") && !e.contains(token),
            "{e}"
        );
        let other = explain(&format!("connect failed: token={token} at s3://bucket/x"));
        assert!(
            !other.contains(token) && other.contains("s3://bucket/x"),
            "{other}"
        );
        let path = format!("/home/{}", "long-folder-name-".repeat(6));
        assert_eq!(explain(&path), path);
    }
}
