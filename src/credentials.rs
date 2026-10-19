//! `~/.config/comind/credentials`: the password of each team this machine belongs to, by team name.
//! Created readable only by you; API keys never go here (they are in the team settings, encrypted).

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    team: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    name: String,
    password: String,
}

pub fn path() -> PathBuf {
    crate::config::config_path()
        .parent()
        .map(|d| d.join("credentials"))
        .unwrap_or_else(|| PathBuf::from("credentials"))
}

fn read() -> File {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

/// The password stored for team `name`.
pub fn password(name: &str) -> Option<String> {
    read()
        .team
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| e.password)
}

/// Store the password for team `name` (an empty one removes the entry).
pub fn set_password(name: &str, password: &str) -> Result<()> {
    let mut f = read();
    f.team.retain(|e| e.name != name);
    if !password.trim().is_empty() {
        f.team.push(Entry {
            name: name.to_string(),
            password: password.trim().to_string(),
        });
    }
    let p = path();
    std::fs::create_dir_all(p.parent().context("config dir")?)?;
    let text = format!(
        "# comind team passwords, one entry per team. Keep this file private.\n\n{}",
        toml::to_string(&f)?
    );
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, &p).with_context(|| format!("write {}", p.display()))
}
