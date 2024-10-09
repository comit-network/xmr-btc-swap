pub mod tracing_util;

use std::{collections::HashMap, path::PathBuf};

use anyhow::anyhow;
use tokio::{
    fs::{read_dir, File},
    io::{AsyncBufReadExt, BufReader},
};
use uuid::Uuid;

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
                #[allow(clippy::redundant_closure_call)]
                $replacements.insert(address.as_str().to_owned(), $create_placeholder(counter));
                counter += 1;
            }
        }
    }};
}

/// Print the logs from the specified logs or from the default location
/// to the specified path or the terminal.
///
/// If specified, filter by swap id or redact addresses.
pub async fn get_logs(
    logs_dir: PathBuf,
    swap_id: Option<Uuid>,
    redact_addresses: bool,
) -> anyhow::Result<Vec<String>> {
    tracing::debug!("reading logfiles from {}", logs_dir.display());

    // get all files in the directory
    let mut log_files = read_dir(&logs_dir).await?;

    let mut log_messages = Vec::new();
    // when we redact we need to store the placeholder
    let mut placeholders = HashMap::new();

    // print all lines from every log file. TODO: sort files by date?
    while let Some(entry) = log_files.next_entry().await? {
        // get the file path
        let file_path = entry.path();

        // filter for .log files
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        if !file_name.ends_with(".log") {
            continue;
        }

        // use BufReader to stay easy on memory and then read line by line
        let buf_reader = BufReader::new(File::open(&file_path).await?);
        let mut lines = buf_reader.lines();

        // print each line, redacted if the flag is set
        while let Some(line) = lines.next_line().await? {
            // if we should filter by swap id, check if the line contains it
            if let Some(swap_id) = swap_id {
                // we only want lines which contain the swap id
                if !line.contains(&swap_id.to_string()) {
                    continue;
                }
            }

            // redact if necessary
            let line = if redact_addresses {
                redact_with(&line, &mut placeholders)
            } else {
                line
            };
            // save redacted message
            log_messages.push(line);
        }
    }

    Ok(log_messages)
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
    let mut replacements = HashMap::new();
    redact_with(input, &mut replacements)
}

/// Same as [`redact`] but retrieves palceholders from and stores them
/// in a specified hashmap.
pub fn redact_with(input: &str, replacements: &mut HashMap<String, String>) -> String {
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
