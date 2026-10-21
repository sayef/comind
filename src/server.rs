//! Server mode (on when `COMINDLY_PUBLIC_URL` is set): members, invitations, sign-in links,
//! sessions, personal access tokens and encrypted secrets, in SQLite next to comindly's data.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// `NAME` from the environment, or the contents of the file `NAME_FILE` names; read once.
pub fn env_secret(name: &str) -> Option<String> {
    static READ: std::sync::OnceLock<Mutex<std::collections::HashMap<String, Option<String>>>> =
        std::sync::OnceLock::new();
    let cache = READ.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(name).cloned()) {
        return v;
    }
    let value = read_secret(name);
    if let Ok(mut c) = cache.lock() {
        c.insert(name.to_string(), value.clone());
    }
    value
}

fn read_secret(name: &str) -> Option<String> {
    if let Some(v) = std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
        return Some(v.trim().to_string());
    }
    let path = std::env::var(format!("{name}_FILE")).ok()?;
    match std::fs::read_to_string(&path) {
        Ok(v) => Some(v.trim().to_string()).filter(|v| !v.is_empty()),
        Err(e) => {
            eprintln!("comindly: {name}_FILE ({path}) cannot be read: {e}");
            None
        }
    }
}

/// The address members reach this server at, without a trailing slash.
pub fn public_url() -> Option<String> {
    env_secret("COMINDLY_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string())
}

pub fn enabled() -> bool {
    public_url().is_some()
}

/// Admins named by the deployment; always admins, whatever the database says.
pub fn admin_emails() -> Vec<String> {
    env_secret("COMINDLY_ADMIN_EMAILS")
        .unwrap_or_default()
        .split([',', ' ', '\n'])
        .map(normalize)
        .filter(|e| e.contains('@'))
        .collect()
}

/// How members sign in: everything by email, or everything by codes admins hand out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignIn {
    Email,
    Code,
}

/// Email when the server can send it, else codes; `COMINDLY_SIGN_IN=email|code` decides when set.
pub fn sign_in() -> Result<SignIn> {
    match env_secret("COMINDLY_SIGN_IN").as_deref() {
        Some("email") => {
            anyhow::ensure!(
                mail_configured(),
                "COMINDLY_SIGN_IN=email needs COMINDLY_SMTP_URL"
            );
            Ok(SignIn::Email)
        }
        Some("code") => Ok(SignIn::Code),
        Some(other) => bail!("COMINDLY_SIGN_IN is `email` or `code`, not `{other}`"),
        None if mail_configured() => Ok(SignIn::Email),
        None => Ok(SignIn::Code),
    }
}

pub fn mode() -> SignIn {
    sign_in().unwrap_or(SignIn::Code)
}

pub fn normalize(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn db_path() -> PathBuf {
    crate::config::env("COMINDLY_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(crate::config::data_dir()).join("server.db"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Member,
}

impl Role {
    fn parse(s: &str) -> Role {
        if s == "admin" {
            Role::Admin
        } else {
            Role::Member
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }
}

/// How a request proved who it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Session,
    AdminToken,
    /// A personal access token: enough for agents, not for managing the account.
    Token,
}

/// Who a request comes from.
#[derive(Debug, Clone, Serialize)]
pub struct Who {
    pub email: String,
    pub role: Role,
}

#[derive(Debug, Serialize)]
pub struct Member {
    pub email: String,
    pub role: Role,
    pub fixed: bool,
    pub created: String,
    pub last_seen: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Invite {
    pub email: String,
    pub role: Role,
    pub created_by: String,
    pub expires: String,
}

#[derive(Debug, Serialize)]
pub struct AccessToken {
    pub id: String,
    pub name: String,
    pub created: String,
    pub last_used: Option<String>,
}

/// A secret's state for the UI; its value never leaves the server.
#[derive(Debug, Serialize)]
pub struct SecretInfo {
    pub name: String,
    pub source: &'static str,
    pub hint: Option<String>,
    pub updated: Option<String>,
    pub updated_by: Option<String>,
}

const SESSION_DAYS: i64 = 30;
const LINK_MINUTES: i64 = 15;
const INVITE_DAYS: i64 = 7;
const MAX_FAILED: i64 = 5;
const MAX_REDEEM_FAILED: i64 = 20;
const LOCK_MINUTES: i64 = 15;
const MAX_LINKS_PER_HOUR: i64 = 5;
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub struct Store {
    db: Mutex<Connection>,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

/// A random URL-safe token with `prefix`.
pub fn new_token(prefix: &str) -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("system randomness");
    format!("{prefix}{}", blake3::Hash::from(b).to_hex())
}

/// A sign-in or invitation code people type: 20 characters (100 bits) in groups of four.
pub fn new_code() -> String {
    let mut b = [0u8; 20];
    getrandom::fill(&mut b).expect("system randomness");
    let chars: Vec<char> = b
        .iter()
        .map(|x| CODE_ALPHABET[(*x as usize) % CODE_ALPHABET.len()] as char)
        .collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// A code as typed, without case, spaces or dashes mattering.
fn code_hash(code: &str) -> String {
    let plain: String = code
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    hash(&format!("code:{plain}"))
}

fn hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn cipher(key: &str) -> XChaCha20Poly1305 {
    let k = blake3::derive_key("comindly secrets v1", key.as_bytes());
    XChaCha20Poly1305::new((&k).into())
}

/// Encrypt `value`, bound to the secret's `name` so a value cannot be moved to another name.
fn seal(key: &str, name: &str, value: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).context("randomness")?;
    let payload = Payload {
        msg: value.as_bytes(),
        aad: name.as_bytes(),
    };
    let sealed = cipher(key)
        .encrypt(XNonce::from_slice(&nonce), payload)
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    Ok((sealed, nonce.to_vec()))
}

fn open(key: &str, name: &str, sealed: &[u8], nonce: &[u8]) -> Option<String> {
    let payload = Payload {
        msg: sealed,
        aad: name.as_bytes(),
    };
    let plain = cipher(key)
        .decrypt(XNonce::from_slice(nonce), payload)
        .ok()?;
    String::from_utf8(plain).ok()
}

/// The last four characters, to recognise a token without showing it.
fn hint(value: &str) -> String {
    let tail: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Owner-only from the start; SQLite gives its -wal and -shm files the same mode.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("create {}", path.display()))?;
        }
        let db = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS users (email TEXT PRIMARY KEY, role TEXT NOT NULL,
                 created TEXT NOT NULL, last_seen TEXT);
             CREATE TABLE IF NOT EXISTS invites (hash TEXT PRIMARY KEY, email TEXT NOT NULL,
                 role TEXT NOT NULL, created_by TEXT NOT NULL, created TEXT NOT NULL,
                 expires TEXT NOT NULL, used TEXT);
             CREATE TABLE IF NOT EXISTS links (hash TEXT PRIMARY KEY, email TEXT NOT NULL,
                 created TEXT NOT NULL, expires TEXT NOT NULL, used TEXT);
             CREATE TABLE IF NOT EXISTS sessions (hash TEXT PRIMARY KEY, email TEXT NOT NULL,
                 created TEXT NOT NULL, expires TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS tokens (id TEXT PRIMARY KEY, hash TEXT UNIQUE NOT NULL,
                 email TEXT NOT NULL, name TEXT NOT NULL, created TEXT NOT NULL, last_used TEXT);
             CREATE TABLE IF NOT EXISTS secrets (name TEXT PRIMARY KEY, sealed BLOB NOT NULL,
                 nonce BLOB NOT NULL, hint TEXT NOT NULL, updated TEXT NOT NULL,
                 updated_by TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS codes (email TEXT PRIMARY KEY, hash TEXT NOT NULL,
                 created TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS failures (key TEXT NOT NULL, at TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS failures_key ON failures (key, at);
             DROP TABLE IF EXISTS attempts;
             CREATE TABLE IF NOT EXISTS audit (at TEXT NOT NULL, actor TEXT NOT NULL,
                 action TEXT NOT NULL, detail TEXT NOT NULL);",
        )?;
        let store = Store { db: Mutex::new(db) };
        store.rotate_key()?;
        store.prune();
        let unreadable = store.unreadable_secrets();
        if !unreadable.is_empty() {
            eprintln!(
                "comindly: {} stored secret(s) cannot be decrypted with COMINDLY_SECRET_KEY ({}); \
                 set COMINDLY_SECRET_KEY_PREVIOUS to the old key, or save them again",
                unreadable.len(),
                unreadable.join(", ")
            );
        }
        Ok(store)
    }

    /// Drop expired links, invitations and sessions, and old failed attempts.
    fn prune(&self) {
        let t = now();
        let day = (Utc::now() - Duration::days(1)).to_rfc3339();
        let db = self.db();
        for (sql, arg) in [
            (
                "DELETE FROM links WHERE expires < ?1 OR used IS NOT NULL",
                &t,
            ),
            ("DELETE FROM invites WHERE expires < ?1", &t),
            ("DELETE FROM sessions WHERE expires < ?1", &t),
            ("DELETE FROM failures WHERE at < ?1", &day),
        ] {
            let _ = db.execute(sql, [arg]);
        }
    }

    /// Names of stored secrets the current key cannot open.
    fn unreadable_secrets(&self) -> Vec<String> {
        let Some(key) = env_secret("COMINDLY_SECRET_KEY") else {
            return Vec::new();
        };
        let db = self.db();
        let Ok(mut q) = db.prepare("SELECT name, sealed, nonce FROM secrets") else {
            return Vec::new();
        };
        q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map(|rows| {
            rows.flatten()
                .filter(|(n, s, nonce)| open(&key, n, s, nonce).is_none())
                .map(|(n, _, _)| n)
                .collect()
        })
        .unwrap_or_default()
    }

    /// Failed attempts under `key` in the last few minutes.
    fn failures(&self, key: &str) -> i64 {
        self.db()
            .query_row(
                "SELECT COUNT(*) FROM failures WHERE key = ?1 AND at > ?2",
                params![
                    key,
                    (Utc::now() - Duration::minutes(LOCK_MINUTES)).to_rfc3339()
                ],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    fn fail(&self, key: &str) {
        let _ = self
            .db()
            .execute("INSERT INTO failures VALUES (?1, ?2)", params![key, now()]);
    }

    fn forgive(&self, key: &str) {
        let _ = self
            .db()
            .execute("DELETE FROM failures WHERE key = ?1", [key]);
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn audit(&self, actor: &str, action: &str, detail: &str) {
        let _ = self.db().execute(
            "INSERT INTO audit VALUES (?1, ?2, ?3, ?4)",
            params![now(), actor, action, detail],
        );
    }

    /// The member `email`, with deployment admins always present and admin.
    pub fn member(&self, email: &str) -> Result<Option<Who>> {
        let email = normalize(email);
        if admin_emails().contains(&email) {
            return Ok(Some(Who {
                email,
                role: Role::Admin,
            }));
        }
        let role: Option<String> = self
            .db()
            .query_row("SELECT role FROM users WHERE email = ?1", [&email], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(role.map(|r| Who {
            email,
            role: Role::parse(&r),
        }))
    }

    pub fn members(&self) -> Result<Vec<Member>> {
        let admins = admin_emails();
        let db = self.db();
        let mut q =
            db.prepare("SELECT email, role, created, last_seen FROM users ORDER BY email")?;
        let mut out: Vec<Member> = q
            .query_map([], |r| {
                Ok(Member {
                    email: r.get(0)?,
                    role: Role::parse(&r.get::<_, String>(1)?),
                    fixed: false,
                    created: r.get(2)?,
                    last_seen: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        for m in &mut out {
            if admins.contains(&m.email) {
                m.role = Role::Admin;
                m.fixed = true;
            }
        }
        for a in admins {
            if !out.iter().any(|m| m.email == a) {
                out.push(Member {
                    email: a,
                    role: Role::Admin,
                    fixed: true,
                    created: String::new(),
                    last_seen: None,
                });
            }
        }
        Ok(out)
    }

    pub fn set_role(&self, by: &str, email: &str, role: Role) -> Result<()> {
        let email = normalize(email);
        if admin_emails().contains(&email) {
            bail!("{email} is an admin by the deployment's settings");
        }
        let n = self.db().execute(
            "UPDATE users SET role = ?2 WHERE email = ?1",
            params![email, role.as_str()],
        )?;
        anyhow::ensure!(n == 1, "{email} is not a member");
        self.audit(by, "role", &format!("{email} {}", role.as_str()));
        Ok(())
    }

    /// Remove a member with their sessions, tokens and invitations.
    pub fn remove(&self, by: &str, email: &str) -> Result<()> {
        let email = normalize(email);
        if admin_emails().contains(&email) {
            bail!("{email} is an admin by the deployment's settings");
        }
        let db = self.db();
        for table in ["users", "sessions", "tokens", "invites", "links", "codes"] {
            db.execute(&format!("DELETE FROM {table} WHERE email = ?1"), [&email])?;
        }
        let prefix = format!("code|{email}|");
        db.execute(
            "DELETE FROM failures WHERE substr(key, 1, ?2) = ?1",
            params![prefix, prefix.len() as i64],
        )?;
        drop(db);
        self.audit(by, "remove", &email);
        Ok(())
    }

    /// An invitation for `email` only: a link token, or with `code` a code to hand over.
    pub fn invite(&self, by: &str, email: &str, role: Role, code: bool) -> Result<String> {
        let email = normalize(email);
        anyhow::ensure!(
            email.contains('@') && !email.contains(char::is_whitespace),
            "that is not an email address"
        );
        let (token, h) = if code {
            let c = new_code();
            let h = code_hash(&c);
            (c, h)
        } else {
            let t = new_token("inv_");
            let h = hash(&t);
            (t, h)
        };
        let db = self.db();
        db.execute(
            "DELETE FROM invites WHERE email = ?1 AND used IS NULL",
            [&email],
        )?;
        db.execute(
            "INSERT INTO invites VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                h,
                email,
                role.as_str(),
                by,
                now(),
                (Utc::now() + Duration::days(INVITE_DAYS)).to_rfc3339()
            ],
        )?;
        drop(db);
        self.audit(by, "invite", &format!("{email} {}", role.as_str()));
        Ok(token)
    }

    pub fn invites(&self) -> Result<Vec<Invite>> {
        let db = self.db();
        let mut q = db.prepare(
            "SELECT email, role, created_by, expires FROM invites
             WHERE used IS NULL AND expires > ?1 ORDER BY created DESC",
        )?;
        let out = q
            .query_map([now()], |r| {
                Ok(Invite {
                    email: r.get(0)?,
                    role: Role::parse(&r.get::<_, String>(1)?),
                    created_by: r.get(2)?,
                    expires: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    pub fn revoke_invite(&self, by: &str, email: &str) -> Result<()> {
        let email = normalize(email);
        self.db().execute(
            "DELETE FROM invites WHERE email = ?1 AND used IS NULL",
            [&email],
        )?;
        self.audit(by, "revoke-invite", &email);
        Ok(())
    }

    /// A sign-in link token for a member, or `None` when `email` is not one.
    pub fn sign_in_link(&self, email: &str) -> Result<Option<String>> {
        let Some(who) = self.member(email)? else {
            return Ok(None);
        };
        let db = self.db();
        let recent: i64 = db.query_row(
            "SELECT COUNT(*) FROM links WHERE email = ?1 AND created > ?2",
            params![who.email, (Utc::now() - Duration::hours(1)).to_rfc3339()],
            |r| r.get(0),
        )?;
        // Answered like any other address, so the limit does not reveal who is a member.
        if recent >= MAX_LINKS_PER_HOUR {
            eprintln!("comindly: a member asked for more than {MAX_LINKS_PER_HOUR} sign-in links in an hour");
            return Ok(None);
        }
        let token = new_token("lnk_");
        db.execute(
            "INSERT INTO links VALUES (?1, ?2, ?3, ?4, NULL)",
            params![
                hash(&token),
                who.email,
                now(),
                (Utc::now() + Duration::minutes(LINK_MINUTES)).to_rfc3339()
            ],
        )?;
        Ok(Some(token))
    }

    /// Exchange a sign-in link, an invitation or the admin token for a new session; `client`
    /// limits how often one place may guess.
    pub fn redeem(&self, token: &str, client: &str) -> Result<(String, Who)> {
        let token = token.trim();
        let from = format!("redeem|{client}");
        anyhow::ensure!(
            self.failures(&from) < MAX_REDEEM_FAILED,
            "Too many attempts from here; try again in {LOCK_MINUTES} minutes"
        );
        if let Some(admin) = env_secret("COMINDLY_ADMIN_TOKEN") {
            if constant_eq(token, &admin) {
                let email = admin_emails()
                    .into_iter()
                    .next()
                    .context("set COMINDLY_ADMIN_EMAILS to sign in with the admin token")?;
                self.audit(&email, "sign-in", "admin token");
                return self.session_for(&email);
            }
        }
        let h = hash(token);
        let t = now();
        let link: Option<String> = self
            .db()
            .query_row(
                "UPDATE links SET used = ?2 WHERE hash = ?1 AND used IS NULL AND expires > ?2
                 RETURNING email",
                params![h, t],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(email) = link {
            self.audit(&email, "sign-in", "email link");
            return self.session_for(&email);
        }
        let invite: Option<(String, String)> = self
            .db()
            .query_row(
                "UPDATE invites SET used = ?2 WHERE hash = ?1 AND used IS NULL AND expires > ?2
                 RETURNING email, role",
                params![h, t],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((email, role)) = invite {
            self.db().execute(
                "INSERT INTO users VALUES (?1, ?2, ?3, NULL) ON CONFLICT(email) DO NOTHING",
                params![email, role, t],
            )?;
            self.audit(&email, "join", &role);
            return self.session_for(&email);
        }
        self.fail(&from);
        bail!("this link has expired or was already used; ask for a new one")
    }

    /// Sign in with an email address and a code: an invitation code, which also issues the
    /// person's own sign-in code (returned once), or that sign-in code.
    /// `client` identifies where the attempt comes from: wrong codes pause that address for that
    /// client only, so nobody can lock another person out.
    pub fn sign_in_code(
        &self,
        email: &str,
        code: &str,
        client: &str,
    ) -> Result<(String, Who, Option<String>)> {
        let email = normalize(email);
        anyhow::ensure!(valid_email(&email), "Wrong email or code");
        let from = format!("code|{email}|{client}");
        anyhow::ensure!(
            self.failures(&from) < MAX_FAILED,
            "Too many wrong codes for this address; try again in {LOCK_MINUTES} minutes"
        );
        let h = code_hash(code);
        let t = now();
        let invite: Option<String> = self
            .db()
            .query_row(
                "UPDATE invites SET used = ?3 WHERE hash = ?1 AND email = ?2 AND used IS NULL
                     AND expires > ?3 RETURNING role",
                params![h, email, t],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(role) = invite {
            self.db().execute(
                "INSERT INTO users VALUES (?1, ?2, ?3, NULL) ON CONFLICT(email) DO NOTHING",
                params![email, role, t],
            )?;
            let personal = self.set_code(&email)?;
            self.forgive(&from);
            self.audit(&email, "join", &role);
            let (session, who) = self.session_for(&email)?;
            return Ok((session, who, Some(personal)));
        }
        let known: Option<String> = self
            .db()
            .query_row(
                "SELECT email FROM codes WHERE email = ?1 AND hash = ?2",
                params![email, h],
                |r| r.get(0),
            )
            .optional()?;
        if known.is_some() && self.member(&email)?.is_some() {
            self.forgive(&from);
            self.audit(&email, "sign-in", "code");
            let (session, who) = self.session_for(&email)?;
            return Ok((session, who, None));
        }
        self.fail(&from);
        bail!("Wrong email or code")
    }

    /// Give `email` a new sign-in code; their sessions end, except `keep` (the caller's own).
    pub fn reset_code(&self, by: &str, email: &str, keep: Option<&str>) -> Result<String> {
        let email = normalize(email);
        anyhow::ensure!(self.member(&email)?.is_some(), "{email} is not a member");
        anyhow::ensure!(
            by == email || !admin_emails().contains(&email),
            "{email} is an admin by the deployment's settings and resets their own code"
        );
        let code = self.set_code(&email)?;
        self.db().execute(
            "DELETE FROM sessions WHERE email = ?1 AND hash != ?2",
            params![email, keep.map(hash).unwrap_or_default()],
        )?;
        let prefix = format!("code|{email}|");
        let _ = self.db().execute(
            "DELETE FROM failures WHERE substr(key, 1, ?2) = ?1",
            params![prefix, prefix.len() as i64],
        );
        self.audit(by, "reset-code", &email);
        Ok(code)
    }

    fn set_code(&self, email: &str) -> Result<String> {
        let code = new_code();
        self.db().execute(
            "INSERT INTO codes VALUES (?1, ?2, ?3)
             ON CONFLICT(email) DO UPDATE SET hash = ?2, created = ?3",
            params![email, code_hash(&code), now()],
        )?;
        Ok(code)
    }

    fn session_for(&self, email: &str) -> Result<(String, Who)> {
        let who = self.member(email)?.context("not a member")?;
        let token = new_token("ses_");
        self.db().execute(
            "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4)",
            params![
                hash(&token),
                who.email,
                now(),
                (Utc::now() + Duration::days(SESSION_DAYS)).to_rfc3339()
            ],
        )?;
        Ok((token, who))
    }

    /// The member behind a session cookie or, failing that, a bearer token, and which it was.
    pub fn authenticate(
        &self,
        session: Option<&str>,
        bearer: Option<&str>,
    ) -> Result<Option<(Who, Via)>> {
        let t = now();
        let mut found: Option<(String, Via)> = None;
        if let Some(s) = session {
            found = self
                .db()
                .query_row(
                    "SELECT email FROM sessions WHERE hash = ?1 AND expires > ?2",
                    params![hash(s), t],
                    |r| r.get(0),
                )
                .optional()?
                .map(|e| (e, Via::Session));
        }
        if found.is_none() {
            if let Some(b) = bearer {
                found = if env_secret("COMINDLY_ADMIN_TOKEN").is_some_and(|a| constant_eq(b, &a)) {
                    admin_emails()
                        .into_iter()
                        .next()
                        .map(|e| (e, Via::AdminToken))
                } else {
                    self.db()
                        .query_row(
                            "UPDATE tokens SET last_used = ?2 WHERE hash = ?1 RETURNING email",
                            params![hash(b), t],
                            |r| r.get(0),
                        )
                        .optional()?
                        .map(|e| (e, Via::Token))
                };
            }
        }
        let Some((email, via)) = found else {
            return Ok(None);
        };
        let who = self.member(&email)?;
        if who.is_some() {
            let _ = self.db().execute(
                "UPDATE users SET last_seen = ?2 WHERE email = ?1 AND (last_seen IS NULL OR last_seen < ?3)",
                params![email, t, (Utc::now() - Duration::minutes(5)).to_rfc3339()],
            );
        }
        Ok(who.map(|w| (w, via)))
    }

    pub fn sign_out(&self, session: &str, everywhere: bool) -> Result<()> {
        let db = self.db();
        if everywhere {
            db.execute(
                "DELETE FROM sessions WHERE email = (SELECT email FROM sessions WHERE hash = ?1)",
                [hash(session)],
            )?;
        } else {
            db.execute("DELETE FROM sessions WHERE hash = ?1", [hash(session)])?;
        }
        Ok(())
    }

    /// A personal access token for agents; its value is returned once.
    pub fn create_token(&self, who: &Who, name: &str) -> Result<String> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "give the token a name");
        let token = new_token("cly_");
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.db().execute(
            "INSERT INTO tokens VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            params![id, hash(&token), who.email, name, now()],
        )?;
        self.audit(&who.email, "token", name);
        Ok(token)
    }

    pub fn tokens(&self, who: &Who) -> Result<Vec<AccessToken>> {
        let db = self.db();
        let mut q = db.prepare(
            "SELECT id, name, created, last_used FROM tokens WHERE email = ?1 ORDER BY created DESC",
        )?;
        let out = q
            .query_map([&who.email], |r| {
                Ok(AccessToken {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created: r.get(2)?,
                    last_used: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    pub fn revoke_token(&self, who: &Who, id: &str) -> Result<()> {
        self.db().execute(
            "DELETE FROM tokens WHERE id = ?1 AND email = ?2",
            params![id, who.email],
        )?;
        self.audit(&who.email, "revoke-token", id);
        Ok(())
    }

    /// Store `value` under `name` (an empty value removes it), sealed with `COMINDLY_SECRET_KEY`.
    pub fn set_secret(&self, by: &str, name: &str, value: &str) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            self.db()
                .execute("DELETE FROM secrets WHERE name = ?1", [name])?;
            self.audit(by, "secret-removed", name);
            return Ok(());
        }
        let key = env_secret("COMINDLY_SECRET_KEY")
            .context("set COMINDLY_SECRET_KEY on the server to store tokens here")?;
        let (sealed, nonce) = seal(&key, name, value)?;
        self.db().execute(
            "INSERT INTO secrets VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(name) DO UPDATE SET sealed = ?2, nonce = ?3, hint = ?4, updated = ?5,
                 updated_by = ?6",
            params![name, sealed, nonce, hint(value), now(), by],
        )?;
        self.audit(by, "secret-set", name);
        Ok(())
    }

    pub fn secret(&self, name: &str) -> Option<String> {
        let key = env_secret("COMINDLY_SECRET_KEY")?;
        let (sealed, nonce): (Vec<u8>, Vec<u8>) = self
            .db()
            .query_row(
                "SELECT sealed, nonce FROM secrets WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()?;
        open(&key, name, &sealed, &nonce)
    }

    pub fn secret_info(&self, name: &str) -> Option<SecretInfo> {
        self.db()
            .query_row(
                "SELECT hint, updated, updated_by FROM secrets WHERE name = ?1",
                [name],
                |r| {
                    Ok(SecretInfo {
                        name: name.to_string(),
                        source: "stored",
                        hint: r.get(0)?,
                        updated: r.get(1)?,
                        updated_by: r.get(2)?,
                    })
                },
            )
            .ok()
    }

    /// Names of stored secrets.
    pub fn secret_names(&self) -> Vec<String> {
        let db = self.db();
        let Ok(mut q) = db.prepare("SELECT name FROM secrets ORDER BY name") else {
            return Vec::new();
        };
        q.query_map([], |r| r.get(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Re-seal secrets stored under `COMINDLY_SECRET_KEY_PREVIOUS` with the current key.
    fn rotate_key(&self) -> Result<()> {
        let (Some(key), Some(old)) = (
            env_secret("COMINDLY_SECRET_KEY"),
            env_secret("COMINDLY_SECRET_KEY_PREVIOUS"),
        ) else {
            return Ok(());
        };
        let db = self.db();
        let rows: Vec<(String, Vec<u8>, Vec<u8>)> = {
            let mut q = db.prepare("SELECT name, sealed, nonce FROM secrets")?;
            let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for (name, sealed, nonce) in rows {
            if open(&key, &name, &sealed, &nonce).is_some() {
                continue;
            }
            let Some(plain) = open(&old, &name, &sealed, &nonce) else {
                continue;
            };
            let (s, n) = seal(&key, &name, &plain)?;
            db.execute(
                "UPDATE secrets SET sealed = ?2, nonce = ?3 WHERE name = ?1",
                params![name, s, n],
            )?;
        }
        Ok(())
    }
}

/// Equal without revealing where or how long the values differ.
fn constant_eq(a: &str, b: &str) -> bool {
    let (a, b) = (blake3::hash(a.as_bytes()), blake3::hash(b.as_bytes()));
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn valid_email(email: &str) -> bool {
    email.contains('@') && !email.contains(|c: char| c.is_whitespace() || c == '|')
}

/// The store, when this process runs in server mode (opened once).
pub fn store() -> Option<&'static Store> {
    static STORE: std::sync::OnceLock<Option<Store>> = std::sync::OnceLock::new();
    STORE
        .get_or_init(|| {
            if !enabled() {
                return None;
            }
            Store::open(&db_path())
                .map_err(|e| eprintln!("comindly: server database: {e:#}"))
                .ok()
        })
        .as_ref()
}

/// Secret names for a git host's token and for the LLM key.
pub fn git_secret(host: &str) -> String {
    format!("git:{}", host.to_lowercase())
}

pub const LLM_SECRET: &str = "llm_api_key";

/// A git host's token stored on this server.
pub fn git_token(host: &str) -> Option<String> {
    store()?.secret(&git_secret(host))
}

/// Whether sign-in links and invitations can be emailed.
pub fn mail_configured() -> bool {
    env_secret("COMINDLY_SMTP_URL").is_some()
}

/// Send a plain-text email through `COMINDLY_SMTP_URL` (e.g. `smtps://user:pass@host`).
pub async fn send_mail(to: &str, subject: &str, body: &str) -> Result<()> {
    use lettre::message::header::ContentType;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
    let url = env_secret("COMINDLY_SMTP_URL").context("no mail server configured")?;
    let from =
        env_secret("COMINDLY_SMTP_FROM").unwrap_or_else(|| "comindly <comindly@localhost>".into());
    let msg = Message::builder()
        .from(
            from.parse()
                .context("COMINDLY_SMTP_FROM is not an address")?,
        )
        .to(to.parse().context("not an email address")?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())?;
    AsyncSmtpTransport::<Tokio1Executor>::from_url(&url)
        .context("COMINDLY_SMTP_URL is not a valid smtp:// or smtps:// URL")?
        .build()
        .send(msg)
        .await
        .map_err(|e| anyhow::anyhow!("sending mail failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> ((), Store) {
        let dir = std::env::temp_dir().join(format!("comindly-server-{}", uuid::Uuid::new_v4()));
        ((), Store::open(&dir.join("server.db")).unwrap())
    }

    #[test]
    fn invitations_are_single_use_and_tied_to_their_email() {
        let (_d, s) = temp_store();
        let token = s
            .invite("admin@x.org", "Dev@X.org ", Role::Member, false)
            .unwrap();
        let (session, who) = s.redeem(&token, "10.0.0.1").unwrap();
        assert_eq!(who.email, "dev@x.org");
        assert_eq!(who.role, Role::Member);
        assert!(s.redeem(&token, "10.0.0.1").is_err());
        let (back, _) = s.authenticate(Some(&session), None).unwrap().unwrap();
        assert_eq!(back.email, "dev@x.org");
        s.sign_out(&session, false).unwrap();
        assert!(s.authenticate(Some(&session), None).unwrap().is_none());
    }

    #[test]
    fn sign_in_links_only_for_members_and_once() {
        let (_d, s) = temp_store();
        assert!(s.sign_in_link("stranger@x.org").unwrap().is_none());
        let inv = s.invite("a", "m@x.org", Role::Member, false).unwrap();
        s.redeem(&inv, "10.0.0.1").unwrap();
        let link = s.sign_in_link("m@x.org").unwrap().unwrap();
        assert!(s.redeem(&link, "10.0.0.1").is_ok());
        assert!(s.redeem(&link, "10.0.0.1").is_err());
    }

    #[test]
    fn removing_a_member_ends_their_tokens() {
        let (_d, s) = temp_store();
        let inv = s.invite("a", "m@x.org", Role::Member, false).unwrap();
        let (_, who) = s.redeem(&inv, "10.0.0.1").unwrap();
        let token = s.create_token(&who, "laptop").unwrap();
        assert!(s.authenticate(None, Some(&token)).unwrap().is_some());
        s.remove("a", "m@x.org").unwrap();
        assert!(s.authenticate(None, Some(&token)).unwrap().is_none());
    }

    #[test]
    fn invitation_codes_need_their_own_address_and_give_a_sign_in_code() {
        let (_d, s) = temp_store();
        let invite = s.invite("a", "m@x.org", Role::Member, true).unwrap();
        assert!(s.sign_in_code("other@x.org", &invite, "10.0.0.1").is_err());
        let typed = invite.to_lowercase().replace('-', " ");
        let (_, who, personal) = s.sign_in_code("M@x.org", &typed, "10.0.0.1").unwrap();
        assert_eq!(who.email, "m@x.org");
        let personal = personal.unwrap();
        assert!(s.sign_in_code("m@x.org", &invite, "10.0.0.1").is_err());
        let (_, _, again) = s.sign_in_code("m@x.org", &personal, "10.0.0.1").unwrap();
        assert!(again.is_none());
    }

    #[test]
    fn a_reset_code_replaces_the_old_one_and_ends_other_sessions() {
        let (_d, s) = temp_store();
        let invite = s.invite("a", "m@x.org", Role::Member, true).unwrap();
        let (mine, _, old) = s.sign_in_code("m@x.org", &invite, "10.0.0.1").unwrap();
        let (elsewhere, _, _) = s
            .sign_in_code("m@x.org", old.as_ref().unwrap(), "10.0.0.1")
            .unwrap();
        let new = s.reset_code("m@x.org", "m@x.org", Some(&mine)).unwrap();
        assert!(s.authenticate(Some(&mine), None).unwrap().is_some());
        assert!(s.authenticate(Some(&elsewhere), None).unwrap().is_none());
        assert!(s
            .sign_in_code("m@x.org", old.as_ref().unwrap(), "10.0.0.1")
            .is_err());
        assert!(s.sign_in_code("m@x.org", &new, "10.0.0.1").is_ok());
    }

    #[test]
    fn wrong_codes_lock_the_address_for_that_client() {
        let (_d, s) = temp_store();
        let invite = s.invite("a", "m@x.org", Role::Member, true).unwrap();
        for _ in 0..MAX_FAILED {
            assert!(s.sign_in_code("m@x.org", "AAAA-AAAA", "10.0.0.1").is_err());
        }
        let e = s
            .sign_in_code("m@x.org", &invite, "10.0.0.1")
            .unwrap_err()
            .to_string();
        assert!(e.contains("Too many"), "{e}");
        assert!(s.sign_in_code("m@x.org", &invite, "10.0.0.2").is_ok());
    }

    #[test]
    fn codes_are_twenty_characters_in_groups() {
        let c = new_code();
        assert_eq!(c.len(), 24);
        assert!(c.split('-').all(|g| g.len() == 4));
        assert_ne!(c, new_code());
    }

    #[test]
    fn sealed_values_open_only_with_their_key() {
        let (sealed, nonce) = seal("key-one", "git:gitlab.com", "glpat-example").unwrap();
        assert_eq!(
            open("key-one", "git:gitlab.com", &sealed, &nonce).as_deref(),
            Some("glpat-example")
        );
        assert!(open("key-two", "git:gitlab.com", &sealed, &nonce).is_none());
        assert!(open("key-one", "llm_api_key", &sealed, &nonce).is_none());
        assert_eq!(hint("glpat-example"), "…mple");
    }
}
