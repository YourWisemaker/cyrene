//! Self-update support: version checks against GitHub Releases plus an
//! installer hand-off (R-update).
//!
//! Cyrene ships as a single binary, so "updating" means fetching the latest
//! published release tag and, when the user opts in, re-running the official
//! installer to replace the binary in place. To stay out of the way, the CLI
//! only hits the network at most once per day: the result is cached under
//! `~/.cyrene/.update-check.json` and a one-line notification is printed from
//! that cache on subsequent runs (no network, no latency).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// `owner/repo` slug used for the GitHub Releases API.
const REPO: &str = "cyrene-agent/cyrene";
/// The official installer, re-run to perform an in-place update.
const INSTALL_URL: &str = "https://raw.githubusercontent.com/cyrene-agent/cyrene/master/install.sh";
/// Minimum gap between network checks so the CLI stays snappy and quiet.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
/// Network checks are best-effort; never let one stall the CLI for long.
const REQUEST_TIMEOUT_SECS: u64 = 3;

/// The version this binary was built as.
#[must_use]
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Cached result of the last network check.
#[derive(Debug, Serialize, Deserialize)]
struct UpdateCache {
    /// Unix timestamp (seconds) of the last successful check.
    last_check: u64,
    /// Latest release version seen (semver, no leading `v`).
    latest: String,
}

fn cache_path() -> Option<std::path::PathBuf> {
    cyrene_config::cyrene_home_dir().map(|d| d.join(".update-check.json"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache() -> Option<UpdateCache> {
    let path = cache_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_cache(latest: &str) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cache = UpdateCache {
        last_check: now_secs(),
        latest: latest.to_owned(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = std::fs::write(path, json);
    }
}

/// `true` when `latest` is a strictly newer semver than `current`. Unparseable
/// versions are treated as "no update" so a bad tag never nags the user.
fn is_newer(current: &str, latest: &str) -> bool {
    let cur = current.trim_start_matches('v');
    let lat = latest.trim_start_matches('v');
    match (semver::Version::parse(cur), semver::Version::parse(lat)) {
        (Ok(c), Ok(l)) => l > c,
        _ => false,
    }
}

/// Fetches the latest release tag from the GitHub API. Best-effort: any network
/// or parse failure returns `None` rather than surfacing an error to the user.
async fn fetch_latest() -> Option<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .user_agent(concat!("cyrene/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    Some(tag.trim_start_matches('v').to_owned())
}

/// Runs an async future to completion on a throwaway runtime. Returns `None` if
/// a runtime can't be created (the caller then simply skips the check).
fn block_on<F: std::future::Future>(fut: F) -> Option<F::Output> {
    tokio::runtime::Runtime::new()
        .ok()
        .map(|rt| rt.block_on(fut))
}

/// Returns the latest version, refreshing the cache over the network when it is
/// missing or older than [`CHECK_INTERVAL_SECS`]. `force` always refreshes.
fn latest_version(force: bool) -> Option<String> {
    let cached = read_cache();
    if !force {
        if let Some(c) = &cached {
            if now_secs().saturating_sub(c.last_check) < CHECK_INTERVAL_SECS {
                return Some(c.latest.clone());
            }
        }
    }
    match block_on(fetch_latest()).flatten() {
        Some(latest) => {
            write_cache(&latest);
            Some(latest)
        }
        // Network failed: fall back to whatever we last knew, if anything.
        None => cached.map(|c| c.latest),
    }
}

/// Prints a one-line notice if a newer release is available. Called at the
/// start of ordinary commands; rate-limited to one network check per day so it
/// is effectively free on most invocations.
pub fn maybe_notify() {
    // Only nag interactive users; never add latency to pipes, scripts, or CI.
    use std::io::IsTerminal;
    if !std::io::stderr().is_terminal() {
        return;
    }
    let Some(latest) = latest_version(false) else {
        return;
    };
    if is_newer(current_version(), &latest) {
        eprintln!(
            "  ↑ Cyrene {latest} is available (you have {}). Run `cyrene update` to upgrade.\n",
            current_version()
        );
    }
}

/// Implements `cyrene update`. With `check_only`, just reports status. Otherwise
/// confirms with the user and re-runs the official installer in place.
pub fn run_update(check_only: bool) {
    println!("Checking for updates… (current: {})", current_version());

    let Some(latest) = latest_version(true) else {
        println!("  Could not reach the update server. Check your connection and try again.");
        return;
    };

    if !is_newer(current_version(), &latest) {
        println!("  ✓ You're on the latest version ({}).", current_version());
        return;
    }

    println!(
        "  ↑ A new version is available: {latest} (you have {})",
        current_version()
    );

    if check_only {
        println!("  Run `cyrene update` to install it.");
        return;
    }

    let proceed = dialoguer::Confirm::new()
        .with_prompt(format!("Update to {latest} now?"))
        .default(true)
        .interact()
        .unwrap_or(false);

    if !proceed {
        println!("  Update cancelled.");
        return;
    }

    println!("  Running the installer…\n");
    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!("curl -fsSL {INSTALL_URL} | bash"))
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("\n  ✓ Updated to {latest}. Run `cyrene --version` to confirm.");
        }
        Ok(s) => {
            eprintln!("\n  ✗ Installer exited with status {s}. See output above.");
        }
        Err(e) => {
            eprintln!("\n  ✗ Could not run the installer: {e}");
            eprintln!("    Update manually: curl -fsSL {INSTALL_URL} | bash");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn detects_newer_versions() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.0", "v0.1.1"));
        assert!(is_newer("v1.0.0", "1.0.1"));
    }

    #[test]
    fn ignores_same_or_older() {
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.1.0"));
        assert!(!is_newer("1.0.0", "v1.0.0"));
    }

    #[test]
    fn unparseable_versions_are_no_update() {
        assert!(!is_newer("0.1.0", "not-a-version"));
        assert!(!is_newer("garbage", "0.2.0"));
    }
}
