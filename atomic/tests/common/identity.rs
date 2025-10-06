use std::{
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Error;
use expectrl::{Session, WaitStatus};

use super::{Interaction, InteractionType};
use libatomic::key::SecretKey;

pub mod default {
    pub const ID_NAME: &str = "my_identity";
    pub const FULL_NAME: &str = "Firstname Lastname";
    pub const EMAIL: &str = "person@example.com";
    pub const EXPIRY: &str = "2056-01-01";
    pub const LOGIN: &str = "my_username";
    pub const ORIGIN: &str = "ssh.pijul.com";
    pub const PASSWORD: &str = "correct-horse-battery-staple";
    pub const SSH: &str = ""; // Just confirm the first item in SSH option list
}

pub mod prompt {
    pub const ID_NAME: &str = "Unique identity name";
    pub const DISPLAY_NAME: &str = "Display name";
    pub const EMAIL: &str = "Email (leave blank for none)";
    pub const EXPIRY_DATE: &str = "Expiry date (YYYY-MM-DD)";
    pub const LOGIN: &str = "Remote username";
    pub const ORIGIN: &str = "Remote URL";
    pub const PASSWORD: &str = "New password";
    pub const PASSWORD_REPROMPT: &str = "Confirm password";
    pub const SELECT_KEY: &str = "Select key";

    pub mod confirm {
        pub const SSH: &str = "Do you want to change the default SSH key?";
        pub const ENCRYPTION: &str = "Do you want to change the encryption?";
        pub const EXPIRY: &str = "Do you want this key to expire?";
        pub const REMOTE: &str = "Do you want to link this identity to a remote?";
    }
}

const CONFIG_DATA: &str = "colors = 'never'
[author]
login = ''";

#[derive(Clone)]
pub enum SubCommand {
    New,
    Edit(String),
    Remove,
}

#[derive(Clone)]
pub struct Identity {
    pub id_name: Interaction,
    pub display_name: Option<String>,
    pub email: Option<Interaction>,
    pub expiry: Option<Interaction>,
    pub login: Option<Interaction>,
    pub origin: Option<Interaction>,
    pub password: Option<Interaction>,
    pub key_path: Option<Interaction>,
    config_path: PathBuf,
}

impl Identity {
    pub fn new<P: AsRef<Path>>(
        path_name: P,
        id_name: Interaction,
        full_name: Option<String>,
        email: Option<Interaction>,
        expiry: Option<Interaction>,
        login: Option<Interaction>,
        remote: Option<Interaction>,
        password: Option<Interaction>,
        key_path: Option<Interaction>,
    ) -> Result<Self, Error> {
        let config_path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(path_name);

        let identity = Self {
            id_name,
            display_name: full_name,
            email,
            expiry,
            login,
            origin: remote,
            password,
            key_path,
            config_path,
        };
        identity.reset_fs(Vec::new().as_slice())?;

        Ok(identity)
    }

    pub fn reset_fs(&self, existing_identities: &[Identity]) -> Result<(), Error> {
        let mut config_path = self.config_path.clone();
        config_path.push("identities");
        if config_path.exists() {
            std::fs::remove_dir_all(&config_path)?;
        }

        std::fs::create_dir_all(&config_path)?;
        config_path.pop();

        config_path.push("config.toml");
        std::fs::write(&config_path, CONFIG_DATA)?;
        config_path.pop();

        // Create every identity that should exist
        for existing_id in existing_identities {
            assert_eq!(existing_id.config_path, config_path);

            println!(
                "Creating existing identity with name: {}",
                existing_id.id_name.valid_input().as_string()
            );
            existing_id.run_cli_edit(
                generate_command(&config_path, &SubCommand::New),
                true,
                &SubCommand::New,
            )?;
        }

        Ok(())
    }

    fn verify(&self) -> Result<(), Error> {
        let identity_path = self
            .config_path
            .join("identities")
            .join(&self.id_name.valid_input().as_string())
            .join("identity.toml");

        // Parse the generated TOML and verify
        let identity_data = std::fs::read_to_string(identity_path)?;
        let toml_data = identity_data.parse::<toml::Value>().unwrap();

        self.display_name.as_ref().map_or_else(
            || {
                if let Some(full_name) = toml_data.get("name") {
                    assert_eq!(full_name.as_str().unwrap(), whoami::realname());
                }
            },
            |full_name| {
                assert_eq!(
                    full_name.as_str(),
                    toml_data.get("display_name").unwrap().as_str().unwrap()
                );
            },
        );

        self.email.as_ref().map_or_else(
            || {
                assert!(toml_data.get("email").is_none());
            },
            |email| {
                assert_eq!(
                    email.valid_input().as_string().as_str(),
                    toml_data.get("email").unwrap().as_str().unwrap()
                );
            },
        );

        self.login.as_ref().map_or_else(
            || {
                let default = toml::value::Value::String(String::new());
                let data = toml_data.get("login").unwrap_or(&default).as_str().unwrap();
                assert!(data.is_empty() || data == whoami::username());
            },
            |login| {
                assert_eq!(
                    login.valid_input().as_string().as_str(),
                    toml_data.get("username").unwrap().as_str().unwrap()
                );
            },
        );

        self.origin.as_ref().map_or_else(
            || {
                let default = toml::value::Value::String(String::new());
                let data = toml_data
                    .get("origin")
                    .unwrap_or(&default)
                    .as_str()
                    .unwrap();
                assert!(data.is_empty() || data == "ssh.pijul.com");
            },
            |origin| {
                assert_eq!(
                    origin.valid_input().as_string().as_str(),
                    toml_data.get("origin").unwrap().as_str().unwrap()
                );
            },
        );

        if let Some(expiry) = &self.expiry {
            let time_stamp = toml_data
                .get("public_key")
                .unwrap()
                .get("expires")
                .unwrap()
                .as_str()
                .unwrap();
            let parsed_time_stamp =
                dateparser::parse_with_timezone(time_stamp, &chrono::offset::Utc)?;

            assert_eq!(
                expiry.valid_input().as_string(),
                parsed_time_stamp.format("%Y-%m-%d").to_string()
            );
        } else {
            assert!(toml_data
                .get("public_key")
                .unwrap()
                .get("expires")
                .is_none());
        }

        let mut secret_key_file = std::fs::File::open(
            self.config_path
                .join("identities")
                .join(self.id_name.valid_input().as_string())
                .join("secret_key.json"),
        )?;
        let mut secret_key_text = String::new();
        secret_key_file.read_to_string(&mut secret_key_text)?;
        let secret_key: SecretKey = serde_json::from_str(&secret_key_text)?;
        assert_eq!(secret_key.encryption.is_some(), self.password.is_some());

        self.password.as_ref().map_or_else(
            || {
                secret_key.load(None).unwrap();
            },
            |password| {
                secret_key
                    .load(Some(password.valid_input().as_string().as_str()))
                    .unwrap();
            },
        );

        Ok(())
    }

    pub fn run_cli_edit(
        &self,
        mut pijul_cmd: Command,
        valid: bool,
        subcmd: &SubCommand,
    ) -> Result<WaitStatus, Error> {
        pijul_cmd.arg("--no-prompt");

        match subcmd {
            SubCommand::New => {
                pijul_cmd.arg(self.id_name.get_input(valid));
            }
            SubCommand::Edit(old_name) => {
                pijul_cmd.arg(&old_name);

                let new_name = self.id_name.get_input(valid);
                if old_name != &new_name {
                    pijul_cmd.arg("--new-name").arg(new_name);
                }
            }
            SubCommand::Remove => {
                panic!("Wrong function call!");
            }
        };

        if let Some(full_name) = self.display_name.clone() {
            pijul_cmd.arg("--display-name").arg(full_name);
        }
        if let Some(email) = self.email.clone() {
            pijul_cmd.arg("--email").arg(email.get_input(valid));
        }
        if let Some(expiry) = self.expiry.clone() {
            pijul_cmd.arg("--expiry").arg(expiry.get_input(valid));
        }
        if let Some(login) = self.login.clone() {
            pijul_cmd.arg("--username").arg(login.get_input(valid));
        }
        if let Some(origin) = self.origin.clone() {
            pijul_cmd.arg("--remote").arg(origin.get_input(valid));
        }
        if self.password.is_some() {
            pijul_cmd.arg("--read-password");
        }

        println!(
            "Running pijul with args: {:#?}",
            pijul_cmd
                .get_args()
                .collect::<Vec<_>>()
                .join(OsStr::new(" "))
        );

        let mut session = Session::spawn(pijul_cmd)?;

        if valid {
            if let Some(password) = self.password.clone() {
                password.interact(&mut session)?;
            }
        }

        Ok(session.get_process().wait()?)
    }

    fn run_interactive_edit(&self, pijul_cmd: Command) -> Result<WaitStatus, Error> {
        // Interatction tree
        // ├── Identity name
        // ├── Display name
        // ├── Email
        // ├── Encryption
        // │   ├── Password
        // │   └── Confirm
        // ├── Expiry
        // │   └── Date
        // └── Link to remote
        //     ├── Username
        //     ├── Origin
        //     └── Default SSH key
        //         └── Key path
        let mut session = Session::spawn(pijul_cmd)?;

        // Interaction: ID name
        self.id_name.interact(&mut session)?;

        // Interaction: Display name
        if let Some(display_name) = self.display_name.clone() {
            Interaction::new(prompt::DISPLAY_NAME, InteractionType::Input(display_name))
                .interact(&mut session)?;
        } else {
            Interaction::new(prompt::DISPLAY_NAME, InteractionType::Input(String::new()))
                .interact(&mut session)?;
        }

        // Interaction: Email
        self.email
            .clone()
            .unwrap_or(Interaction::new(
                prompt::EMAIL,
                InteractionType::Input(String::new()),
            ))
            .interact(&mut session)?;

        // Interaction: Encryption
        Interaction::new(
            format!(
                "{} (Current status: not encrypted)",
                prompt::confirm::ENCRYPTION
            ),
            InteractionType::Confirm(self.password.is_some()),
        )
        .interact(&mut session)?;
        if let Some(password) = self.password.clone() {
            password.interact(&mut session)?;
        }

        // Interaction: Expiry
        Interaction::new(
            format!("{} (Current expiry: never)", prompt::confirm::EXPIRY),
            InteractionType::Confirm(self.expiry.is_some()),
        )
        .interact(&mut session)?;
        if let Some(expiry) = self.expiry.clone() {
            expiry.interact(&mut session)?;
        }

        // Interaction: Link remote
        let remote_data = self.login.is_some() || self.origin.is_some() || self.key_path.is_some();
        Interaction::new(
            prompt::confirm::REMOTE,
            InteractionType::Confirm(remote_data),
        )
        .interact(&mut session)?;
        if remote_data {
            if let Some(login) = self.login.clone() {
                login.interact(&mut session)?;
            } else {
                // Use an empty login
                Interaction::new(prompt::LOGIN, InteractionType::Input(String::new()))
                    .interact(&mut session)?;
            }
            if let Some(origin) = self.origin.clone() {
                origin.interact(&mut session)?;
            } else {
                // Use an empty origin
                Interaction::new(prompt::ORIGIN, InteractionType::Input(String::new()))
                    .interact(&mut session)?;
            }

            Interaction::new(
                prompt::confirm::SSH,
                InteractionType::Confirm(self.key_path.is_some()),
            )
            .interact(&mut session)?;
            if let Some(key_path) = self.key_path.clone() {
                key_path.interact(&mut session)?;
            }
        }

        Ok(session.get_process().wait()?)
    }

    pub fn run_edit(
        &self,
        subcmd: &SubCommand,
        existing_identities: Vec<Self>,
    ) -> Result<(), Error> {
        let invalid_interactions = [
            self.id_name.invalid_input(),
            self.email.as_ref().and_then(Interaction::invalid_input),
            self.expiry.as_ref().and_then(Interaction::invalid_input),
        ];

        // If any of the items have invalid values, we need to test the program correctly errors out
        if invalid_interactions.iter().any(Option::is_some) {
            println!("Detected invalid inputs, expecting failure with --no-prompt");
            self.reset_fs(&existing_identities)?;
            let cli_status =
                self.run_cli_edit(generate_command(&self.config_path, subcmd), false, subcmd)?;
            assert!(!matches!(cli_status, WaitStatus::Exited(_, exitcode::OK)));
            println!("Program failed as expected");
        }

        self.reset_fs(&existing_identities)?;
        let cli_status =
            self.run_cli_edit(generate_command(&self.config_path, subcmd), true, subcmd)?;
        assert!(matches!(cli_status, WaitStatus::Exited(_, exitcode::OK)));
        self.verify()?;
        println!("Successfully ran pijul in CLI mode");

        self.reset_fs(&existing_identities)?;
        let interactive_status =
            self.run_interactive_edit(generate_command(&self.config_path, subcmd))?;
        assert!(matches!(
            interactive_status,
            WaitStatus::Exited(_, exitcode::OK)
        ));
        self.verify()?;
        println!("Successfully ran pijul in interactive mode");

        Ok(())
    }

    pub fn run(&self, subcmd: &SubCommand, existing_identities: Vec<Self>) -> Result<(), Error> {
        match subcmd {
            SubCommand::New | SubCommand::Edit(_) => {
                self.run_edit(subcmd, existing_identities)?;
            }
            SubCommand::Remove => {
                self.reset_fs(&existing_identities)?;

                let pijul_cmd = generate_command(&self.config_path, subcmd);
                println!(
                    "Running pijul with args: {:#?}",
                    pijul_cmd
                        .get_args()
                        .collect::<Vec<_>>()
                        .join(OsStr::new(" "))
                );
                let mut session = Session::spawn(pijul_cmd)?;

                Interaction::new("Do you wish to continue?", InteractionType::Confirm(true))
                    .interact(&mut session)?;

                let status = session.get_process().wait()?;
                assert!(matches!(status, WaitStatus::Exited(_, exitcode::OK)));
                assert!(!self
                    .config_path
                    .join("identities")
                    .join(&self.id_name.valid_input().as_string())
                    .exists());
            }
        }

        Ok(())
    }
}

fn subcommand_name(subcmd: &SubCommand) -> String {
    match subcmd {
        SubCommand::New => String::from("new"),
        SubCommand::Edit(_) => String::from("edit"),
        SubCommand::Remove => String::from("remove"),
    }
}

fn generate_command(config_path: &PathBuf, subcmd: &SubCommand) -> Command {
    let mut pijul_cmd = Command::new(std::env::var("CARGO_BIN_EXE_pijul").unwrap_or_else(|_| "pijul".to_string()));
    pijul_cmd.env("PIJUL_CONFIG_DIR", config_path);
    pijul_cmd.arg("identity");

    let subcommand = subcommand_name(&subcmd);
    pijul_cmd.arg(&subcommand);

    if subcommand == "edit" || subcommand == "new" {
        pijul_cmd.arg("--no-link");
    }

    pijul_cmd
}
