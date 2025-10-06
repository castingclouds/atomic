#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]

mod common;

use anyhow::Error;
use common::identity::{default, prompt, Identity, SubCommand};
use common::{Interaction, InteractionType, SecondAttempt};

fn default_id_name() -> Interaction {
    Interaction::new(
        prompt::ID_NAME,
        InteractionType::Input(default::ID_NAME.to_string()),
    )
}

#[test]
fn new_minimal() -> Result<(), Error> {
    let identity = Identity::new(
        "new_minimal",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    identity.run(&SubCommand::New, Vec::new())?;
    Ok(())
}

#[test]
fn new_full() -> Result<(), Error> {
    let identity = Identity::new(
        "new_full",
        default_id_name(),
        Some(default::FULL_NAME.to_string()),
        Some(Interaction::new(
            prompt::EMAIL,
            InteractionType::Input(default::EMAIL.to_string()),
        )),
        Some(Interaction::new(
            prompt::EXPIRY_DATE,
            InteractionType::Input(default::EXPIRY.to_string()),
        )),
        Some(Interaction::new(
            prompt::LOGIN,
            InteractionType::Input(default::LOGIN.to_string()),
        )),
        Some(Interaction::new(
            prompt::ORIGIN,
            InteractionType::Input(default::ORIGIN.to_string()),
        )),
        Some(Interaction::new(
            prompt::PASSWORD,
            InteractionType::Password {
                input: default::PASSWORD.to_string(),
                confirm: Some(prompt::PASSWORD_REPROMPT.to_string()),
            },
        )),
        Some(Interaction::new(
            prompt::SELECT_KEY,
            InteractionType::Input(default::SSH.to_string()),
        )),
    )?;

    identity.run(&SubCommand::New, Vec::new())?;
    Ok(())
}

#[test]
fn new_email() -> Result<(), Error> {
    let identity = Identity::new(
        "new_email",
        default_id_name(),
        None,
        Some(
            Interaction::new(
                prompt::EMAIL,
                InteractionType::Input(String::from("BAD-EMAIL")),
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Input(default::EMAIL.to_string()),
                "Invalid email address",
            )?)?,
        ),
        None,
        None,
        None,
        None,
        None,
    )?;

    identity.run(&SubCommand::New, Vec::new())?;
    Ok(())
}

#[test]
fn new_expiry() -> Result<(), Error> {
    let identity = Identity::new(
        "new_expiry",
        default_id_name(),
        None,
        None,
        Some(
            Interaction::new(
                prompt::EXPIRY_DATE,
                InteractionType::Input(String::from("BAD-EXPIRY")),
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Input(default::EXPIRY.to_string()),
                "Invalid date",
            )?)?,
        ),
        None,
        None,
        None,
        None,
    )?;

    identity.run(&SubCommand::New, Vec::new())?;
    Ok(())
}

#[test]
fn new_login() -> Result<(), Error> {
    let identity = Identity::new(
        "new_login",
        default_id_name(),
        None,
        None,
        None,
        Some(Interaction::new(
            prompt::LOGIN,
            InteractionType::Input(default::LOGIN.to_string()),
        )),
        None,
        None,
        None,
    )?;

    identity.run(&SubCommand::New, Vec::new())?;
    Ok(())
}

#[test]
fn new_origin() -> Result<(), Error> {
    let identity = Identity::new(
        "new_origin",
        default_id_name(),
        None,
        None,
        None,
        None,
        Some(Interaction::new(
            prompt::ORIGIN,
            InteractionType::Input(default::ORIGIN.to_string()),
        )),
        None,
        None,
    )?;

    identity.run(&SubCommand::New, Vec::new())?;
    Ok(())
}

#[test]
fn new_password() -> Result<(), Error> {
    let identity = Identity::new(
        "new_password",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        Some(
            Interaction::new(
                prompt::PASSWORD,
                InteractionType::Password {
                    input: default::PASSWORD.to_string(),
                    confirm: Some(prompt::PASSWORD_REPROMPT.to_string()),
                },
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Password {
                    input: "Good-Password".to_string(),
                    confirm: Some(prompt::PASSWORD_REPROMPT.to_string()),
                },
                "Password mismatch",
            )?)?,
        ),
        None,
    )?;

    identity.run(&SubCommand::New, Vec::new())?;
    Ok(())
}

#[test]
fn edit_full() -> Result<(), Error> {
    let old_identity = Identity::new(
        "edit_full",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let new_identity = Identity::new(
        "edit_full",
        Interaction::new(
            prompt::ID_NAME,
            InteractionType::Input(String::from("new_id_name")),
        ),
        Some(default::FULL_NAME.to_string()),
        Some(
            Interaction::new(
                prompt::EMAIL,
                InteractionType::Input(String::from("BAD_EMAIL")),
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Input(default::EMAIL.to_string()),
                "Invalid email address",
            )?)?,
        ),
        Some(
            Interaction::new(
                prompt::EXPIRY_DATE,
                InteractionType::Input(String::from("BAD-EXPIRY")),
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Input(default::EXPIRY.to_string()),
                "Invalid date",
            )?)?,
        ),
        Some(Interaction::new(
            prompt::LOGIN,
            InteractionType::Input(default::LOGIN.to_string()),
        )),
        Some(Interaction::new(
            prompt::ORIGIN,
            InteractionType::Input(default::ORIGIN.to_string()),
        )),
        Some(
            Interaction::new(
                prompt::PASSWORD,
                InteractionType::Password {
                    input: default::PASSWORD.to_string(),
                    confirm: Some(prompt::PASSWORD_REPROMPT.to_string()),
                },
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Password {
                    input: "Good-Password".to_string(),
                    confirm: Some(prompt::PASSWORD_REPROMPT.to_string()),
                },
                "Password mismatch",
            )?)?,
        ),
        Some(Interaction::new(
            prompt::SELECT_KEY,
            InteractionType::Input(default::SSH.to_string()),
        )),
    )?;

    new_identity.run(
        &SubCommand::Edit(old_identity.id_name.valid_input().as_string()),
        vec![old_identity],
    )?;

    Ok(())
}

#[test]
fn edit_id_name() -> Result<(), Error> {
    let old_identity = Identity::new(
        "edit_id_name",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let new_identity = Identity::new(
        "edit_id_name",
        Interaction::new(
            prompt::ID_NAME,
            InteractionType::Input(String::from("new_id_name")),
        ),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    new_identity.run(
        &SubCommand::Edit(old_identity.id_name.valid_input().as_string()),
        vec![old_identity],
    )?;

    Ok(())
}

#[test]
fn edit_email() -> Result<(), Error> {
    let old_identity = Identity::new(
        "edit_email",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let new_identity = Identity::new(
        "edit_email",
        default_id_name(),
        None,
        Some(
            Interaction::new(
                prompt::EMAIL,
                InteractionType::Input(String::from("BAD_EMAIL")),
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Input(default::EMAIL.to_string()),
                "Invalid email address",
            )?)?,
        ),
        None,
        None,
        None,
        None,
        None,
    )?;

    new_identity.run(
        &SubCommand::Edit(old_identity.id_name.valid_input().as_string()),
        vec![old_identity],
    )?;

    Ok(())
}

#[test]
fn edit_expiry() -> Result<(), Error> {
    let old_identity = Identity::new(
        "edit_expiry",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let new_identity = Identity::new(
        "edit_expiry",
        default_id_name(),
        None,
        None,
        Some(
            Interaction::new(
                prompt::EXPIRY_DATE,
                InteractionType::Input(String::from("BAD-EXPIRY")),
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Input(default::EXPIRY.to_string()),
                "Invalid date",
            )?)?,
        ),
        None,
        None,
        None,
        None,
    )?;

    new_identity.run(
        &SubCommand::Edit(old_identity.id_name.valid_input().as_string()),
        vec![old_identity],
    )?;

    Ok(())
}

#[test]
fn edit_password() -> Result<(), Error> {
    let old_identity = Identity::new(
        "edit_password",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let new_identity = Identity::new(
        "edit_password",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        Some(
            Interaction::new(
                prompt::PASSWORD,
                InteractionType::Password {
                    input: default::PASSWORD.to_string(),
                    confirm: Some(prompt::PASSWORD_REPROMPT.to_string()),
                },
            )
            .with_second_attempt(SecondAttempt::new(
                InteractionType::Password {
                    input: "Good-Password".to_string(),
                    confirm: Some(prompt::PASSWORD_REPROMPT.to_string()),
                },
                "Password mismatch",
            )?)?,
        ),
        None,
    )?;

    new_identity.run(
        &SubCommand::Edit(old_identity.id_name.valid_input().as_string()),
        vec![old_identity],
    )?;

    Ok(())
}

#[test]
fn remove() -> Result<(), Error> {
    let identity = Identity::new(
        "remove",
        default_id_name(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    identity.run(&SubCommand::Remove, vec![identity.clone()])?;
    Ok(())
}
