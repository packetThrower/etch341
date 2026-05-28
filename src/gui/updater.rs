//! Boot-time "newer release available" check against the project's
//! GitHub Releases.
//!
//! Detection only. We never download a replacement bundle or rewrite
//! anything on disk — the check finds the latest stable tag, compares
//! it to `CARGO_PKG_VERSION`, and if a newer release exists publishes
//! an [`UpdateState`] global that the chrome reads to paint an amber
//! dot on the Settings sidebar item. The Settings → Updates section
//! shows the version + a "View release" button that opens the
//! Releases page in the browser.
//!
//! etch341 ships stable-only (no `@alpha` channel), so the check hits
//! `/releases/latest` and ignores pre-releases entirely.
//!
//! The fetch is blocking (`ureq`); callers run it on a
//! `BackgroundExecutor` task so it can't stall the render thread.
//! Offline / parse failure resolves to `None` — the user never sees
//! a "check failed" diagnostic, exactly as if no update existed.

use std::time::Duration;

use serde::Deserialize;

/// Live state of the update check, installed as a gpui `Global` so
/// any render path can read it cheaply. `available == None` means
/// "no check completed yet" OR "already on the newest release" —
/// render code treats both identically (no dot painted).
#[derive(Debug, Clone, Default)]
pub struct UpdateState {
    pub available: Option<UpdateAvailable>,
}

impl gpui::Global for UpdateState {}

/// Available-update payload exposed to the chrome.
#[derive(Debug, Clone)]
pub struct UpdateAvailable {
    /// Bare tag string (no leading `v`), e.g. `"0.5.0"`.
    pub version: String,
    /// Browser URL for the GitHub Releases page entry.
    pub html_url: String,
}

/// Cheap read of the current available-update payload from the
/// global. `None` if the global isn't installed yet or no update is
/// pending. Used by the sidebar (dot) and Settings (Updates row).
pub fn available(cx: &gpui::App) -> Option<UpdateAvailable> {
    cx.try_global::<UpdateState>()
        .and_then(|s| s.available.clone())
}

/// GitHub Releases API response shape — only the fields we use.
#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Query the project's latest stable GitHub release and report it if
/// it's newer than `current_version`.
///
/// Returns `Ok(None)` when nothing is newer, the network call fails,
/// or the response doesn't parse. Returns `Err` only if
/// `current_version` doesn't parse as semver (can't happen — it's
/// `env!("CARGO_PKG_VERSION")`, which Cargo validates at build time).
///
/// Blocking — call from a `BackgroundExecutor` task.
pub fn check_for_update(current_version: &str) -> Result<Option<UpdateAvailable>, semver::Error> {
    let current = semver::Version::parse(current_version)?;
    let Some(release) = fetch_latest() else {
        return Ok(None);
    };
    // `/releases/latest` already excludes drafts + pre-releases, but
    // filter defensively in case the endpoint ever changes.
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let bare = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let Ok(latest) = semver::Version::parse(bare) else {
        return Ok(None);
    };
    if latest <= current {
        return Ok(None);
    }
    Ok(Some(UpdateAvailable {
        version: latest.to_string(),
        html_url: release.html_url,
    }))
}

/// Hit the GitHub Releases API for the latest stable release.
/// Returns `None` on any error — the caller treats that as "no
/// update", so the user never sees a failure diagnostic.
fn fetch_latest() -> Option<Release> {
    // 5s connect + 5s read: long enough that slow Wi-Fi doesn't
    // false-negative, short enough that a captive portal doesn't pin
    // the worker thread.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(5))
        .user_agent(concat!(
            "etch341/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/packetThrower/etch341)"
        ))
        .build();
    let response = agent
        .get("https://api.github.com/repos/packetThrower/etch341/releases/latest")
        .call()
        .ok()?;
    response.into_json::<Release>().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn current_version_parses_as_semver() {
        let raw = env!("CARGO_PKG_VERSION");
        semver::Version::parse(raw).expect("CARGO_PKG_VERSION must be valid semver");
    }
}
