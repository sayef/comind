//! Local mirror of a remote (S3) index for latency-sensitive readers (hooks, the MCP server, the web
//! UI): a file copy of `_graph` and the ledger as one JSON file. Writes still go to the remote store.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use futures::{StreamExt, TryStreamExt};
use object_store::{ObjectMeta, ObjectStore, ObjectStoreExt};

use crate::ledger::Record;
use crate::model::{Edge, Symbol};

/// Default seconds between freshness checks against the remote index.
pub const DEFAULT_TTL: u64 = 300;

pub fn is_remote(index_root: &str) -> bool {
    index_root.contains("://")
}

/// Cache directory for a remote index root.
pub fn dir(index_root: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let key = blake3::hash(index_root.as_bytes()).to_hex();
    Some(
        PathBuf::from(home)
            .join(".cache/comind/remote")
            .join(&key[..16]),
    )
}

/// The cached graph and ledger, when a mirror exists.
pub fn read(cache: &Path) -> Option<(Vec<Symbol>, Vec<Edge>, Vec<Record>)> {
    let graph = cache.join("_graph");
    if !graph.is_dir() {
        return None;
    }
    let (symbols, edges) = crate::index::read_graph_blocking(graph.to_str()?).ok()?;
    let records = std::fs::read(cache.join("ledger.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    Some((symbols, edges, records))
}

/// Whether the mirror is due for a freshness check (missing, or older than `ttl`).
pub fn stale(cache: &Path, ttl: Duration) -> bool {
    std::fs::metadata(cache.join("checked"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_none_or(|age| age > ttl)
}

/// Mark the mirror stale so the next reader refreshes it (after a local write).
pub fn invalidate(index_root: &str) {
    if let Some(d) = dir(index_root) {
        let _ = std::fs::remove_file(d.join("checked"));
    }
}

/// Sync the mirror: copy the graph when its remote version changed, re-read the ledger.
/// Returns whether the graph was copied.
pub fn refresh(index_root: &str, cache: &Path) -> Result<bool> {
    std::fs::create_dir_all(cache)?;
    let lock = cache.join("refresh.lock");
    // One refresher at a time; a lock older than 10 minutes is from a crashed run.
    let held = std::fs::metadata(&lock)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < Duration::from_secs(600));
    if held {
        return Ok(false);
    }
    std::fs::write(&lock, b"")?;
    let out = sync(index_root, cache);
    let _ = std::fs::remove_file(&lock);
    out
}

fn sync(index_root: &str, cache: &Path) -> Result<bool> {
    let copied = crate::ledger::runtime()?.block_on(mirror(index_root, &cache.join("_graph")))?;
    let records = crate::ledger::all_blocking(index_root)?;
    let tmp = cache.join("ledger.json.new");
    std::fs::write(&tmp, serde_json::to_vec(&records)?)?;
    std::fs::rename(&tmp, cache.join("ledger.json"))?;
    std::fs::write(cache.join("checked"), b"")?;
    Ok(copied)
}

/// Copy new or changed files of the remote `_graph` to `dst`, data before manifests so a reader
/// never sees a version whose files are missing. Returns whether anything was copied.
async fn mirror(index_root: &str, dst: &Path) -> Result<bool> {
    let (store, prefix) = crate::ledger::sub_store(index_root, "_graph")?;
    let objects: Vec<ObjectMeta> = store.list(Some(&prefix)).try_collect().await?;
    let rel = |o: &ObjectMeta| {
        let p = o.location.as_ref();
        p.strip_prefix(prefix.as_ref())
            .unwrap_or(p)
            .trim_start_matches('/')
            .to_string()
    };
    let same = |o: &ObjectMeta| {
        std::fs::metadata(dst.join(rel(o))).is_ok_and(|m| {
            m.len() == o.size && m.modified().ok() == Some(SystemTime::from(o.last_modified))
        })
    };
    let (manifests, data): (Vec<&ObjectMeta>, Vec<&ObjectMeta>) = objects
        .iter()
        .filter(|o| !same(o))
        .partition(|o| rel(o).contains("_versions/") || rel(o).contains("_latest"));
    let fetch = |o: &ObjectMeta| {
        let (store, path) = (store.clone(), dst.join(rel(o)));
        let (location, at) = (o.location.clone(), SystemTime::from(o.last_modified));
        async move {
            let bytes = store.get(&location).await?.bytes().await?;
            std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
            let part = PathBuf::from(format!("{}.part", path.display()));
            std::fs::write(&part, &bytes)?;
            std::fs::File::options()
                .write(true)
                .open(&part)?
                .set_modified(at)?;
            std::fs::rename(&part, &path)?;
            anyhow::Ok(())
        }
    };
    futures::stream::iter(data.iter().map(|o| fetch(o)))
        .buffer_unordered(16)
        .try_collect::<Vec<_>>()
        .await?;
    for o in &manifests {
        fetch(o).await?;
    }
    let keep: std::collections::HashSet<PathBuf> =
        objects.iter().map(|o| dst.join(rel(o))).collect();
    prune(dst, &keep);
    Ok(!data.is_empty() || !manifests.is_empty())
}

/// Remove local files the remote no longer has.
fn prune(dir: &Path, keep: &std::collections::HashSet<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune(&path, keep);
        } else if !keep.contains(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Where to read the graph of `index_root`: the index itself, or for a remote index its local
/// mirror, synced first when missing or stale (or in the background when `wait` is false).
pub fn graph_dir(index_root: &str, ttl: Duration, wait: bool) -> String {
    let remote = format!("{}/_graph", index_root.trim_end_matches('/'));
    let Some(cache) = dir(index_root).filter(|_| is_remote(index_root)) else {
        return remote;
    };
    let have = cache.join("_graph").is_dir();
    if !have || (wait && stale(&cache, ttl)) {
        if let Err(e) = refresh(index_root, &cache) {
            eprintln!("comind: reading the remote index directly ({e:#})");
            if !have {
                return remote;
            }
        }
    } else if stale(&cache, ttl) {
        refresh_in_background(index_root);
    }
    cache.join("_graph").to_string_lossy().into_owned()
}

/// Start `comind cache refresh` detached, so the caller never waits on the remote store.
pub fn refresh_in_background(index_root: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(exe)
        .args(["cache", "refresh", "--index-dir", index_root])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_follows_the_remote() {
        let base = std::env::temp_dir().join(format!("comind-cache-{}", std::process::id()));
        let remote = base.join("remote");
        let cache = base.join("cache");
        let root = remote.to_str().unwrap();
        let sym = |name: &str| crate::model::Symbol {
            id: crate::model::GlobalSymbolId {
                scheme: "t".into(),
                package_manager: ".".into(),
                package: "r".into(),
                version: ".".into(),
                descriptor: format!("m/{name}()."),
            },
            name: name.into(),
            kind: crate::model::SymbolKind::Function,
            language: crate::model::Language::Python,
            repo: crate::model::RepoId("r".into()),
            file_path: "m.py".into(),
            range: crate::model::Range {
                start: crate::model::Position { line: 1, column: 0 },
                end: crate::model::Position { line: 2, column: 0 },
            },
            signature: None,
            docstring: None,
            fingerprint: None,
        };
        let graph = format!("{root}/_graph");
        crate::index::write_graph_blocking(&graph, &[sym("a")], &[]).unwrap();
        assert!(stale(&cache, Duration::from_secs(300)));
        assert!(refresh(root, &cache).unwrap(), "first sync copies");
        assert!(!stale(&cache, Duration::from_secs(300)));
        assert!(
            !refresh(root, &cache).unwrap(),
            "unchanged remote is not copied again"
        );
        crate::index::write_graph_blocking(&graph, &[sym("a"), sym("b")], &[]).unwrap();
        assert!(
            refresh(root, &cache).unwrap(),
            "a new remote version is copied"
        );
        let (symbols, _, records) = read(&cache).unwrap();
        assert_eq!(symbols.len(), 2);
        assert!(records.is_empty());
        std::fs::remove_dir_all(&base).ok();
    }
}
