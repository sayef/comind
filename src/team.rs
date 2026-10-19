//! Team settings, stored with the index (`<index>/_team`, a versioned LanceDB table) so everyone
//! using it gets the same LLM, embedding model and key. Secrets are sealed with the team password:
//! Argon2id derives the key (salted), XChaCha20-Poly1305 encrypts.

use std::sync::{Arc, OnceLock, RwLock};

use anyhow::{bail, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use futures::TryStreamExt;
use lancedb::arrow::arrow_array::{ArrayRef, RecordBatch, StringArray};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema};
use lancedb::query::ExecutableQuery;
use serde::{Deserialize, Serialize};

const TABLE: &str = "settings";

/// What everyone using an index shares; `None` leaves comind's default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrich: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flows: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide: Option<bool>,
    /// Secrets, encrypted with the team password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<Sealed>,
}

/// The secrets, in the clear only in memory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Secrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_api_key: Option<String>,
}

/// Encrypted secrets with what is needed to derive their key again from the password.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sealed {
    pub kdf: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
    pub nonce: String,
    pub data: String,
}

/// The stored settings with the table version they came from and who saved them.
#[derive(Debug, Clone, Default)]
pub struct Stored {
    pub settings: TeamSettings,
    pub version: Option<u64>,
    pub changed_by: Option<String>,
    pub changed_at: Option<String>,
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("random bytes: {e}"))?;
    Ok(b)
}

/// Seven random words from the BIP39 list: about 77 bits.
pub fn passphrase() -> Result<String> {
    let words = bip39::Language::English.word_list();
    let bytes = random::<14>()?;
    Ok(bytes
        .chunks(2)
        .map(|c| words[(u16::from_le_bytes([c[0], c[1]]) % 2048) as usize])
        .collect::<Vec<_>>()
        .join("-"))
}

fn derive(password: &str, salt: &[u8], m_kib: u32, t: u32, p: u32) -> Result<[u8; 32]> {
    let params = argon2::Params::new(m_kib, t, p, Some(32))
        .map_err(|e| anyhow::anyhow!("argon2 parameters: {e}"))?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    a.hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("argon2: {e}"))?;
    Ok(key)
}

/// Encrypt `secrets` under `password` with a fresh salt and nonce.
pub fn seal(secrets: &Secrets, password: &str) -> Result<Sealed> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    let (m_kib, t, p) = (64 * 1024, 3, 1);
    let salt = random::<16>()?;
    let nonce = random::<24>()?;
    let key = derive(password, &salt, m_kib, t, p)?;
    let cipher = chacha20poly1305::XChaCha20Poly1305::new(&chacha20poly1305::Key::from(key));
    let plain = serde_json::to_vec(secrets)?;
    let data = cipher
        .encrypt(&chacha20poly1305::XNonce::from(nonce), plain.as_ref())
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    Ok(Sealed {
        kdf: "argon2id".into(),
        m_kib,
        t,
        p,
        salt: B64.encode(salt),
        nonce: B64.encode(nonce),
        data: B64.encode(data),
    })
}

/// Decrypt with `password`; a wrong password fails here.
pub fn open(sealed: &Sealed, password: &str) -> Result<Secrets> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    if sealed.kdf != "argon2id" {
        bail!("unknown key derivation `{}`", sealed.kdf);
    }
    let salt = B64.decode(&sealed.salt)?;
    let nonce: [u8; 24] = B64
        .decode(&sealed.nonce)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("bad nonce"))?;
    let key = derive(password, &salt, sealed.m_kib, sealed.t, sealed.p)?;
    let cipher = chacha20poly1305::XChaCha20Poly1305::new(&chacha20poly1305::Key::from(key));
    let plain = cipher
        .decrypt(
            &chacha20poly1305::XNonce::from(nonce),
            B64.decode(&sealed.data)?.as_ref(),
        )
        .map_err(|_| anyhow::anyhow!("wrong team password"))?;
    Ok(serde_json::from_slice(&plain)?)
}

fn uri(root: &str) -> String {
    format!("{}/_team", root.trim_end_matches('/'))
}

/// The team settings of the index at `root` (empty when none were saved).
pub async fn load(root: &str) -> Result<Stored> {
    let db = crate::s3::connect(&uri(root)).execute().await?;
    let Ok(tbl) = db.open_table(TABLE).execute().await else {
        return Ok(Stored::default());
    };
    let version = tbl.version().await.ok();
    let mut st = tbl.query().execute().await?;
    let mut out = Stored {
        version,
        ..Default::default()
    };
    while let Some(b) = st.try_next().await? {
        let col = |n: &str| crate::index::str_col(&b, n);
        if b.num_rows() > 0 {
            out.settings = serde_json::from_str(col("settings")?.value(0))?;
            out.changed_by = Some(col("changed_by")?.value(0).to_string());
            out.changed_at = Some(col("changed_at")?.value(0).to_string());
        }
    }
    Ok(out)
}

/// Save `settings` if nobody saved since `expected` (the version they were loaded at).
pub async fn save(root: &str, settings: &TeamSettings, expected: Option<u64>) -> Result<u64> {
    let db = crate::s3::connect(&uri(root)).execute().await?;
    let now = match db.open_table(TABLE).execute().await {
        Ok(t) => Some(t.version().await?),
        Err(_) => None,
    };
    if now != expected {
        bail!("the team settings were changed by someone else; reload and try again");
    }
    let by = crate::ledger::author_name().unwrap_or_else(crate::ledger::actor);
    let schema = Arc::new(Schema::new(vec![
        Field::new("settings", DataType::Utf8, false),
        Field::new("changed_by", DataType::Utf8, false),
        Field::new("changed_at", DataType::Utf8, false),
    ]));
    let col = |v: String| Arc::new(StringArray::from(vec![v])) as ArrayRef;
    let batch = RecordBatch::try_new(
        schema,
        vec![
            col(serde_json::to_string(settings)?),
            col(by),
            col(chrono::Utc::now().to_rfc3339()),
        ],
    )?;
    crate::index::write_versioned(&db, TABLE, batch).await
}

/// The team password: `COMIND_TEAM_PASSWORD`, else the one stored for this team's name.
pub fn password(settings: &TeamSettings) -> Option<String> {
    if let Ok(p) = std::env::var("COMIND_TEAM_PASSWORD") {
        return Some(p).filter(|p| !p.is_empty());
    }
    crate::credentials::password(settings.name.as_deref()?)
}

/// Remember the team password on this machine, under the team's name.
pub fn remember_password(settings: &TeamSettings, password: &str) -> Result<()> {
    let name = settings
        .name
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("give the team a name first"))?;
    crate::credentials::set_password(name, password)
}

/// The team settings in effect for this process, with their secrets when unlocked.
#[derive(Debug, Clone, Default)]
pub struct Active {
    pub settings: TeamSettings,
    pub secrets: Option<Secrets>,
    /// Why the secrets are not available (no password, wrong password).
    pub locked: Option<String>,
}

fn active() -> &'static RwLock<Option<Active>> {
    static A: OnceLock<RwLock<Option<Active>>> = OnceLock::new();
    A.get_or_init(|| RwLock::new(None))
}

/// The settings `Config` should prefer, if a team index was loaded.
pub fn current() -> Option<Active> {
    active().read().ok().and_then(|a| a.clone())
}

/// Load the team settings of `root`, unlock their secrets if a password is known, and make them
/// the ones this process uses.
pub fn apply(root: &str) -> Result<Option<Active>> {
    let stored = crate::ledger::runtime()?.block_on(load(root))?;
    let a = unlock(&stored.settings, password(&stored.settings).as_deref());
    if let Ok(mut g) = active().write() {
        *g = Some(a.clone());
    }
    Ok(Some(a))
}

/// The settings with their secrets opened by `password`, or why they stay locked.
pub fn unlock(settings: &TeamSettings, password: Option<&str>) -> Active {
    let (secrets, locked) = match (&settings.sealed, password) {
        (None, _) => (None, None),
        (Some(_), None) => (None, Some("enter the team password".to_string())),
        (Some(s), Some(p)) => match open(s, p) {
            Ok(x) => (Some(x), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        },
    };
    Active {
        settings: settings.clone(),
        secrets,
        locked,
    }
}

/// Replace the process's active team settings (after a save from the UI).
pub fn set_current(a: Active) {
    if let Ok(mut g) = active().write() {
        *g = Some(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_and_reject_wrong_password() {
        let s = Secrets {
            llm_api_key: Some("sk-test".into()),
        };
        let pw = passphrase().unwrap();
        assert_eq!(pw.split('-').count(), 7);
        let sealed = seal(&s, &pw).unwrap();
        assert!(!sealed.data.contains("sk-test"));
        assert_eq!(
            open(&sealed, &pw).unwrap().llm_api_key.as_deref(),
            Some("sk-test")
        );
        assert!(open(&sealed, "wrong").is_err());
        let again = seal(&s, &pw).unwrap();
        assert_ne!(again.salt, sealed.salt, "every seal gets a fresh salt");
    }

    #[tokio::test]
    async fn save_load_and_refuse_a_stale_write() {
        let dir = std::env::temp_dir().join(format!("comind-team-{}", std::process::id()));
        let root = dir.to_str().unwrap();
        let first = TeamSettings {
            llm_model: Some("m1".into()),
            ..Default::default()
        };
        let v1 = save(root, &first, None).await.unwrap();
        let loaded = load(root).await.unwrap();
        assert_eq!(loaded.settings, first);
        let second = TeamSettings {
            llm_model: Some("m2".into()),
            ..Default::default()
        };
        save(root, &second, Some(v1)).await.unwrap();
        assert!(save(root, &first, Some(v1)).await.is_err(), "stale version");
        assert_eq!(
            load(root).await.unwrap().settings.llm_model.as_deref(),
            Some("m2")
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
