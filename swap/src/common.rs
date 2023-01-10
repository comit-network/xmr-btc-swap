use anyhow::anyhow;

const LATEST_RELEASE_URL: &str = "https://github.com/comit-network/xmr-btc-swap/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Version {
    Current,
    Available,
}

/// Check the latest release from GitHub API.
pub async fn check_latest_version(current_version: &str) -> anyhow::Result<Version> {
    let response = reqwest::get(LATEST_RELEASE_URL).await?;
    let e = "Failed to get latest release.";
    let download_url = response.url();
    let segments = download_url.path_segments().ok_or_else(|| anyhow!(e))?;
    let latest_version = segments.last().ok_or_else(|| anyhow!(e))?;

    let result = if is_latest_version(current_version, latest_version) {
        Version::Current
    } else {
        tracing::warn!(%current_version, %latest_version, %download_url,
            "You are not on the latest version",
        );
        Version::Available
    };

    Ok(result)
}

// todo: naive implementation can be improved using semver
fn is_latest_version(current: &str, latest: &str) -> bool {
    current == latest
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn it_compares_versions() {
        assert!(is_latest_version("0.10.2", "0.10.2"));
        assert!(!is_latest_version("0.10.2", "0.10.3"));
        assert!(!is_latest_version("0.10.2", "0.11.0"));
    }

    #[tokio::test]
    #[ignore = "For local testing, makes http requests to github."]
    async fn it_compares_with_github() {
        let result = check_latest_version("0.11.0").await.unwrap();
        assert_eq!(result, Version::Available);

        let result = check_latest_version("0.11.1").await.unwrap();
        assert_eq!(result, Version::Current);
    }
}
