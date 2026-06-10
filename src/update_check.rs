use anyhow::{Context, Result};
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt::Display;

const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/chanderlud/giga-grabber/releases/latest";
#[cfg(test)]
const LATEST_RELEASE_PAGE_URL: &str = "https://github.com/chanderlud/giga-grabber/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseInfo {
    pub(crate) version: String,
    pub(crate) url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    Available(ReleaseInfo),
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateCheckError {
    message: String,
}

impl UpdateCheckError {
    pub(crate) fn new(error: impl Display) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl Display for UpdateCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

pub(crate) async fn check_latest_release() -> Result<UpdateStatus> {
    let release: GitHubRelease = reqwest::Client::new()
        .get(LATEST_RELEASE_API_URL)
        .header(
            USER_AGENT,
            concat!("giga-grabber/", env!("CARGO_PKG_VERSION")),
        )
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .context("requesting latest GitHub release")?
        .error_for_status()
        .context("GitHub latest release request failed")?
        .json()
        .await
        .context("parsing latest GitHub release")?;

    Ok(status_for_versions(env!("CARGO_PKG_VERSION"), release))
}

fn status_for_versions(current: &str, release: GitHubRelease) -> UpdateStatus {
    match compare_versions(&release.tag_name, current) {
        Ordering::Greater => UpdateStatus::Available(ReleaseInfo {
            version: release.tag_name,
            url: release.html_url,
        }),
        Ordering::Equal | Ordering::Less => UpdateStatus::Current,
    }
}

fn compare_versions(candidate: &str, current: &str) -> Ordering {
    let candidate_parts = version_parts(candidate);
    let current_parts = version_parts(current);
    let width = candidate_parts.len().max(current_parts.len());

    for index in 0..width {
        let candidate_part = candidate_parts.get(index).copied().unwrap_or_default();
        let current_part = current_parts.get(index).copied().unwrap_or_default();

        match candidate_part.cmp(&current_part) {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }

    Ordering::Equal
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(['.', '-'])
        .map(|part| part.parse::<u64>().unwrap_or_default())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_newer_v_prefixed_tag() {
        let release = GitHubRelease {
            tag_name: "v1.4.0".to_string(),
            html_url: LATEST_RELEASE_PAGE_URL.to_string(),
        };

        assert_eq!(
            status_for_versions("1.3.2", release),
            UpdateStatus::Available(ReleaseInfo {
                version: "v1.4.0".to_string(),
                url: LATEST_RELEASE_PAGE_URL.to_string(),
            })
        );
    }

    #[test]
    fn treats_matching_tag_as_current() {
        let release = GitHubRelease {
            tag_name: "v1.3.2".to_string(),
            html_url: LATEST_RELEASE_PAGE_URL.to_string(),
        };

        assert_eq!(status_for_versions("1.3.2", release), UpdateStatus::Current);
    }

    #[test]
    fn treats_older_release_as_current() {
        let release = GitHubRelease {
            tag_name: "v1.2.9".to_string(),
            html_url: LATEST_RELEASE_PAGE_URL.to_string(),
        };

        assert_eq!(status_for_versions("1.3.2", release), UpdateStatus::Current);
    }
}
