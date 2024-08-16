pub mod tracing_util;

use std::{collections::HashMap, path::PathBuf};

use anyhow::anyhow;
use tokio::{fs::{create_dir_all, read_dir, try_exists, File}, io::{self, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader, Stdout}};
use uuid::Uuid;

use crate::fs::system_data_dir;

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

/// Print the logs from the specified logs or from the default location 
/// to the specified path or the terminal.
/// 
/// If specified, filter by swap id or redact addresses.
pub async fn print_or_write_logs(logs_dir: Option<PathBuf>, output_path: Option<PathBuf>, swap_id: Option<Uuid>, redact_addresses: bool) -> anyhow::Result<()> {
    // use provided directory of default one
    let default_dir = system_data_dir()?.join("logs");
    let logs_dir = logs_dir.unwrap_or(default_dir);

    tracing::info!("Reading `*.log` files from `{}`", logs_dir.display());

    // get all files in the directory
    let mut log_files = read_dir(&logs_dir).await?;

    /// Enum for abstracting over output channels
    enum OutputChannel {
        File(File),
        Stdout(Stdout),
    }

    /// Conveniance method for writing to either file or stdout
    async fn write_to_channel(
        mut channel: &mut OutputChannel,
        output: &str,
    ) -> Result<(), io::Error> {
        match &mut channel {
            OutputChannel::File(file) => file.write_all(output.as_bytes()).await,
            OutputChannel::Stdout(stdout) => stdout.write_all(output.as_bytes()).await,
        }
    }

    // check where we should write to
    let mut output_channel = match output_path {
        Some(path) => {
            // make sure the directory exists
            if !try_exists(&path).await? {
                let mut dir_part = path.clone();
                dir_part.pop();
                create_dir_all(&dir_part).await?;
            }

            tracing::info!("Writing logs to `{}`", path.display());

            // create/open and truncate file.
            // this means we aren't appending which is probably intuitive behaviour
            // since we reprint the complete logs anyway
            OutputChannel::File(File::create(&path).await?)
        }
        None => OutputChannel::Stdout(stdout()),
    };

    // conveniance method for checking whether we should filter a specific line
    let filter_by_swap_id: Box<dyn Fn(&str) -> bool + Send + Sync> = match swap_id {
        // if we should filter by swap id, check if the line contains the string
        Some(swap_id) => {
            let swap_id = swap_id.to_string();
            Box::new(move |line: &str| line.contains(&swap_id))
        }
        // otherwise we let every line pass
        None => Box::new(|_| true),
    };
    
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

        let buf_reader = BufReader::new(File::open(&file_path).await?);
        let mut lines = buf_reader.lines();

        // print each line, redacted if the flag is set
        while let Some(line) = lines.next_line().await? {
            // check if we should filter this line
            if !filter_by_swap_id(&line) {
                continue;
            }

            let line = if redact_addresses { redact(&line) } else { line };
            write_to_channel(&mut output_channel, &line).await?;
            // don't forget newlines
            write_to_channel(&mut output_channel, "\n").await?;
        }
    }

    Ok(())
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
