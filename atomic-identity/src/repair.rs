use super::Complete;
use atomic_config as config;

use libatomic::key::{PublicKey, SecretKey};

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{bail, Context};
use log::debug;
use thiserror::Error;

const FIRST_IDENTITY_MESSAGE: &str = "It doesn't look like you have any identities configured!
Each author in atomic is identified by a unique key to provide greater security & flexibility over names/emails.
To make sure humans (including you!) can easily identify these keys, we need a few personal details.
For more information see https://atomic.org/manual/keys.html";

const MIGRATE_IDENTITY_MESSAGE: &str =
    "It seems you have configured an identity in an older version of atomic, which uses an older identity format!
Please take a moment to confirm your details are correct.";

const MISMATCHED_KEYS_MESSAGE: &str = "It seems the keys on your system are mismatched!
This is most likely the result of data corruption, please check your drive and try again.";

#[derive(Error, Debug)]
pub enum IdentityParseError {
    #[error("Mismatching keys")]
    MismatchingKeys,
    #[error("Could not find secret key at path {0}")]
    NoSecretKey(PathBuf),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Ensure that the user has at least one valid identity on disk.
///
/// This function performs the following:
/// * Migrate users from the old identity format
/// * Validate all identity key pairs
/// * Create a new identity if none exist
pub async fn fix_identities() -> Result<(), anyhow::Error> {
    let mut dir = config::global_config_dir().unwrap();
    dir.push("identities");
    std::fs::create_dir_all(&dir)?;
    dir.pop();

    let identities = Complete::load_all()?;

    if identities.is_empty() {
        // This could be because the old format exists on disk, but if the
        // extraction fails then we can be fairly sure the user simply isn't set up
        let extraction_result = Complete::from_old_format();

        let mut stderr = std::io::stderr();

        match extraction_result {
            Ok(old_identity) => {
                // Migrate to new format
                writeln!(stderr, "{MIGRATE_IDENTITY_MESSAGE}")?;

                // Confirm details then write to disk
                old_identity.clone().create(true).await?;

                // The identity is stored as the public key's signature on disk
                let identity_path = format!("identities/{}", &old_identity.public_key.key);

                // Try to delete what remains of the old identities
                let paths_to_delete =
                    vec!["publickey.json", "secretkey.json", identity_path.as_str()];
                for path in paths_to_delete {
                    let file_path = dir.join(path);
                    if file_path.exists() {
                        debug!("Deleting old file: {file_path:?}");
                        fs::remove_file(file_path)?;
                    } else {
                        debug!("Could not delete old file (path not found): {file_path:?}");
                    }
                }
            }
            Err(e) => {
                match e {
                    IdentityParseError::MismatchingKeys => {
                        bail!("User must repair broken keys before continuing");
                    }
                    IdentityParseError::NoSecretKey(_) => {
                        // This is the user's first time setting up an identity
                        writeln!(stderr, "{FIRST_IDENTITY_MESSAGE}")?;
                        Complete::default()?.create(true).await?;
                    }
                    IdentityParseError::Other(err) => {
                        bail!(err);
                    }
                }
            }
        }
    }

    // Sanity check to make sure everything is in order
    for identity in Complete::load_all()? {
        identity.valid_keys()?;
    }

    Ok(())
}

impl Complete {
    /// Checks if the key pair on disk is valid
    fn valid_keys(&self) -> Result<bool, anyhow::Error> {
        let public_key = &self.public_key;
        let decryped_public_key = self.decrypt()?.0.public_key();

        if public_key.key != decryped_public_key.key {
            let mut stderr = std::io::stderr();
            writeln!(stderr, "{MISMATCHED_KEYS_MESSAGE}")?;
            writeln!(stderr, "Got the following public key signatures:")?;
            writeln!(stderr, "Plaintext public key: {public_key:#?}")?;
            writeln!(stderr, "Decrypted public key: {decryped_public_key:#?}")?;

            return Ok(false);
        }

        Ok(true)
    }

    /// Migrate user from old to new identity format.
    ///
    /// # Arguments
    /// * `password` - The password used to decrypt the secret key
    ///
    /// # Data format
    /// Data stored in the old format should look as follows:
    /// ```md
    ///    .config/atomic/ (or applicable global config directory)
    ///    ├── config.toml
    ///    │   ├── Username
    ///    │   ├── Full name
    ///    │   └── Email
    ///    ├── secretkey.json
    ///    │   ├── Version
    ///    │   ├── Algorithm
    ///    │   └── Key
    ///    ├── publickey.json
    ///    │   ├── Version
    ///    │   ├── Algorithm
    ///    │   ├── Signature
    ///    │   └── Key
    ///    └── identities/
    ///        └── <PUBLIC KEY> (JSON, no extension)
    ///            ├── Public key
    ///            │   ├── Version
    ///            │   ├── Algorithm
    ///            │   ├── Signature
    ///            │   └── Key
    ///            ├── Login
    ///            └── Last modified
    ///```
    ///
    /// As you can see, there is a lot of redundant data. We can leverage this
    /// information to repair partially corrupted state. For example, we can
    /// reconstruct `publickey.json` using the identity file. We are also able
    /// to reconstruct the public key from the private key, so the steps should
    /// look roughly as follows:
    /// 1. Extract secret key
    /// 2. Extract public key from (in order):
    ///     1. publickey.json
    ///     2. File in identities/
    ///     3. secretkey.json
    /// 3. Extract login info from (in order):
    ///     1. File in identities/
    ///     2. config.toml
    /// 4. Validate extracted data (query user to fill in blanks)
    fn from_old_format() -> Result<Self, IdentityParseError> {
        let config_dir = config::global_config_dir().unwrap();

        let config_path = config_dir.join("config.toml");
        let identities_path = config_dir.join("identities");
        let public_key_path = config_dir.join("publickey.json");
        let secret_key_path = config_dir.join("secretkey.json");

        // If we don't have the private key, there is no chance of repairing
        // the data. This will also trigger if the data is not in the old format
        if !secret_key_path.exists() {
            return Err(IdentityParseError::NoSecretKey(secret_key_path));
        }
        // From this point, we can be in 2 states:
        // - Old identity format
        // - Broken/missing data

        // Extract data from secretkey.json
        let mut secret_key_file =
            fs::File::open(&secret_key_path).context("Failed to open secret key file")?;
        let mut secret_key_text = String::new();
        secret_key_file
            .read_to_string(&mut secret_key_text)
            .context("Failed to read secret key file")?;
        let secret_key: SecretKey =
            serde_json::from_str(&secret_key_text).context("Failed to parse secret key file")?;

        // Extract data from publickey.json
        // TODO: handle public key not existing
        let public_key: PublicKey = if public_key_path.exists() {
            let mut public_key_file =
                fs::File::open(&public_key_path).context("Failed to open public key file")?;
            let mut public_key_text = String::new();
            public_key_file
                .read_to_string(&mut public_key_text)
                .context("Failed to read public key file")?;

            serde_json::from_str(&public_key_text).context("Failed to parse public key file")?
        } else {
            return Err(IdentityParseError::Other(anyhow::anyhow!(
                "Public key does not exist!"
            )));
        };

        // Extract valid identities
        let identity: Option<Complete> = if identities_path.exists() {
            if identities_path.is_dir() {
                let identities_iter =
                    fs::read_dir(identities_path).context("Failed to read identities directory")?;
                let mut identities: Vec<Complete> = vec![];

                // We only need to keep the valid files
                for dir_entry in identities_iter {
                    let path = dir_entry.unwrap().path();

                    if path.is_file() {
                        // Try and deserialize the data. If it fails, there is
                        // a fairly high chance it's not what we need
                        let mut identity_file =
                            fs::File::open(&path).context("Failed to open identity file")?;
                        let mut identity_text = String::new();
                        identity_file
                            .read_to_string(&mut identity_text)
                            .context("Failed to read identity file")?;
                        let deserialization_result: Result<Complete, _> =
                            serde_json::from_str(&identity_text);

                        if deserialization_result.is_ok() {
                            identities.push(
                                deserialization_result
                                    .context("Failed to deserialize identity file")?,
                            );
                        }
                    }
                }

                if identities.len() == 1 {
                    Some(identities[0].clone())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let config: super::Config = if config_path.exists() {
            let mut config_file =
                fs::File::open(&config_path).context("Failed to open config file")?;
            let mut config_text = String::new();
            config_file
                .read_to_string(&mut config_text)
                .context("Failed to read config file")?;

            let config_data: config::Global =
                toml::from_str(&config_text).context("Failed to parse config file")?;

            super::Config {
                key_path: config_data.author.key_path.clone(),
                author: config_data.author,
            }
        } else {
            let mut author = config::Author::default();
            author.username = identity
                .as_ref()
                .map_or_else(String::new, |x| x.config.author.username.clone());

            super::Config {
                key_path: None,
                author,
            }
        };

        let identity = Self::new(
            String::from("default"),
            config,
            public_key,
            Some(super::Credentials::from(secret_key)),
        );

        if identity.valid_keys()? {
            Ok(identity)
        } else {
            Err(IdentityParseError::MismatchingKeys)
        }
    }
}
