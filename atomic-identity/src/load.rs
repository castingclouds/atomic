use super::fix_identities;
use super::Complete;
use atomic_config as config;

use libatomic::key::{PublicKey, SecretKey};

use std::fs;
use std::path::PathBuf;

use anyhow::bail;
use atomic_interaction::Select;
use std::sync::OnceLock;

static CHOSEN_IDENTITY: OnceLock<String> = OnceLock::new();

/// Returns the directory in which identity information should be stored.
///
/// # Arguments
/// * `name` - The name of the identity. This is encoded on-disk as identities/`<NAME>`
/// * `should_exist` - If the path should already exist.
///
/// # Errors
/// * An identity of `name` does not exist
/// * The identity name is empty
pub fn path(name: &str, should_exist: bool) -> Result<PathBuf, anyhow::Error> {
    let mut path = config::global_config_dir()
        .expect("Could not find global config directory")
        .join("identities");

    if name.is_empty() {
        bail!("Cannot get path of un-named identity");
    }

    path.push(name);
    if !path.exists() && should_exist {
        bail!("Cannot get identity path: name does not exist")
    }

    Ok(path)
}

/// Returns the public key for identity named <NAME>.
///
/// # Arguments
/// * `name` - The name of the identity. This is encoded on-disk as identities/`<NAME>`
pub fn public_key(name: &str) -> Result<PublicKey, anyhow::Error> {
    let text = fs::read_to_string(path(name, true)?.join("identity.toml"))?;
    let identity: Complete = toml::from_str(&text)?;

    Ok(identity.public_key)
}

/// Returns the secret key for identity named <NAME>.
///
/// # Arguments
/// * `name` - The name of the identity. This is encoded on-disk as identities/`<NAME>`
pub fn secret_key(name: &str) -> Result<SecretKey, anyhow::Error> {
    let identity_text = fs::read_to_string(path(name, true)?.join("secret_key.json"))?;
    let secret_key: SecretKey = serde_json::from_str(&identity_text)?;

    Ok(secret_key)
}

/// Choose an identity, either through defaults or a user prompt.
///
/// # Errors
/// * User input is required to continue, but `no_prompt` is set to true
pub async fn choose_identity_name() -> Result<String, anyhow::Error> {
    if let Some(name) = CHOSEN_IDENTITY.get() {
        return Ok(name.clone());
    }

    let mut possible_identities = Complete::load_all()?;
    if possible_identities.is_empty() {
        fix_identities().await?;
        possible_identities = Complete::load_all()?;
    }

    let chosen_name = if possible_identities.len() == 1 {
        possible_identities[0].clone().name
    } else {
        let index = Select::new()?
            .with_prompt("Select identity")
            .with_items(&possible_identities)
            .with_default(0 as usize)
            .interact()?;

        possible_identities[index].clone().name
    };

    // The user has selected once, don't want to query them again
    CHOSEN_IDENTITY
        .set(chosen_name.clone())
        .expect("Could not set chosen identity");

    Ok(chosen_name)
}

impl Complete {
    /// Loads a complete identity associated with the given identity name.
    ///
    /// # Arguments
    /// * `identity_name` - The name of the identity. This is encoded on-disk as identities/`<NAME>`
    pub fn load(identity_name: &str) -> Result<Self, anyhow::Error> {
        let identity_path = path(identity_name, true)?;

        let text = fs::read_to_string(identity_path.join("identity.toml"))?;
        let identity: Complete = toml::from_str(&text)?;

        let secret_key = secret_key(identity_name)?;

        Ok(Self::new(
            identity_name.to_string(),
            identity.config,
            identity.public_key,
            Some(super::Credentials::from(secret_key)),
        ))
    }

    /// Loads all valid identities found on disk
    pub fn load_all() -> Result<Vec<Self>, anyhow::Error> {
        let config_dir = config::global_config_dir().unwrap();
        let identities_path = config_dir.join("identities");
        std::fs::create_dir_all(&identities_path)?;

        let identities_dir = identities_path.as_path().read_dir()?;
        let mut identities = vec![];

        for dir_entry in identities_dir {
            let file_name = dir_entry?.file_name();
            let identity_name = file_name.to_str().unwrap();

            if let Ok(identity) = Self::load(identity_name) {
                identities.push(identity);
            }
        }

        Ok(identities)
    }
}
