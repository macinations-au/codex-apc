#![cfg(any(not(debug_assertions), test))]

use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

use codex_core::config::Config;
use codex_core::default_client::create_client;

use crate::version::CODEX_CLI_VERSION;

pub fn get_upgrade_version(config: &Config) -> Option<String> {
    if std::env::var("CODEX_DISABLE_UPDATE_CHECK").ok().as_deref() == Some("1") {
        return None;
    }
    let version_file = version_filepath(config);
    let info = read_version_info(&version_file).ok();

    if match &info {
        None => true,
        Some(info) => info.last_checked_at < Utc::now() - Duration::hours(20),
    } {
        // Refresh the cached latest version in the background so TUI startup
        // isn’t blocked by a network call. The UI reads the previously cached
        // value (if any) for this run; the next run shows the banner if needed.
        tokio::spawn(async move {
            check_for_update(&version_file)
                .await
                .inspect_err(|e| tracing::error!("Failed to update version: {e}"))
        });
    }

    let current = current_version();

    info.and_then(|info| {
        if is_newer(&info.latest_version, &current).unwrap_or(false) {
            Some(info.latest_version)
        } else {
            None
        }
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VersionInfo {
    latest_version: String,
    // ISO-8601 timestamp (RFC3339)
    last_checked_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug, Clone)]
struct ReleaseInfo {
    tag_name: String,
}

const VERSION_FILENAME: &str = "version.json";

fn latest_release_url() -> String {
    // Allow downstream launchers (e.g., codex-agentic) to override the
    // releases feed via env. Fallback to upstream Codex releases.
    std::env::var("CODEX_UPDATE_LATEST_URL")
        .unwrap_or_else(|_| "https://api.github.com/repos/openai/codex/releases/latest".to_string())
}

fn version_filepath(config: &Config) -> PathBuf {
    config.codex_home.join(VERSION_FILENAME)
}

fn read_version_info(version_file: &Path) -> anyhow::Result<VersionInfo> {
    let contents = std::fs::read_to_string(version_file)?;
    Ok(serde_json::from_str(&contents)?)
}

async fn check_for_update(version_file: &Path) -> anyhow::Result<()> {
    let ReleaseInfo {
        tag_name: latest_tag_name,
    } = create_client()
        .get(latest_release_url())
        .send()
        .await?
        .error_for_status()?
        .json::<ReleaseInfo>()
        .await?;

    let info = VersionInfo {
        latest_version: normalize_tag_name(&latest_tag_name)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse latest tag name '{latest_tag_name}'"))?
            .into(),
        last_checked_at: Utc::now(),
    };

    let json_line = format!("{}\n", serde_json::to_string(&info)?);
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, json_line).await?;
    Ok(())
}

fn is_newer(latest: &str, current: &str) -> Option<bool> {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => Some(l > c),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ParsedVersion {
    maj: u64,
    min: u64,
    pat: u64,
    // Optional downstream patch indicator: e.g. 0.39.0-apc.3 → ("apc", 3)
    // Only used as a tiebreaker when maj.min.pat are equal and both have
    // the same variant prefix.
    variant_num: Option<u64>,
}

fn parse_version(v: &str) -> Option<ParsedVersion> {
    let v = v.trim();
    let (core, suffix) = v
        .split_once('-')
        .map(|(a, b)| (a, Some(b)))
        .unwrap_or((v, None));
    let mut iter = core.split('.');
    let maj = iter.next()?.parse::<u64>().ok()?;
    let min = iter.next()?.parse::<u64>().ok()?;
    let pat = iter.next()?.parse::<u64>().ok()?;

    // Optional: recognize "apc.<y>" and capture y as downstream tiebreaker.
    // If there is any other suffix (e.g., beta, rc), treat the whole version as
    // unparsable to avoid making claims about prereleases.
    let variant_num = match suffix {
        None => None,
        Some(s) => {
            if let Some(rest) = s.strip_prefix("apc.") {
                rest.split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|n| n.parse::<u64>().ok())
            } else {
                return None;
            }
        }
    };

    Some(ParsedVersion {
        maj,
        min,
        pat,
        variant_num,
    })
}

fn normalize_tag_name(tag: &str) -> Option<&str> {
    // Accept tags like "rust-v0.39.0", "v0.39.0", or plain "0.39.0-apc.1".
    if let Some(s) = tag.strip_prefix("rust-v") {
        Some(s)
    } else if let Some(s) = tag.strip_prefix('v') {
        Some(s)
    } else if tag
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        Some(tag)
    } else {
        None
    }
}

fn current_version() -> String {
    // Allow downstream to provide its own version (e.g., codex-agentic’s
    // Cargo version), otherwise fall back to this crate’s package version.
    std::env::var("CODEX_CURRENT_VERSION").unwrap_or_else(|_| CODEX_CLI_VERSION.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prerelease_version_is_not_considered_newer() {
        assert_eq!(is_newer("0.11.0-beta.1", "0.11.0"), None);
        assert_eq!(is_newer("1.0.0-rc.1", "1.0.0"), None);
    }

    #[test]
    fn plain_semver_comparisons_work() {
        assert_eq!(is_newer("0.11.1", "0.11.0"), Some(true));
        assert_eq!(is_newer("0.11.0", "0.11.1"), Some(false));
        assert_eq!(is_newer("1.0.0", "0.9.9"), Some(true));
        assert_eq!(is_newer("0.9.9", "1.0.0"), Some(false));
    }

    #[test]
    fn whitespace_is_ignored() {
        assert_eq!(
            parse_version(" 1.2.3 \n").map(|p| (p.maj, p.min, p.pat)),
            Some((1, 2, 3))
        );
        assert_eq!(is_newer(" 1.2.3 ", "1.2.2"), Some(true));
    }

    #[test]
    fn apc_suffix_is_recognized_and_compares() {
        let a = parse_version("0.39.0-apc.1").unwrap();
        let b = parse_version("0.39.0-apc.2").unwrap();
        assert!(b > a);
        // Equal core version, only one side has apc suffix → treat as equal core,
        // no conclusion for is_newer (returns None) unless both parse.
        assert_eq!(is_newer("0.39.0-apc.2", "0.39.0"), Some(true));
        assert_eq!(is_newer("0.39.0", "0.39.0-apc.2"), Some(false));
    }
}
