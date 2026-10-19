//! comind's own checkout of each repository's default branch: a detached git worktree under
//! `~/.cache/comind/worktrees`, so indexing never touches your working folder or branches. Uses the
//! `git` command, so your SSH keys and credential helpers apply.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// A checkout ready to index.
pub struct Checkout {
    pub dir: PathBuf,
    /// What was checked out, e.g. `origin/main`.
    pub target: String,
    pub commit: String,
    /// Set when fetching failed and the last fetched state was used instead.
    pub stale: Option<String>,
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Give up on a stalled fetch instead of waiting on a struggling server.
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1000")
        .env("GIT_HTTP_LOW_SPEED_TIME", "20")
        .output()
        .context("run git (is it installed?)")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The repository's name: its folder's name.
pub fn name(repo: &Path) -> String {
    repo.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into())
}

fn has_origin(repo: &Path) -> bool {
    git(repo, &["remote", "get-url", "origin"]).is_ok()
}

/// The branch to index: `origin`'s default (from `origin/HEAD`), else `main` or `master`.
pub fn default_branch(repo: &Path) -> Result<String> {
    if let Ok(head) = git(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        if let Some(b) = head.strip_prefix("origin/") {
            return Ok(b.to_string());
        }
    }
    let prefix = if has_origin(repo) {
        "refs/remotes/origin/"
    } else {
        "refs/heads/"
    };
    for b in ["main", "master"] {
        if git(
            repo,
            &["rev-parse", "--verify", "--quiet", &format!("{prefix}{b}")],
        )
        .is_ok()
        {
            return Ok(b.to_string());
        }
    }
    bail!("no main or master branch in {}", repo.display())
}

/// The commit `branch` points to on the remote, without fetching.
pub fn remote_head(repo: &Path, branch: &str) -> Result<String> {
    let out = git(
        repo,
        &["ls-remote", "origin", &format!("refs/heads/{branch}")],
    )?;
    out.split_whitespace()
        .next()
        .map(str::to_string)
        .with_context(|| format!("origin has no branch {branch}"))
}

/// The branch checked out in your own folder (for display).
pub fn current_branch(repo: &Path) -> Option<String> {
    git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()
}

pub fn remote_url(repo: &Path) -> Option<String> {
    git(repo, &["remote", "get-url", "origin"]).ok()
}

fn worktree_dir(repo: &Path) -> Result<PathBuf> {
    let abs = std::fs::canonicalize(repo).with_context(|| format!("{}", repo.display()))?;
    let key = blake3::hash(abs.to_string_lossy().as_bytes()).to_hex();
    let home = std::env::var_os("HOME").context("HOME")?;
    Ok(PathBuf::from(home)
        .join(".cache/comind/worktrees")
        .join(&key[..12])
        .join(name(repo)))
}

/// Fetch `branch` (default: the repository's default branch) and check it out in comind's
/// worktree, detached, so `branch` stays free to check out in your folder.
pub fn prepare(repo: &Path, branch: Option<&str>) -> Result<Checkout> {
    if !repo.join(".git").exists() {
        bail!("{} is not a git repository", repo.display());
    }
    let branch = match branch {
        Some(b) => b.to_string(),
        None => default_branch(repo)?,
    };
    let mut stale = None;
    let target = if has_origin(repo) {
        let target = format!("origin/{branch}");
        if let Err(e) = git(repo, &["fetch", "--quiet", "origin", &branch]) {
            let why = format!("{e:#}");
            git(repo, &["rev-parse", "--verify", "--quiet", &target])
                .map_err(|_| e.context("and no earlier fetch to fall back to"))?;
            stale = Some(why);
        }
        target
    } else {
        branch.clone()
    };
    let dir = worktree_dir(repo)?;
    let _ = git(repo, &["worktree", "prune"]);
    if dir.join(".git").exists() {
        git(
            &dir,
            &["checkout", "--quiet", "--detach", "--force", &target],
        )?;
        git(&dir, &["clean", "-fdq"])?;
    } else {
        std::fs::create_dir_all(dir.parent().context("worktree parent")?)?;
        let d = dir.to_string_lossy();
        git(
            repo,
            &[
                "worktree", "add", "--quiet", "--detach", "--force", &d, &target,
            ],
        )?;
    }
    let commit = git(&dir, &["rev-parse", "--short", "HEAD"])?;
    Ok(Checkout {
        dir,
        target,
        commit,
        stale,
    })
}

/// Remove comind's worktree for `repo` (your folder is untouched).
pub fn remove(repo: &Path) -> Result<()> {
    let dir = worktree_dir(repo)?;
    if dir.exists() {
        let d = dir.to_string_lossy();
        git(repo, &["worktree", "remove", "--force", &d])?;
    }
    Ok(())
}

/// Git repositories under `dir`, up to `depth` levels down (not inside other repositories).
pub fn scan(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries.into_iter().filter(|p| p.is_dir()) {
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden || path.ends_with("node_modules") {
            continue;
        }
        if path.join(".git").exists() {
            out.push(path);
        } else if depth > 1 {
            out.extend(scan(&path, depth - 1));
        }
    }
    out
}

/// Clone `url` into `parent/<name>` with your git credentials; returns the new folder.
pub fn clone(url: &str, parent: &Path) -> Result<PathBuf> {
    let name = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .map(|n| n.trim_end_matches(".git"))
        .filter(|n| !n.is_empty())
        .context("cannot tell the repository's name from the URL")?;
    let dest = parent.join(name);
    if dest.exists() {
        bail!("{} already exists", dest.display());
    }
    std::fs::create_dir_all(parent)?;
    git(parent, &["clone", "--quiet", url, name])?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_follows_main_and_leaves_the_folder_alone() {
        let base = std::env::temp_dir().join(format!("comind-wt-{}", std::process::id()));
        let origin = base.join("origin");
        let mine = base.join("mine");
        std::fs::create_dir_all(&origin).unwrap();
        let g = |d: &Path, a: &[&str]| git(d, a).unwrap();
        g(&origin, &["init", "--quiet", "-b", "main"]);
        g(&origin, &["config", "user.email", "t@t"]);
        g(&origin, &["config", "user.name", "t"]);
        std::fs::write(origin.join("a.py"), "x = 1\n").unwrap();
        g(&origin, &["add", "."]);
        g(&origin, &["commit", "--quiet", "-m", "one"]);
        g(
            &base,
            &["clone", "--quiet", origin.to_str().unwrap(), "mine"],
        );
        g(&mine, &["checkout", "--quiet", "-b", "feature"]);
        std::fs::write(mine.join("wip.py"), "draft\n").unwrap();

        std::fs::write(origin.join("b.py"), "y = 2\n").unwrap();
        g(&origin, &["add", "."]);
        g(&origin, &["commit", "--quiet", "-m", "two"]);

        let c = prepare(&mine, None).unwrap();
        assert_eq!(c.target, "origin/main");
        assert!(
            c.dir.join("b.py").exists(),
            "the worktree has the newest main"
        );
        assert!(!c.dir.join("wip.py").exists());
        assert_eq!(current_branch(&mine).as_deref(), Some("feature"));
        assert!(mine.join("wip.py").exists(), "your folder is untouched");
        g(&mine, &["checkout", "--quiet", "main"]);
        assert!(
            prepare(&mine, None).is_ok(),
            "a second run reuses the worktree"
        );
        remove(&mine).unwrap();
        assert!(!c.dir.exists());
        assert_eq!(scan(&base, 1).len(), 2);
        std::fs::remove_dir_all(&base).ok();
    }
}
