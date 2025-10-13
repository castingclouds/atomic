use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use canonical_path::{CanonicalPath, CanonicalPathBuf};
use chrono::Utc;
use clap::{Parser, ValueEnum, ValueHint};
use libatomic::attribution::SuggestionType;
use libatomic::change::*;
use libatomic::changestore::*;
use libatomic::{
    ArcTxn, Base32, ChannelMutTxnT, ChannelRef, ChannelTxnT, MutTxnTExt, TxnT, TxnTExt,
};
use libatomic::{HashMap, HashSet};
use log::debug;

use atomic_repository::*;

#[derive(Parser, Debug)]
pub struct Record {
    /// Open an editor to interactively edit the change before recording
    #[clap(short = 'e', long = "edit")]
    pub edit: bool,
    /// Set the change message
    #[clap(short = 'm', long = "message")]
    pub message: Option<String>,
    /// Set the description field.
    #[clap(long = "description")]
    pub description: Option<String>,
    /// Set the author field
    #[clap(long = "author")]
    pub author: Option<String>,
    /// Record the change in this channel instead of the current channel
    #[clap(long = "channel")]
    pub channel: Option<String>,
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    pub repo_path: Option<PathBuf>,
    /// Set the timestamp field
    #[clap(long = "timestamp", value_parser = parse_datetime_rfc2822)]
    pub timestamp: Option<i64>,
    /// Ignore missing (deleted) files
    #[clap(long = "ignore-missing")]
    pub ignore_missing: bool,
    #[clap(long = "working-copy")]
    pub working_copy: Option<String>,
    /// Amend this change instead of creating a new change
    #[clap(long = "amend")]
    #[allow(clippy::option_option)]
    pub amend: Option<Option<String>>,
    /// Paths in which to record the changes
    pub prefixes: Vec<PathBuf>,
    /// Identity to sign changes with
    #[clap(long = "identity")]
    pub identity: Option<String>,
    /// Use Patience diff instead of the default Myers diff
    #[clap(long = "patience")]
    pub patience: bool,
    /// Mark this change as AI-assisted
    #[clap(long = "ai-assisted")]
    pub ai_assisted: bool,
    /// Specify the AI provider (e.g., "openai", "anthropic", "github")
    #[clap(long = "ai-provider")]
    pub ai_provider: Option<String>,
    /// Specify the AI model (e.g., "gpt-4", "claude-3")
    #[clap(long = "ai-model")]
    pub ai_model: Option<String>,
    /// Specify the type of AI suggestion
    #[clap(long = "ai-suggestion-type", value_enum)]
    pub ai_suggestion_type: Option<AISuggestionType>,
    /// AI confidence score (0.0 to 1.0)
    #[clap(long = "ai-confidence")]
    pub ai_confidence: Option<f64>,
}

/// CLI enum for AI suggestion types
#[derive(Debug, Clone, ValueEnum)]
pub enum AISuggestionType {
    /// AI generated the entire patch
    Complete,
    /// AI suggested, human modified
    Partial,
    /// Human started, AI completed
    Collaborative,
    /// Human wrote based on AI suggestion
    Inspired,
    /// AI reviewed human code
    Review,
    /// AI refactored existing code
    Refactor,
}

impl From<AISuggestionType> for SuggestionType {
    fn from(cli_type: AISuggestionType) -> Self {
        match cli_type {
            AISuggestionType::Complete => SuggestionType::Complete,
            AISuggestionType::Partial => SuggestionType::Partial,
            AISuggestionType::Collaborative => SuggestionType::Collaborative,
            AISuggestionType::Inspired => SuggestionType::Inspired,
            AISuggestionType::Review => SuggestionType::Review,
            AISuggestionType::Refactor => SuggestionType::Refactor,
        }
    }
}

pub(crate) fn parse_datetime_rfc2822(s: &str) -> Result<i64, &'static str> {
    if let Ok(ts) = chrono::DateTime::parse_from_rfc2822(s) {
        return Ok(ts.timestamp());
    }
    if let Ok(t) = s.parse() {
        return Ok(t);
    }
    Err("Could not parse timestamp")
}

impl Record {
    pub async fn run(self) -> Result<(), anyhow::Error> {
        // Setup environment variables from CLI flags if provided
        if self.ai_assisted || self.ai_provider.is_some() {
            if self.ai_assisted {
                std::env::set_var("ATOMIC_AI_ENABLED", "true");
            }
            if let Some(ref provider) = self.ai_provider {
                std::env::set_var("ATOMIC_AI_PROVIDER", provider);
            }
            if let Some(ref model) = self.ai_model {
                std::env::set_var("ATOMIC_AI_MODEL", model);
            }
            if let Some(ref suggestion_type) = self.ai_suggestion_type {
                std::env::set_var(
                    "ATOMIC_AI_SUGGESTION_TYPE",
                    &format!("{:?}", suggestion_type).to_lowercase(),
                );
            }
            if let Some(confidence) = self.ai_confidence {
                std::env::set_var("ATOMIC_AI_CONFIDENCE", &confidence.to_string());
            }
        }

        let repo = Repository::find_root(self.repo_path.clone())?;
        let mut stdout = std::io::stdout();
        let mut stderr = std::io::stderr();

        for h in repo.config.hooks.record.iter() {
            h.run(repo.path.clone())?
        }
        let txn = repo.pristine.arc_txn_begin()?;
        let cur = txn
            .read()
            .current_channel()
            .unwrap_or(libatomic::DEFAULT_CHANNEL)
            .to_string();
        let channel = if let Some(ref c) = self.channel {
            c
        } else {
            cur.as_str()
        };
        let mut channel = if let Some(channel) = txn.read().load_channel(&channel)? {
            channel
        } else {
            bail!("Channel {:?} not found", channel);
        };

        let mut extra = Vec::new();
        for h in repo.config.extra_dependencies.iter() {
            let (h, c) = txn.read().hash_from_prefix(h)?;
            if txn
                .read()
                .get_changeset(txn.read().changes(&*channel.read()), &c)?
                .is_none()
            {
                bail!(
                    "Change {:?} (from .atomic/config) is not on channel {:?}",
                    h,
                    channel.read().name
                )
            }
            extra.push(h)
        }

        let header = if let Some(ref amend) = self.amend {
            let h = if let Some(ref hash) = amend {
                txn.read().hash_from_prefix(hash)?.0
            } else if let Some(h) = txn.read().reverse_log(&*channel.read(), None)?.next() {
                (h?.1).0.into()
            } else {
                return Ok(());
            };
            let header = if let Some(message) = self.message.clone() {
                ChangeHeader {
                    message,
                    ..repo.changes.get_header(&h)?
                }
            } else {
                repo.changes.get_header(&h)?
            };

            txn.write().unrecord(
                &repo.changes,
                &mut channel,
                &h,
                self.timestamp.unwrap_or(0) as u64,
            )?;
            header
        } else {
            self.header().await?
        };
        let no_prefixes =
            self.prefixes.is_empty() && !self.ignore_missing && self.working_copy.is_none();
        let (repo_path, working_copy) = if let Some(ref w) = self.working_copy {
            (
                CanonicalPathBuf::canonicalize(w)?,
                Some(libatomic::working_copy::filesystem::FileSystem::from_root(
                    w,
                )),
            )
        } else {
            (CanonicalPathBuf::canonicalize(&repo.path)?, None)
        };

        let complete =
            atomic_identity::Complete::load(&atomic_identity::choose_identity_name().await?)?;

        let (secret, _) = complete.decrypt()?;

        txn.write()
            .apply_root_change_if_needed(&repo.changes, &channel, rand::thread_rng())?;

        let result = self.record(
            txn,
            channel.clone(),
            working_copy.as_ref().unwrap_or(&repo.working_copy),
            &repo.changes,
            repo_path,
            header,
            &extra,
            &repo.config,
            &repo.pristine,
        )?;
        match result {
            Either::A((txn, mut change, updates, oldest)) => {
                // Add AI attribution metadata BEFORE saving the change
                if let Some(attribution) = libatomic::helpers::create_attribution_from_env() {
                    if let Ok(serialized_attribution) =
                        libatomic::helpers::serialize_attribution_for_metadata(&attribution)
                    {
                        change.hashed.metadata = serialized_attribution;
                    }
                }

                let hash = repo.changes.save_change(&mut change, |change, hash| {
                    change.unhashed = Some(serde_json::json!({
                        "signature": secret.sign_raw(&hash.to_bytes()).unwrap(),
                    }));
                    Ok::<_, anyhow::Error>(())
                })?;

                let mut txn_ = txn.write();
                txn_.apply_local_change(&mut channel, &change, &hash, &updates)?;
                let mut path = repo.path.join(libatomic::DOT_DIR);
                path.push("identities");
                std::fs::create_dir_all(&path)?;

                writeln!(stdout, "Hash: {}", hash.to_base32())?;
                debug!("oldest = {:?}", oldest);
                if no_prefixes {
                    let mut oldest = oldest
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    if oldest == 0 {
                        // If no diff was done at all, it means that no
                        // existing file changed since last time (some
                        // files may have been added, deleted or moved,
                        // but `touch` isn't about those).
                        oldest = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;
                    }
                    txn_.touch_channel(&mut *channel.write(), Some((oldest / 1000) * 1000));
                }
                std::mem::drop(txn_);
                txn.commit()?;
            }
            Either::B(txn) => {
                if no_prefixes {
                    txn.write().touch_channel(&mut *channel.write(), None);
                    txn.commit()?;
                }
                writeln!(stderr, "Nothing to record")?;
            }
        }
        Ok(())
    }

    async fn header(&self) -> Result<ChangeHeader, anyhow::Error> {
        let config = atomic_config::Global::load();
        let mut authors = Vec::new();
        let mut b = std::collections::BTreeMap::new();
        if let Some(ref a) = self.author {
            b.insert("name".to_string(), a.clone());
        } else {
            let identity_name = self
                .identity
                .clone()
                .unwrap_or(atomic_identity::choose_identity_name().await?);

            let public_key = atomic_identity::public_key(&identity_name);
            b.insert("key".to_string(), public_key?.key);
        }

        authors.push(Author(b));
        let templates = config
            .as_ref()
            .ok()
            .and_then(|(cfg, _)| cfg.template.as_ref());
        let message = if let Some(message) = &self.message {
            message.clone()
        } else if let Some(message_file) = templates.and_then(|t| t.message.as_ref()) {
            match std::fs::read_to_string(message_file) {
                Ok(m) => m,
                Err(e) => bail!("Could not read message template: {:?}: {}", message_file, e),
            }
        } else {
            String::new()
        };
        let description = if let Some(description) = &self.description {
            Some(description.clone())
        } else if let Some(descr_file) = templates.and_then(|t| t.description.as_ref()) {
            match std::fs::read_to_string(descr_file) {
                Ok(d) => Some(d),
                Err(e) => bail!(
                    "Could not read description template: {:?}: {}",
                    descr_file,
                    e
                ),
            }
        } else {
            None
        };
        let header = ChangeHeader {
            message,
            authors,
            description,
            timestamp: if let Some(t) = self.timestamp {
                chrono::DateTime::from_timestamp(t, 0).unwrap()
            } else {
                Utc::now()
            },
        };
        Ok(header)
    }

    fn fill_relative_prefixes(&mut self) -> Result<(), anyhow::Error> {
        let cwd = std::env::current_dir()?;
        for p in self.prefixes.iter_mut() {
            if p.is_relative() {
                *p = cwd.join(&p);
            }
        }
        Ok(())
    }

    fn record<
        T: TxnTExt + MutTxnTExt + Sync + Send + 'static,
        C: ChangeStore + Send + Clone + 'static,
    >(
        mut self,
        txn: ArcTxn<T>,
        channel: ChannelRef<T>,
        working_copy: &libatomic::working_copy::FileSystem,
        changes: &C,
        repo_path: CanonicalPathBuf,
        header: ChangeHeader,
        extra_deps: &[libatomic::Hash],
        _repo_config: &atomic_config::Config,
        _pristine: &libatomic::pristine::sanakirja::Pristine,
    ) -> Result<
        Either<
            (
                ArcTxn<T>,
                Change,
                HashMap<usize, libatomic::InodeUpdate>,
                std::time::SystemTime,
            ),
            ArcTxn<T>,
        >,
        anyhow::Error,
    > {
        let mut state = libatomic::RecordBuilder::new();
        if self.ignore_missing {
            state.ignore_missing = true;
        }
        if self.prefixes.is_empty() {
            if self.ignore_missing {
                for f in ignore::Walk::new(&repo_path) {
                    let f = f?;
                    if f.metadata()?.is_file() {
                        let p = CanonicalPath::new(f.path())?;
                        let p = p.as_path().strip_prefix(&repo_path).unwrap();
                        state.record(
                            txn.clone(),
                            if self.patience {
                                libatomic::Algorithm::Patience
                            } else {
                                libatomic::Algorithm::default()
                            },
                            false,
                            &libatomic::DEFAULT_SEPARATOR,
                            channel.clone(),
                            working_copy,
                            changes,
                            p.to_str().unwrap(),
                            1, // std::thread::available_parallelism()?.get(),
                        )?
                    }
                }
            } else {
                state.record(
                    txn.clone(),
                    if self.patience {
                        libatomic::Algorithm::Patience
                    } else {
                        libatomic::Algorithm::default()
                    },
                    false,
                    &libatomic::DEFAULT_SEPARATOR,
                    channel.clone(),
                    working_copy,
                    changes,
                    "",
                    1, // std::thread::available_parallelism()?.get(),
                )?
            }
        } else {
            self.fill_relative_prefixes()?;
            working_copy.record_prefixes(
                txn.clone(),
                if self.patience {
                    libatomic::Algorithm::Patience
                } else {
                    libatomic::Algorithm::default()
                },
                channel.clone(),
                changes,
                &mut state,
                repo_path,
                &self.prefixes,
                false,
                1, // std::thread::available_parallelism()?.get(),
                self.timestamp.unwrap_or(0) as u64,
            )?;
        }

        let mut rec = state.finish();
        if rec.actions.is_empty() {
            return Ok(Either::B(txn));
        }

        if rec.has_binary_files && self.edit {
            bail!("Cannot record a binary change interactively. Please remove -e flag.")
        }

        debug!("TAKING LOCK {}", line!());
        let txn_ = txn.write();
        let actions = rec
            .actions
            .into_iter()
            .map(|rec| rec.globalize(&*txn_).unwrap())
            .collect();
        let contents = if let Ok(c) = Arc::try_unwrap(rec.contents) {
            c.into_inner()
        } else {
            unreachable!()
        };

        let mut change =
            LocalChange::make_change(&*txn_, &channel, actions, contents, header, Vec::new())?;

        let current: HashSet<_> = change.dependencies.iter().cloned().collect();
        for dep in extra_deps.iter() {
            if !current.contains(dep) {
                change.dependencies.push(*dep)
            }
        }

        debug!("has_binary = {:?}", rec.has_binary_files);
        let mut change = if self.edit {
            let mut o = Vec::new();
            debug!("write change");
            change.write(changes, None, true, &mut o)?;
            debug!("write change done");

            let mut with_errors: Option<Vec<u8>> = None;
            let mut editor_attempts = 0;
            const MAX_EDITOR_ATTEMPTS: usize = 3;

            let mut change = loop {
                editor_attempts += 1;
                if editor_attempts > MAX_EDITOR_ATTEMPTS {
                    bail!("Too many failed editor attempts. Remove -e flag to skip interactive editing, or check your EDITOR configuration.");
                }

                let bytes_result = if let Some(ref o) = with_errors {
                    edit::edit_bytes_with_builder(&o[..], tempfile::Builder::new().suffix(".toml"))
                } else {
                    edit::edit_bytes_with_builder(&o[..], tempfile::Builder::new().suffix(".toml"))
                };

                let mut bytes = match bytes_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        bail!("Editor failed: {}. Remove -e flag to skip interactive editing, or check your EDITOR configuration.", e);
                    }
                };

                if bytes.iter().all(|c| (*c as char).is_whitespace()) {
                    bail!("Empty change")
                }
                let mut change = std::io::BufReader::new(std::io::Cursor::new(&bytes));
                if let Ok(change) =
                    Change::read_and_deps(&mut change, &mut rec.updatables, &*txn_, &channel)
                {
                    break change;
                }

                let mut err = SYNTAX_ERROR.as_bytes().to_vec();
                err.append(&mut bytes);
                with_errors = Some(err);

                eprintln!(
                    "Failed to parse change (attempt {}/{}). Please fix the syntax and try again.",
                    editor_attempts, MAX_EDITOR_ATTEMPTS
                );
            };
            if change.changes.is_empty() {
                bail!("Cannot parse change")
            }

            // Merge CLI-provided attribution flags with editor content
            // CLI flags take precedence over what was in the editor
            if self.ai_assisted
                || self.ai_provider.is_some()
                || self.ai_model.is_some()
                || self.ai_suggestion_type.is_some()
                || self.ai_confidence.is_some()
            {
                // Read existing metadata from editor if present
                let existing_attribution = if !change.hashed.metadata.is_empty() {
                    libatomic::helpers::deserialize_attribution_from_metadata(
                        &change.hashed.metadata,
                    )
                    .ok()
                } else {
                    None
                };

                // Override with CLI flags
                if let Some(mut attr) = existing_attribution {
                    if self.ai_assisted {
                        attr.ai_assisted = true;
                    }
                    if let Some(ref provider) = self.ai_provider {
                        if let Some(ref mut ai_meta) = attr.ai_metadata {
                            ai_meta.provider = provider.clone();
                        }
                    }
                    if let Some(ref model) = self.ai_model {
                        if let Some(ref mut ai_meta) = attr.ai_metadata {
                            ai_meta.model = model.clone();
                        }
                    }
                    if let Some(ref suggestion_type) = self.ai_suggestion_type {
                        if let Some(ref mut ai_meta) = attr.ai_metadata {
                            ai_meta.suggestion_type = suggestion_type.clone().into();
                        }
                    }
                    if let Some(confidence) = self.ai_confidence {
                        attr.confidence = Some(confidence);
                    }

                    // Serialize updated attribution
                    if let Ok(metadata_bytes) =
                        libatomic::helpers::serialize_attribution_for_metadata(&attr)
                    {
                        change.hashed.metadata = metadata_bytes;
                    }
                } else {
                    // No existing attribution, create from CLI flags
                    if let Some(attribution) = libatomic::helpers::create_attribution_from_env() {
                        if let Ok(metadata_bytes) =
                            libatomic::helpers::serialize_attribution_for_metadata(&attribution)
                        {
                            change.hashed.metadata = metadata_bytes;
                        }
                    }
                }
            }

            change
        } else {
            change
        };

        let current: HashSet<_> = change.dependencies.iter().cloned().collect();
        for dep in extra_deps.iter() {
            if !current.contains(dep) {
                change.dependencies.push(*dep)
            }
        }

        if change.header.message.trim().is_empty() {
            bail!("No change message")
        }
        debug!("saving change");
        std::mem::drop(txn_);
        Ok(Either::A((txn, change, rec.updatables, rec.oldest_change)))
    }
}

enum Either<A, B> {
    A(A),
    B(B),
}

const SYNTAX_ERROR: &str = "# Syntax errors, please try again.
# Alternatively, you may delete the entire file (including this
# comment) to abort.
";
