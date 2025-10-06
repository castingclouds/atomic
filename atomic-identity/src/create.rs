use super::load::path;
use super::Complete;

use std::io::Write;
use std::{fs, path::PathBuf};

use anyhow::{bail, Context};
use keyring::Entry;
use log::{debug, warn};
use atomic_interaction::{Confirm, Input, Select};
use thrussh_keys::key::PublicKey;

impl Complete {
    /// Prompt the user to make changes to an identity, returning the new identity
    ///
    /// # Arguments
    /// * `replace_current` - The identity to replace
    pub async fn prompt_changes(
        &self,
        to_replace: Option<String>,
        link_remote: bool,
    ) -> Result<Self, anyhow::Error> {
        let mut new_identity = self.clone();
        let will_replace = to_replace.is_some();

        new_identity.name = Input::new()?
            .with_prompt("Unique identity name")
            .with_default(String::from("default"))
            .with_allow_empty(false)
            .with_initial_text(if will_replace {
                self.name.clone()
            } else {
                String::new()
            })
            .with_validator(move |input: &String| -> Result<(), String> {
                if input.contains(['/', '\\', '.']) {
                    return Err("Name contains illegal characters".to_string());
                }

                match Self::load(input) {
                    Ok(existing_identity) => {
                        if let Some(name) = &to_replace {
                            if name == input {
                                // The user is trying to edit an existing identity
                                Ok(())
                            } else {
                                // The user is editing an existing identity but trying to overwrite a different name
                                Err(format!("The identity {existing_identity} already exists. Either remove the identity or edit it directly."))
                            }
                        } else {
                            // The user is creating a new identity but trying to use an existing name
                            Err(format!("The identity {existing_identity} already exists. Either remove the identity or edit it directly."))
                        }
                    }
                    Err(_) => Ok(()),
                }
            })
            .interact()?;

        new_identity.config.author.display_name = Input::new()?
            .with_prompt("Display name")
            .with_allow_empty(true)
            .with_initial_text(&self.config.author.display_name)
            .interact()?;

        new_identity.config.author.email = Input::new()?
            .with_prompt("Email (leave blank for none)")
            .with_allow_empty(true)
            .with_initial_text(&self.config.author.email)
            .with_validator(move |input: &String| -> Result<(), &str> {
                if input.is_empty() || validator::validate_email(input) {
                    Ok(())
                } else {
                    Err("Invalid email address")
                }
            })
            .interact()?;

        if Confirm::new()?
            .with_prompt(&format!(
                "Do you want to change the encryption? (Current status: {})",
                self.credentials
                    .clone()
                    .unwrap()
                    .secret_key
                    .encryption
                    .map_or("not encrypted", |_| "encrypted")
            ))
            .with_default(false)
            .interact()?
        {
            new_identity.change_password()?;
        }

        // Update the expiry AFTER potential secret key reset
        new_identity.prompt_expiry()?;

        if link_remote {
            if Confirm::new()?
                .with_prompt("Do you want to link this identity to a remote?")
                .with_default(true)
                .interact()?
            {
                new_identity.prompt_remote().await?;
            } else {
                // The user wants an 'offline' identity, so make sure not to store login info
                new_identity.config.key_path = None;
                new_identity.config.author.username = String::new();
                new_identity.config.author.origin = String::new();
            }
        }

        new_identity.last_modified = chrono::offset::Utc::now();

        Ok(new_identity)
    }

    async fn prompt_ssh(&mut self) -> Result<(), anyhow::Error> {
        let mut ssh_agent = thrussh_keys::agent::client::AgentClient::connect_env().await?;
        let identities = ssh_agent.request_identities().await?;
        let ssh_dir = dirs_next::home_dir().unwrap().join(".ssh");

        let selection = Select::new()?
            .with_prompt("Select key")
            .with_items(
                &identities
                    .iter()
                    .map(|id| {
                        format!(
                            "{}: {} ({})",
                            id.name(),
                            id.fingerprint(),
                            ssh_dir
                                .join(match id {
                                    PublicKey::Ed25519(_) =>
                                        thrussh_keys::key::ED25519.identity_file(),
                                    PublicKey::RSA { ref hash, .. } => hash.name().identity_file(),
                                })
                                .display(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .with_default(0 as usize)
            .interact()?;

        self.config.key_path = Some(ssh_dir.join(match identities[selection] {
            PublicKey::Ed25519(_) => thrussh_keys::key::ED25519.identity_file(),
            PublicKey::RSA { ref hash, .. } => hash.name().identity_file(),
        }));

        Ok(())
    }

    async fn prompt_remote(&mut self) -> Result<(), anyhow::Error> {
        self.config.author.username = Input::new()?
            .with_prompt("Remote username")
            .with_default(whoami::username())
            .with_initial_text(&self.config.author.username)
            .interact()?;

        self.config.author.origin = Input::new()?
            .with_prompt("Remote URL")
            .with_initial_text(&self.config.author.origin)
            .with_default(String::from("ssh.atomic.com"))
            .interact()?;

        if Confirm::new()?
            .with_prompt(&format!(
                "Do you want to change the default SSH key? (Current key: {})",
                if let Some(path) = &self.config.key_path {
                    format!("{path:#?}")
                } else {
                    String::from("none")
                }
            ))
            .with_default(false)
            .interact()?
        {
            self.prompt_ssh().await?;
        }

        debug!("prompt remote {:?}", self.config.author);

        Ok(())
    }

    fn prompt_expiry(&mut self) -> Result<(), anyhow::Error> {
        let expiry_message = self
            .public_key
            .expires
            .map(|date| date.format("%Y-%m-%d %H:%M:%S").to_string());

        self.public_key.expires = if Confirm::new()?
            .with_prompt(format!(
                "Do you want this key to expire? (Current expiry: {})",
                expiry_message
                    .clone()
                    .unwrap_or_else(|| String::from("never"))
            ))
            .with_default(false)
            .interact()?
        {
            let time_stamp: String = Input::new()?
                .with_prompt("Expiry date (YYYY-MM-DD)")
                .with_initial_text(expiry_message.unwrap_or_default())
                .with_validator(move |input: &String| -> Result<(), &str> {
                    let parsed_date = dateparser::parse_with_timezone(input, &chrono::offset::Utc);
                    if parsed_date.is_err() {
                        return Err("Invalid date");
                    }

                    let date = parsed_date.unwrap();
                    if chrono::offset::Utc::now().timestamp_millis() > date.timestamp_millis() {
                        Err("Date is in the past")
                    } else {
                        Ok(())
                    }
                })
                .interact()?;

            Some(dateparser::parse_with_timezone(
                &time_stamp,
                &chrono::offset::Utc,
            )?)
        } else {
            None
        };

        Ok(())
    }

    fn write_config(&self, identity_dir: &PathBuf) -> Result<(), anyhow::Error> {
        let config_data = toml::to_string_pretty(&self)?;
        let mut config_file = std::fs::File::create(identity_dir.join("identity.toml"))?;
        config_file.write_all(config_data.as_bytes())?;

        Ok(())
    }

    fn write_secret_key(&self, identity_dir: &PathBuf) -> Result<(), anyhow::Error> {
        let key_data = serde_json::to_string_pretty(&self.secret_key())?;
        let mut key_file = std::fs::File::create(&identity_dir.join("secret_key.json"))?;
        key_file.write_all(key_data.as_bytes())?;

        Ok(())
    }

    /// Write a complete identity to disk.
    fn write(&self) -> Result<(), anyhow::Error> {
        if let Ok(existing_identity) = Self::load(&self.name) {
            bail!("An identity with that name already exists: {existing_identity}");
        }

        // Write the relevant identity files
        let identity_dir = path(&self.name, false)?;

        std::fs::create_dir_all(&identity_dir)?;
        self.write_config(&identity_dir)?;
        self.write_secret_key(&identity_dir)?;

        Ok(())
    }

    /// Create a complete identity, including writing to disk & exchanging key with origin.
    ///
    /// # Arguments
    /// * `link_remote` - Override if the identity should be exchanged with the remote.
    pub async fn create(&self, link_remote: bool) -> Result<(), anyhow::Error> {
        // Prompt the user to edit changes interactively
        let confirmed_identity = self.prompt_changes(None, link_remote).await?;
        confirmed_identity.write()?;

        Ok(())
    }

    /// Replace an existing identity with a new one.
    ///
    /// # Arguments
    /// * `new_identity` - The new identity that will be created
    pub fn replace_with(self, new_identity: Self) -> Result<Self, anyhow::Error> {
        let changed_names = self.name != new_identity.name;

        // If changing the identity name, remove old directory
        if changed_names {
            let old_identity_path = path(&self.name, true)?;
            debug!("Removing old directory: {old_identity_path:?}");
            fs::remove_dir_all(old_identity_path).context("Could not remove old identity.")?;

            let new_identity_path = path(&new_identity.name, false)?;
            debug!("Creating new directory: {new_identity_path:?}");
            fs::create_dir_all(new_identity_path).context("Could not create new identity.")?;

            new_identity.write()?;

            // Delete the existing password
            if let Err(e) = Entry::new("atomic", &self.name).and_then(|x| x.delete_password()) {
                warn!("Unable to delete password: {e:?}");
            }
        } else {
            // Write only the new data
            let identity_dir = path(&new_identity.name, false)?;
            if self.config != new_identity.config {
                new_identity.write_config(&identity_dir)?;
            }
            if self.secret_key() != new_identity.secret_key() {
                new_identity.write_secret_key(&identity_dir)?;
            }
        }

        // Update the password
        if let Some(password) = new_identity.credentials.clone().unwrap().password.get() {
            if let Err(e) =
                Entry::new("atomic", &new_identity.name).and_then(|x| x.set_password(&password))
            {
                warn!("Unable to set password: {e:?}");
            }
        } else if let Err(e) =
            Entry::new("atomic", &new_identity.name).and_then(|x| x.delete_password())
        {
            warn!("Unable to delete password: {e:?}");
        }

        Ok(new_identity)
    }
}
