use reqwest::Client;
use semver::Version;
use serde::Deserialize;
use std::fmt::Display;

const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/chanderlud/giga-grabber/releases/latest";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
pub(crate) const LOCAL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub(crate) enum Error {
    Http(reqwest::Error),
    Version(semver::Error),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error)
    }
}

impl From<semver::Error> for Error {
    fn from(error: semver::Error) -> Self {
        Self::Version(error)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(error) => write!(f, "failed to fetch latest release: {error}"),
            Self::Version(error) => write!(f, "failed to parse release version: {error}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateInfo {
    pub(crate) current_version: String,
    pub(crate) latest_version: String,
    pub(crate) tag: String,
    pub(crate) release_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Available(UpdateInfo),
    Current {
        current_version: String,
        latest_version: String,
    },
    Failed(String),
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

pub(crate) async fn check_latest_release() -> Outcome {
    match check_latest_release_with_client(&Client::new()).await {
        Ok(outcome) => outcome,
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

async fn check_latest_release_with_client(client: &Client) -> Result<Outcome, Error> {
    let release = fetch_latest_release(client).await?;
    outcome_from_release(LOCAL_VERSION, release)
}

async fn fetch_latest_release(client: &Client) -> Result<GitHubRelease, Error> {
    let release = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .json::<GitHubRelease>()
        .await?;

    Ok(release)
}

fn outcome_from_release(local_version: &str, release: GitHubRelease) -> Result<Outcome, Error> {
    let current = parse_version(local_version)?;
    let latest = parse_version(&release.tag_name)?;
    let current_version = current.to_string();
    let latest_version = latest.to_string();

    if latest > current {
        Ok(Outcome::Available(UpdateInfo {
            current_version,
            latest_version,
            tag: release.tag_name,
            release_url: release.html_url,
        }))
    } else {
        Ok(Outcome::Current {
            current_version,
            latest_version,
        })
    }
}

fn parse_version(version: &str) -> Result<Version, semver::Error> {
    let normalized = version
        .trim()
        .strip_prefix('v')
        .or_else(|| version.trim().strip_prefix('V'))
        .unwrap_or_else(|| version.trim());

    Version::parse(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag_name: &str, html_url: &str) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag_name.to_string(),
            html_url: html_url.to_string(),
        }
    }

    #[test]
    fn newer_leading_v_tag_returns_available_with_release_url() {
        let outcome = outcome_from_release(
            "1.3.2",
            release(
                "v1.3.3",
                "https://github.com/chanderlud/giga-grabber/releases/tag/v1.3.3",
            ),
        )
        .expect("valid release should compare");

        assert_eq!(
            outcome,
            Outcome::Available(UpdateInfo {
                current_version: "1.3.2".to_string(),
                latest_version: "1.3.3".to_string(),
                tag: "v1.3.3".to_string(),
                release_url: "https://github.com/chanderlud/giga-grabber/releases/tag/v1.3.3"
                    .to_string(),
            })
        );
    }

    #[test]
    fn equal_tag_returns_current() {
        let outcome = outcome_from_release(
            "1.3.2",
            release(
                "1.3.2",
                "https://github.com/chanderlud/giga-grabber/releases/tag/1.3.2",
            ),
        )
        .expect("valid release should compare");

        assert_eq!(
            outcome,
            Outcome::Current {
                current_version: "1.3.2".to_string(),
                latest_version: "1.3.2".to_string(),
            }
        );
    }

    #[test]
    fn older_tag_returns_current() {
        let outcome = outcome_from_release(
            "1.3.2",
            release(
                "v1.2.9",
                "https://github.com/chanderlud/giga-grabber/releases/tag/v1.2.9",
            ),
        )
        .expect("valid release should compare");

        assert_eq!(
            outcome,
            Outcome::Current {
                current_version: "1.3.2".to_string(),
                latest_version: "1.2.9".to_string(),
            }
        );
    }

    #[test]
    fn malformed_tag_returns_error() {
        let error = outcome_from_release(
            "1.3.2",
            release("release-latest", "https://example.com/release"),
        )
        .expect_err("malformed tag should fail");

        assert!(matches!(error, Error::Version(_)));
    }

    #[test]
    fn github_release_json_preserves_release_url() {
        let json = r#"{
            "tag_name": "v1.4.0",
            "html_url": "https://github.com/chanderlud/giga-grabber/releases/tag/v1.4.0"
        }"#;

        let release: GitHubRelease = serde_json::from_str(json).expect("json should parse");
        assert_eq!(release.tag_name, "v1.4.0");
        assert_eq!(
            release.html_url,
            "https://github.com/chanderlud/giga-grabber/releases/tag/v1.4.0"
        );
    }

    #[test]
    fn prerelease_comparison_uses_semver_ordering() {
        let outcome = outcome_from_release(
            "1.3.2",
            release(
                "v1.3.3-beta.1",
                "https://github.com/chanderlud/giga-grabber/releases/tag/v1.3.3-beta.1",
            ),
        )
        .expect("valid prerelease should compare");

        assert!(matches!(outcome, Outcome::Available(_)));
    }
}
