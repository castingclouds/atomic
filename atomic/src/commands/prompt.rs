use std::io::Write;
use std::path::PathBuf;

use atomic_repository::Repository;
use clap::{Parser, ValueHint};
use libatomic::TxnT;

#[derive(Parser, Debug)]
pub struct Prompt {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,

    /// Format string for output. Available placeholders: {channel}, {repository}
    #[clap(long = "format", short = 'f')]
    format: Option<String>,

    /// Show only the channel name (equivalent to --format "{channel}")
    #[clap(long = "channel-only")]
    channel_only: bool,

    /// Show repository name
    #[clap(long = "show-repository")]
    show_repository: bool,
}

impl Prompt {
    pub fn run(self) -> Result<(), anyhow::Error> {
        // Try to find repository - if not in a repo, silently exit
        let repo = match Repository::find_root(self.repo_path) {
            Ok(repo) => repo,
            Err(_) => {
                // Not in a repository - output nothing for prompt integration
                return Ok(());
            }
        };

        // Get current channel
        let txn = repo.pristine.txn_begin()?;
        let channel_name = match txn.current_channel() {
            Ok(name) => name,
            Err(_) => {
                // No current channel - silently exit
                return Ok(());
            }
        };

        // Determine format string
        let format = if self.channel_only {
            "{channel}".to_string()
        } else if let Some(fmt) = self.format {
            fmt
        } else {
            // Load from config
            match atomic_config::Global::load() {
                Ok((config, _)) => {
                    if !config.prompt.enabled {
                        // Prompt integration disabled in config
                        return Ok(());
                    }
                    config.prompt.format
                }
                Err(_) => {
                    // Use default format if config can't be loaded
                    "[{channel}]".to_string()
                }
            }
        };

        // Get repository name if needed
        let repo_name = if self.show_repository || format.contains("{repository}") {
            repo.path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("atomic")
                .to_string()
        } else {
            String::new()
        };

        // Replace placeholders
        let output = format
            .replace("{channel}", channel_name)
            .replace("{repository}", &repo_name);

        // Output to stdout (no newline for prompt integration)
        let mut stdout = std::io::stdout();
        write!(stdout, "{}", output)?;
        stdout.flush()?;

        Ok(())
    }
}
