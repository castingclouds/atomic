use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, ValueHint};
use libatomic::changestore::ChangeStore;
use libatomic::*;

use atomic_repository::*;

#[derive(Parser, Debug)]
pub struct Change {
    /// Use the repository at PATH instead of the current directory
    #[clap(long = "repository", value_name = "PATH", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// The hash of the change to show, or an unambiguous prefix thereof
    #[clap(value_name = "HASH")]
    hash: Option<String>,
}

impl Change {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let repo = Repository::find_root(self.repo_path.clone())?;
        let txn = repo.pristine.txn_begin()?;
        let changes = repo.changes;

        let hash = if let Some(ref hash) = self.hash {
            if let Some(h) = Hash::from_base32(hash.as_bytes()) {
                h
            } else {
                txn.hash_from_prefix(hash)?.0
            }
        } else {
            let channel_name = txn.current_channel().unwrap_or(libatomic::DEFAULT_CHANNEL);
            let channel = if let Some(channel) = txn.load_channel(&channel_name)? {
                channel
            } else {
                return Ok(());
            };
            let channel = channel.read();
            if let Some(h) = txn.reverse_log(&*channel, None)?.next() {
                (h?.1).0.into()
            } else {
                return Ok(());
            }
        };
        let change = changes.get_change(&hash)?;

        // Check if this change has consolidating tag metadata
        if let Some(ref tag_metadata) = change.hashed.tag {
            // Display as a consolidating tag
            self.display_tag(&change, &hash, tag_metadata, &changes)?;
        } else {
            // Display as a regular change
            let colors = super::diff::is_colored(repo.config.pager.as_ref());
            change.write(
                &changes,
                Some(hash),
                true,
                super::diff::Colored {
                    w: termcolor::StandardStream::stdout(termcolor::ColorChoice::Auto),
                    colors,
                },
            )?;
        }
        Ok(())
    }

    fn display_tag<C: ChangeStore>(
        &self,
        change: &libatomic::change::Change,
        hash: &Hash,
        tag_metadata: &libatomic::change::TagMetadata,
        changes: &C,
    ) -> Result<(), anyhow::Error> {
        let mut stdout = std::io::stdout();

        // Header
        writeln!(
            stdout,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        )?;
        writeln!(stdout, "CONSOLIDATING TAG")?;
        writeln!(
            stdout,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        )?;
        writeln!(stdout)?;

        // Message and version
        writeln!(stdout, "Message: {}", change.hashed.header.message)?;
        if let Some(ref version) = tag_metadata.version {
            writeln!(stdout, "Version: {}", version)?;
        }
        writeln!(stdout, "Channel: {}", tag_metadata.channel)?;
        writeln!(stdout, "Hash:    {}", hash.to_base32())?;
        writeln!(stdout)?;

        // Author and timestamp
        if let Some(author) = change.hashed.header.authors.first() {
            writeln!(stdout, "Author:  {:?}", author)?;
        }
        writeln!(stdout, "Date:    {}", change.hashed.header.timestamp)?;
        writeln!(stdout)?;

        // Consolidation statistics
        writeln!(
            stdout,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        )?;
        writeln!(stdout, "CONSOLIDATION STATISTICS")?;
        writeln!(
            stdout,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        )?;
        writeln!(stdout)?;
        writeln!(
            stdout,
            "Consolidated Changes:     {}",
            tag_metadata.consolidated_change_count
        )?;
        writeln!(
            stdout,
            "Dependencies Before:      {}",
            tag_metadata.dependency_count_before
        )?;
        let effective_deps = if tag_metadata.previous_consolidation.is_some() {
            1
        } else {
            0
        };
        writeln!(stdout, "Effective Dependencies:   {}", effective_deps)?;
        let reduction = tag_metadata
            .dependency_count_before
            .saturating_sub(effective_deps);
        let reduction_pct = if tag_metadata.dependency_count_before > 0 {
            (reduction as f64 / tag_metadata.dependency_count_before as f64) * 100.0
        } else {
            0.0
        };
        writeln!(
            stdout,
            "Dependency Reduction:     {} ({:.1}%)",
            reduction, reduction_pct
        )?;
        writeln!(stdout)?;

        // Previous consolidation
        if let Some(ref prev) = tag_metadata.previous_consolidation {
            writeln!(stdout, "Previous Consolidation:   {}", prev.to_base32())?;
        }
        if let Some(ref since) = tag_metadata.consolidates_since {
            writeln!(stdout, "Consolidates Since:       {}", since.to_base32())?;
        }
        if let Some(ref created_by) = tag_metadata.created_by {
            writeln!(stdout, "Created By:               {}", created_by)?;
        }
        writeln!(stdout)?;

        // Consolidated changes list
        writeln!(
            stdout,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        )?;
        writeln!(
            stdout,
            "CONSOLIDATED CHANGES ({} total)",
            tag_metadata.consolidated_changes.len()
        )?;
        writeln!(
            stdout,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        )?;
        writeln!(stdout)?;

        for (i, change_hash) in tag_metadata.consolidated_changes.iter().enumerate() {
            let message = match changes.get_header(change_hash) {
                Ok(header) => header.message.lines().next().unwrap_or("").to_string(),
                Err(_) => "[unable to load change]".to_string(),
            };
            let short_hash = &change_hash.to_base32()[..12];
            writeln!(stdout, "  [{:3}] {}... - {}", i + 1, short_hash, message)?;
        }

        writeln!(stdout)?;
        writeln!(
            stdout,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        )?;

        Ok(())
    }
}
