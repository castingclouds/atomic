//! Complete identity management.
//!
//! Atomic uses keys, rather than personal details such as names or emails to attribute changes.
//! The user can have multiple identities on disk, each with completely unique details.
//!
//! This module implements various functionality useful for managing identities on disk.
//! The current format for storing identities is as follows:
//! ```md
//! .config/atomic/ (or applicable global config directory)
//! ├── config.toml (global defaults)
//! │   ├── Username
//! │   ├── Full name
//! │   └── Email
//! └── identities/
//!     └── <IDENTITY NAME>/
//!         ├── identity.toml
//!         │   ├── Username
//!         │   ├── Full name
//!         │   ├── Email
//!         │   └── Public key
//!         │       ├── Version
//!         │       ├── Algorithm
//!         │       ├── Key
//!         │       └── Signature
//!         └── secret_key.json
//!             ├── Version
//!             ├── Algorithm
//!             └── Key
//! ```

#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]

mod create;
mod load;
mod repair;

pub use load::{choose_identity_name, public_key};
use log::warn;
pub use repair::fix_identities;

use atomic_config as config;
use atomic_config::Author;

use libatomic::key::{PublicKey, SKey, SecretKey};

use std::fmt::Display;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use atomic_interaction::Password;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(flatten)]
    pub author: Author,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            key_path: None,
            author: Author::default(),
        }
    }
}

impl From<Author> for Config {
    fn from(author: Author) -> Self {
        Self {
            key_path: None,
            author,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Credentials {
    secret_key: SecretKey,
    password: OnceLock<String>,
}

impl Credentials {
    pub fn new(secret_key: SecretKey, password: Option<String>) -> Self {
        Self {
            secret_key,
            password: if let Some(pw) = password {
                OnceLock::from(pw)
            } else {
                OnceLock::new()
            },
        }
    }
}

impl From<SecretKey> for Credentials {
    fn from(secret_key: SecretKey) -> Self {
        Self {
            secret_key,
            password: OnceLock::new(),
        }
    }
}

impl Credentials {
    pub fn decrypt(&mut self, name: &str) -> Result<(SKey, Option<String>), anyhow::Error> {
        if self.secret_key.encryption.is_none() {
            // Don't mind what the given password is, the secret key has no encryption
            // Make sure to revoke the password
            self.password.take();
            Ok((self.secret_key.load(None)?, None))
        } else if let Ok(key) = self
            .secret_key
            .load(self.password.get().map(String::as_str))
        {
            // The password matches secret key, no extra work needed
            Ok((key, self.password.get().map(|x| x.to_owned())))
        } else {
            // Password does not match secret key
            let mut stderr = std::io::stderr();
            let mut password_attempt = String::new();

            // Try a password stored in the keychain
            if let Ok(password) = keyring::Entry::new("atomic", name).and_then(|x| x.get_password())
            {
                password_attempt = password;
            }

            // Re-prompt as long as the password doesn't work
            while self.secret_key.load(Some(&password_attempt)).is_err() {
                writeln!(stderr, "Password does not match secret key")?;

                password_attempt = Password::new()?
                    .with_prompt("Password for secret key")
                    .with_allow_empty(true)
                    .interact()?;
            }

            // Update the password
            if let Err(e) =
                keyring::Entry::new("atomic", name).and_then(|x| x.set_password(&password_attempt))
            {
                warn!("Unable to set password: {e:?}");
            }
            self.password.set(password_attempt.clone()).unwrap();

            Ok((
                self.secret_key.load(Some(&password_attempt))?,
                Some(password_attempt),
            ))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// A complete user identity, representing the secret key, public key, and user info
pub struct Complete {
    #[serde(skip)]
    pub name: String,
    #[serde(flatten)]
    pub config: Config,
    pub last_modified: chrono::DateTime<chrono::Utc>,
    pub public_key: PublicKey,
    #[serde(skip)]
    pub credentials: Option<Credentials>,
}

impl Complete {
    /// Creates a new identity
    ///
    /// # Arguments
    /// * `name` - The name of the identity. This is encoded on-disk as identities/`<NAME>`
    /// * `config` - User configuration including author details & SSH key
    /// * `public_key` - The user's public key
    /// * `credentials` - The user's secret data including secret key & password
    pub fn new(
        name: String,
        config: Config,
        public_key: PublicKey,
        credentials: Option<Credentials>,
    ) -> Self {
        if name.is_empty() {
            panic!("Identity name cannot be empty!");
        }

        Self {
            name,
            config,
            public_key,
            credentials,
            last_modified: chrono::offset::Utc::now(),
        }
    }

    /// Creates the default identity, inferring details from the user's profile
    pub fn default() -> Result<Self, anyhow::Error> {
        let config_path = config::global_config_dir().unwrap().join("config.toml");
        let author: Author = if config_path.exists() {
            let mut config_file = fs::File::open(&config_path)?;
            let mut config_text = String::new();
            config_file.read_to_string(&mut config_text)?;

            let global_config: config::Global = toml::from_str(&config_text)?;
            global_config.author
        } else {
            Author::default()
        };

        let secret_key = SKey::generate(None);
        let public_key = secret_key.public_key();

        Ok(Self::new(
            String::from("default"),
            Config::from(author),
            public_key,
            Some(Credentials::from(secret_key.save(None))),
        ))
    }

    /// Returns the secret key, if one exists
    pub fn secret_key(&self) -> Option<SecretKey> {
        if let Some(credentials) = &self.credentials {
            Some(credentials.secret_key.clone())
        } else {
            None
        }
    }

    /// Strips the identity of any device-specific information, such as key path & identity name
    /// Returns the stripped identity
    pub fn as_portable(&self) -> Self {
        Self {
            name: String::new(),
            last_modified: chrono::offset::Utc::now(),
            config: Config {
                key_path: None,
                author: self.config.author.clone(),
            },
            public_key: self.public_key.clone(),
            credentials: None,
        }
    }

    /// Decrypts the user's secret key, prompting the user for password if necessary
    /// Returns a tuple containing the decrypted key & the valid password
    pub fn decrypt(&self) -> Result<(SKey, Option<String>), anyhow::Error> {
        self.credentials.clone().unwrap().decrypt(&self.name)
    }

    fn change_password(&mut self) -> Result<(), anyhow::Error> {
        let (decryped_key, _) = self.decrypt()?;

        let user_password = Password::new()?
            .with_prompt("New password")
            .with_allow_empty(true)
            .with_confirmation("Confirm password", "Password mismatch")
            .interact()?;

        let password = if user_password.is_empty() {
            OnceLock::new()
        } else {
            // User has entered a password, add it to the keyring
            if let Err(e) = keyring::Entry::new("atomic", &self.name)
                .and_then(|x| x.set_password(&user_password))
            {
                warn!("Unable to set password: {e:?}");
            }

            OnceLock::from(user_password)
        };

        // Update the key pair to match this new password
        self.public_key = decryped_key.public_key();
        self.credentials = Some(Credentials {
            secret_key: decryped_key.save(password.get().map(String::as_str)),
            password,
        });

        Ok(())
    }
}

// Implement Display so that the user can select identities from the fuzzy matcher
impl Display for Complete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Try and jog the user's memory by giving them a bit more context
        let has_username = !self.config.author.username.is_empty();
        let has_remote = !self.config.author.origin.is_empty();

        let remote_details: Option<String> = if has_username && has_remote {
            Some(format!(
                " [{}@{}]",
                self.config.author.username, self.config.author.origin
            ))
        } else if has_username {
            Some(format!(" [@{}]", self.config.author.username))
        } else if has_remote {
            Some(format!(" [:{}]", self.config.author.origin))
        } else {
            None
        };

        write!(f, "{}{}", self.name, remote_details.unwrap_or_default())
    }
}
