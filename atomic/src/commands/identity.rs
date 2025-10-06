use atomic_config::{self as config, Author};
use atomic_identity::{self as identity, choose_identity_name, fix_identities, Complete};
use atomic_remote as remote;

use std::io::Write;

use anyhow::bail;
use chrono::{DateTime, Utc};
use clap::Parser;
use keyring::Entry;
use log::{info, warn};
use atomic_interaction::Confirm;
use ptree::{print_tree, TreeBuilder};

mod subcmd {
    use anyhow::bail;
    use chrono::{DateTime, Utc};
    use clap::{ArgGroup, Parser, ValueHint};

    fn validate_email(input: &str) -> Result<String, anyhow::Error> {
        if validator::validate_email(input) {
            Ok(input.to_string())
        } else {
            bail!("Invalid email address");
        }
    }

    fn valid_name(input: &str) -> Result<(), anyhow::Error> {
        if input.is_empty() {
            bail!("Name is empty");
        } else {
            Ok(())
        }
    }

    fn name_does_not_exist(input: &str) -> Result<String, anyhow::Error> {
        valid_name(&input)?;

        if atomic_identity::Complete::load(input).is_ok() {
            bail!("Name already exists")
        } else {
            Ok(input.to_string())
        }
    }

    fn name_exists(input: &str) -> Result<String, anyhow::Error> {
        valid_name(&input)?;

        if atomic_identity::Complete::load(input).is_err() {
            bail!("Name does not exist");
        } else {
            Ok(input.to_string())
        }
    }

    fn parse_expiry(input: &str) -> Result<DateTime<Utc>, anyhow::Error> {
        let parsed_date = dateparser::parse_with_timezone(input, &chrono::offset::Utc);
        if parsed_date.is_err() {
            bail!("Invalid date");
        }

        let date = parsed_date.unwrap();
        if chrono::offset::Utc::now().timestamp_millis() > date.timestamp_millis() {
            bail!("Date is in the past")
        } else {
            Ok(date)
        }
    }

    #[derive(Clone, Parser, Debug)]
    #[clap(group(
        ArgGroup::new("edit_data")
            .multiple(true)
            .args(&["display_name", "email", "expiry", "username", "remote", "name", "password"]),
    ))]
    pub struct New {
        /// Do not automatically link keys with the remote
        #[clap(long = "no-link", display_order = 1)]
        pub no_link: bool,
        /// Abort rather than prompt for input
        #[clap(long = "no-prompt", requires("edit_data"), display_order = 1)]
        pub no_prompt: bool,
        /// Set the username
        #[clap(long = "username", display_order = 3, value_hint = ValueHint::Username)]
        pub username: Option<String>,
        /// Set the default remote
        #[clap(long = "remote", display_order = 3)]
        pub remote: Option<String>,
        /// Set the display name
        #[clap(long = "display-name", display_order = 3)]
        pub display_name: Option<String>,
        /// Set the email
        #[clap(long = "email", value_parser = validate_email, display_order = 3)]
        pub email: Option<String>,
        /// Set the new identity name
        #[clap(value_parser = name_does_not_exist)]
        pub name: Option<String>,
        /// Set the expiry
        #[clap(long = "expiry", value_parser = parse_expiry, display_order = 3)]
        pub expiry: Option<DateTime<Utc>>,
        /// Encrypt using a password from standard input. Requires --no-prompt
        #[clap(long = "read-password", display_order = 2, requires = "no_prompt")]
        pub password: bool,
    }

    #[derive(Clone, Parser, Debug)]
    #[clap(group(
        ArgGroup::new("edit_data")
            .multiple(true)
            .args(&["display_name", "email", "new_name", "expiry", "username", "remote", "password"]),
    ))]
    pub struct Edit {
        /// Set the name of the identity to edit
        #[clap(group("name"), value_parser = name_exists)]
        pub old_name: Option<String>,
        /// Do not automatically link keys with the remote
        #[clap(long = "no-link", display_order = 1)]
        pub no_link: bool,
        /// Abort rather than prompt for input
        #[clap(
            long = "no-prompt",
            requires("name"),
            requires("edit_data"),
            display_order = 1
        )]
        pub no_prompt: bool,
        /// Set the username
        #[clap(long = "username", display_order = 3, value_hint = ValueHint::Username)]
        pub username: Option<String>,
        /// Set the default remote
        #[clap(long = "remote", display_order = 3)]
        pub remote: Option<String>,
        /// Set the display name
        #[clap(long = "display-name", display_order = 3)]
        pub display_name: Option<String>,
        /// Set the email
        #[clap(long = "email", value_parser = validate_email, display_order = 3)]
        pub email: Option<String>,
        /// Set the identity name
        #[clap(long = "new-name", display_order = 3)]
        pub new_name: Option<String>,
        /// Set the expiry
        #[clap(long = "expiry", value_parser = parse_expiry, display_order = 3)]
        pub expiry: Option<DateTime<Utc>>,
        /// Encrypt using a password from standard input. Requires --no-prompt
        #[clap(long = "read-password", display_order = 2, requires = "no_prompt")]
        pub password: bool,
    }
}

#[derive(Clone, Parser, Debug)]
pub enum SubCommand {
    /// Create a new identity
    New(subcmd::New),
    /// Repair the identity state on disk, including migration from older versions of atomic
    Repair,
    /// Prove an identity to the server
    Prove {
        /// Set the name used to prove the identity
        #[clap(long = "name")]
        identity_name: Option<String>,
        /// Set the target server
        server: Option<String>,
    },
    /// Pretty-print all valid identities on disk
    List,
    /// Edit an existing identity
    Edit(subcmd::Edit),
    /// Remove an existing identity
    #[clap(alias = "rm")]
    Remove {
        /// Set the name of the identity to remove
        #[clap(long = "name")]
        identity_name: Option<String>,
        /// Remove the matching identity without confirmation
        #[clap(long = "no-confirm")]
        no_confirm: bool,
    },
}

#[derive(Clone, Parser, Debug)]
pub struct IdentityCommand {
    #[clap(subcommand)]
    subcmd: SubCommand,
    /// Do not verify certificates (use with caution)
    #[clap(long = "no-cert-check", short = 'k')]
    no_cert_check: bool,
}

fn unwrap_args(
    default: Complete,
    identity_name: Option<String>,
    login: Option<String>,
    display_name: Option<String>,
    origin: Option<String>,
    email: Option<String>,
    expiry: Option<DateTime<Utc>>,
    password: bool,
) -> Result<Complete, anyhow::Error> {
    let pw = if password {
        Some(
            atomic_interaction::Password::new()?
                .with_prompt("New password")
                .with_confirmation("Confirm password", "Password mismatch")
                .interact()?,
        )
    } else {
        None
    };

    let credentials = if let Some(mut key) = default.secret_key() {
        key.expires = expiry;
        Some(identity::Credentials::new(key, pw))
    } else {
        None
    };

    Ok(Complete::new(
        identity_name.unwrap_or(default.name),
        identity::Config {
            key_path: None,
            author: Author {
                username: login.unwrap_or(default.config.author.username),
                display_name: display_name.unwrap_or(default.config.author.display_name),
                email: email.unwrap_or(default.config.author.email),
                origin: origin.unwrap_or(default.config.author.origin),
                key_path: None,
            },
        },
        default.public_key,
        credentials,
    ))
}

impl IdentityCommand {
    pub async fn run(self) -> Result<(), anyhow::Error> {
        let mut stderr = std::io::stderr();

        match self.subcmd {
            SubCommand::New(options) => {
                let identity = unwrap_args(
                    Complete::default()?,
                    options.name,
                    options.username,
                    options.display_name,
                    options.remote,
                    options.email,
                    options.expiry,
                    options.password,
                )?;

                identity.create(!options.no_link).await?;

                if let Err(_) = remote::prove(&identity, None, self.no_cert_check).await {
                    warn!("Could not prove identity `{}`. Please check your credentials & network connection. If you are on an enterprise network, perhaps try running with `--no-cert-check`. Your data is safe but will not be connected to {} without runnning `atomic identity prove {}`", identity.name, identity.config.author.origin, identity.name);
                } else {
                    info!("Identity `{}` was proved to the server", identity);
                }
            }
            SubCommand::Repair => fix_identities().await?,
            SubCommand::Prove {
                identity_name,
                server,
            } => {
                let identity_name = &identity_name.unwrap_or(choose_identity_name().await?);
                let loaded_identity = Complete::load(identity_name)?;
                remote::prove(&loaded_identity, server.as_deref(), self.no_cert_check).await?;
            }
            SubCommand::List => {
                let identities = Complete::load_all()?;

                if identities.is_empty() {
                    let mut stderr = std::io::stderr();
                    writeln!(
                        stderr,
                        "No identities found. Use `atomic identity new` to create one."
                    )?;
                    writeln!(stderr, "If you have created a key in the past, you may need to migrate via `atomic identity repair`")?;

                    return Ok(());
                }

                let mut tree = TreeBuilder::new("Identities".to_string());
                for identity in identities {
                    tree.begin_child(identity.name.clone());

                    tree.add_empty_child(format!(
                        "Display name: {}",
                        if identity.config.author.display_name.is_empty() {
                            "<NO NAME>"
                        } else {
                            &identity.config.author.display_name
                        }
                    ));

                    tree.add_empty_child(format!(
                        "Email: {}",
                        if identity.config.author.email.is_empty() {
                            "<NO EMAIL>"
                        } else {
                            &identity.config.author.email
                        }
                    ));

                    let username = if identity.config.author.username.is_empty() {
                        "<NO USERNAME>"
                    } else {
                        &identity.config.author.username
                    };

                    let origin = if identity.config.author.origin.is_empty() {
                        "<NO ORIGIN>"
                    } else {
                        &identity.config.author.origin
                    };
                    tree.add_empty_child(format!("Login: {username}@{origin}"));

                    tree.begin_child("Public key".to_string());
                    tree.add_empty_child(format!("Key: {}", identity.public_key.key));
                    tree.add_empty_child(format!("Version: {}", identity.public_key.version));
                    tree.add_empty_child(format!(
                        "Algorithm: {:#?}",
                        identity.public_key.algorithm
                    ));
                    tree.add_empty_child(format!(
                        "Expiry: {}",
                        identity
                            .public_key
                            .expires
                            .map(|date| date.format("%Y-%m-%d %H:%M:%S (UTC)").to_string())
                            .unwrap_or_else(|| "Never".to_string())
                    ));
                    tree.end_child();

                    tree.begin_child("Secret key".to_string());
                    tree.add_empty_child(format!(
                        "Version: {}",
                        identity.secret_key().unwrap().version
                    ));
                    tree.add_empty_child(format!(
                        "Algorithm: {:#?}",
                        identity.secret_key().unwrap().algorithm
                    ));

                    let encryption_message =
                        if let Some(encryption) = identity.secret_key().unwrap().encryption {
                            format!(
                                "{} (Stored in keyring: {})",
                                match encryption {
                                    libatomic::key::Encryption::Aes128(_) => "AES 128-bit",
                                },
                                keyring::Entry::new("atomic", &identity.name)?
                                    .get_password()
                                    .is_ok()
                            )
                        } else {
                            String::from("None")
                        };

                    tree.add_empty_child(format!("Encryption: {encryption_message}"));
                    tree.end_child();

                    tree.add_empty_child(format!(
                        "Last updated: {}",
                        identity.last_modified.format("%Y-%m-%d %H:%M:%S (UTC)")
                    ));
                    tree.end_child();
                }

                print_tree(&tree.build())?;
            }
            SubCommand::Edit(options) => {
                let old_id_name = if let Some(id_name) = options.old_name {
                    id_name
                } else {
                    choose_identity_name().await?
                };
                writeln!(std::io::stderr(), "Editing identity: {old_id_name}")?;

                let old_identity = Complete::load(&old_id_name)?;
                let cli_args = unwrap_args(
                    old_identity.clone(),
                    options.new_name,
                    options.username,
                    options.display_name,
                    options.remote,
                    options.email,
                    options.expiry,
                    options.password,
                )?;

                let new_identity = if options.no_prompt {
                    cli_args
                } else {
                    cli_args
                        .prompt_changes(Some(old_identity.name.clone()), !options.no_link)
                        .await?
                };

                old_identity.clone().replace_with(new_identity.clone())?;

                // There are 2 cases that require re-proving:
                // 1: new secret key
                // 2. new username/origin
                if !options.no_link {
                    if new_identity.secret_key() != old_identity.secret_key()
                        || old_identity.config.author != new_identity.config.author
                    {
                        let prove_result =
                            remote::prove(&new_identity, None, self.no_cert_check).await;

                        if let Err(_) = prove_result {
                            warn!("Could not prove identity `{}`. Please check your credentials & network connection. If you are on an enterprise network, perhaps try running with `--no-cert-check`. Your data is safe but will not be connected to {} without runnning `atomic identity prove {}`", new_identity.name, new_identity.config.author.origin, new_identity.name);
                        } else {
                            info!("Identity `{}` was proved to the server", new_identity);
                        }
                    }
                }
            }
            SubCommand::Remove {
                identity_name,
                no_confirm: no_prompt,
            } => {
                if Complete::load_all()?.is_empty() {
                    writeln!(stderr, "No identities to remove!")?;

                    return Ok(());
                }

                let identity =
                    Complete::load(&identity_name.unwrap_or(choose_identity_name().await?))?;
                let path = config::global_config_dir()
                    .unwrap()
                    .join("identities")
                    .join(&identity.name);

                writeln!(stderr, "Removing identity: {identity} at {path:?}")?;

                // Ask the user to confirm
                if !no_prompt
                    && !Confirm::new()?
                        .with_prompt("Do you wish to continue?")
                        .with_default(false)
                        .interact()?
                {
                    bail!("User did not wish to continue");
                }

                // The user has confirmed, safe to continue
                std::fs::remove_dir_all(path)?;
                writeln!(stderr, "Identity removed.")?;

                if identity.secret_key().unwrap().encryption.is_some() {
                    if let Err(e) =
                        Entry::new("atomic", &identity.name).and_then(|x| x.delete_password())
                    {
                        warn!("Unable to delete password: {e:?}");
                    }
                }
            }
        }

        Ok(())
    }
}
