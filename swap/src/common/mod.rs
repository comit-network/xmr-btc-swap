pub mod tracing_util;

use std::collections::HashMap;

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

/// helper macro for [`redact`]... eldrich sorcery
/// the macro does in essence the following:
/// 1. create a static regex automaton for the pattern
/// 2. find all matching patterns using regex
/// 3. create a placeholder for each distinct matching pattern
/// 4. add the placeholder to the hashmap
macro_rules! regex_find_placeholders {
    ($pattern:expr, $create_placeholder:expr, $replacements:expr, $input:expr) => {{
        // compile the regex pattern
        static REGEX: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
            regex::Regex::new($pattern).expect("invalid regex pattern")
        });

        // keep count of count patterns to generate distinct placeholders
        let mut counter: usize = 0;

        // for every matched address check whether we already found it
        // and if we didn't, generate a placeholder for it
        for address in REGEX.find_iter($input) {
            if !$replacements.contains_key(address.as_str()) {
                $replacements.insert(address.as_str().to_owned(), $create_placeholder(counter));
                counter += 1;
            }
        }
    }};
}

/// Redact logs, etc. by replacing Bitcoin and Monero addresses
/// with generic placeholders.
///
/// # Example
/// ```rust
/// use swap::common::redact;
///
/// let redacted = redact("a9165a1e-d26d-4b56-bf6d-ca9658825c44");
/// assert_eq!(redacted, "<swap_id_0>");
/// ```
pub fn redact(input: &str) -> String {
    // Use a hashmap to keep track of which address we replace with which placeholder
    let mut replacements: HashMap<String, String> = HashMap::new();

    // TODO: verify regex patterns

    const MONERO_ADDR_REGEX: &str = r#"[48][1-9A-HJ-NP-Za-km-z]{94}"#;
    const BITCOIN_ADDR_REGEX: &str = r#"\b[13][a-km-zA-HJ-NP-Z1-9]{25,34}\b"#;
    // Both XMR and BTC transactions have
    // a 64 bit hex id so they aren't distinguishible
    const TX_ID_REGEX: &str = r#"\b[a-fA-F0-9]{64}\b"#;
    const SWAP_ID_REGEX: &str =
        r#"\b[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89aAbB][a-f0-9]{3}-[a-f0-9]{12}\b"#;

    // use the macro to find all addresses and generate placeholders
    // has to be a macro in order to create the regex automata only once.
    regex_find_placeholders!(
        MONERO_ADDR_REGEX,
        |count| format!("<monero_address_{count}>"),
        replacements,
        input
    );
    regex_find_placeholders!(
        BITCOIN_ADDR_REGEX,
        |count| format!("<bitcoin_address_{count}>"),
        replacements,
        input
    );
    regex_find_placeholders!(
        TX_ID_REGEX,
        |count| format!("<tx_id_{count}>"),
        replacements,
        input
    );
    regex_find_placeholders!(
        SWAP_ID_REGEX,
        |count| format!("<swap_id_{count}>"),
        replacements,
        input
    );

    // allocate string variable to operate on
    let mut redacted = input.to_owned();

    // Finally we go through the input string and replace each occurance of an
    // address we want to redact with the corresponding placeholder
    for (address, placeholder) in replacements.iter() {
        println!("replacing `{address}` with `{placeholder}`");
        redacted = redacted.replace(address, placeholder);
    }

    redacted
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
