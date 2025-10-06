#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]

pub mod identity;

use std::io::{Read, Write};

use anyhow::{bail, Error};
use expectrl::{
    process::{unix::UnixProcess, NonBlocking},
    ControlCode, Regex, Session,
};

#[derive(Clone, Debug)]
pub enum InteractionType {
    Confirm(bool),
    Input(String),
    Password {
        input: String,
        confirm: Option<String>,
    },
}

impl InteractionType {
    pub fn as_string(&self) -> String {
        match self {
            Self::Confirm(confirm) => {
                if *confirm {
                    String::from('y')
                } else {
                    String::from('n')
                }
            }
            Self::Input(input) | Self::Password { input, .. } => input.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SecondAttempt {
    input: InteractionType,
    error_message: String,
}

impl SecondAttempt {
    pub fn new<S: Into<String>>(input: InteractionType, error_msg: S) -> Result<Self, Error> {
        let error_message: String = error_msg.into();
        if matches!(input, InteractionType::Confirm(_)) && !error_message.is_empty() {
            bail!("Cannot have error message for confirm propmt");
        }

        Ok(Self {
            input,
            error_message,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Interaction {
    prompt_message: String,
    input: InteractionType,
    second_attempt: Option<SecondAttempt>,
}

impl Interaction {
    pub fn new<S: Into<String>>(prompt_message: S, input: InteractionType) -> Self {
        Self {
            prompt_message: prompt_message.into(),
            input,
            second_attempt: None,
        }
    }

    pub fn with_second_attempt(mut self, second_attempt: SecondAttempt) -> Result<Self, Error> {
        if let Some(second_attempt) = self.second_attempt.clone() {
            let _interaction_type = second_attempt.input;
            if !matches!(&self.input, _interaction_type) {
                bail!("Cannot have non-matching second input!");
            }
        }

        self.second_attempt = Some(second_attempt);
        Ok(self)
    }

    pub fn get_input(&self, valid: bool) -> String {
        if let Some(invalid) = self.invalid_input() {
            if !valid {
                return invalid.as_string();
            }
        }

        self.valid_input().as_string()
    }

    pub fn invalid_input(&self) -> Option<InteractionType> {
        if self.second_attempt.is_some() {
            Some(self.input.clone())
        } else {
            None
        }
    }

    pub fn valid_input(&self) -> InteractionType {
        if let Some(second_input) = &self.second_attempt {
            second_input.input.clone()
        } else {
            self.input.clone()
        }
    }

    pub fn interact<S: NonBlocking + Write + Read>(
        &self,
        session: &mut Session<UnixProcess, S>,
    ) -> Result<(), Error> {
        // Wait for the text to come in
        println!("Expecting prompt message: {}", self.prompt_message);
        session.expect(&self.prompt_message)?;

        match &self.input {
            InteractionType::Confirm(confirm) => {
                println!("Sending confirmation: {confirm}");
                session.send(&self.input.as_string())?;
            }
            InteractionType::Input(_) => {
                if let Some(invalid_input) = self.invalid_input() {
                    clear_prompt(session)?;

                    println!("Sending invalid input: {}", invalid_input.as_string());
                    session.send(invalid_input.as_string())?;
                    session.send(ControlCode::CarriageReturn)?;

                    let error_message = self.second_attempt.clone().unwrap().error_message;
                    println!("Expecting error message: {error_message}");
                    session.expect(error_message)?;
                }

                clear_prompt(session)?;
                let valid_input = self.valid_input().as_string();
                println!("Sending valid input: {}", valid_input);
                session.send(valid_input)?;
                session.send(ControlCode::CarriageReturn)?;
            }
            InteractionType::Password { confirm, .. } => {
                let valid_password = self.valid_input().as_string();
                println!("Sending valid password: {valid_password}");
                session.send(&valid_password)?;
                session.send(ControlCode::CarriageReturn)?;

                // If there is a second attempt, send the invalid password
                if let Some(second_attempt) = self.invalid_input() {
                    let confirm_prompt = confirm.as_ref().unwrap();
                    println!("Expecting password re-prompt: {confirm_prompt}");
                    session.expect(confirm_prompt)?;

                    let invalid_password = second_attempt.as_string();
                    println!("Sending invalid password: {invalid_password}");

                    session.send(&invalid_password)?;
                    session.send(ControlCode::CarriageReturn)?;

                    let error_message = self.second_attempt.clone().unwrap().error_message;
                    println!("Expecting error message: {error_message}");
                    session.expect(&error_message)?;
                }

                // Sometimes the password needs to be confirmed
                if let Some(confirm_prompt) = confirm {
                    // In the case of invalid input, we have to send twice
                    if self.invalid_input().is_some() {
                        println!("Expecting prompt message: {}", self.prompt_message);
                        session.expect(&self.prompt_message)?;

                        println!("Sending valid password: {valid_password}");
                        session.send(&valid_password)?;
                        session.send(ControlCode::CarriageReturn)?;
                    }

                    println!("Expecting password re-prompt: {confirm_prompt}");
                    session.expect(confirm_prompt)?;

                    println!("Re-sending valid password: {valid_password}");
                    session.send(&valid_password)?;
                    session.send(ControlCode::CarriageReturn)?;
                }
            }
        }

        Ok(())
    }
}

fn clear_prompt<S: NonBlocking + Write + Read>(
    session: &mut Session<UnixProcess, S>,
) -> Result<(), Error> {
    println!("Clearing prompt");

    // Use regex to find where the prompt ends
    let prompt_regex = r":.*";
    let captures = session.expect(Regex(prompt_regex))?;
    let matches = captures.matches();

    // Clear default text by sending backspaces
    for _ in 0..matches.last().unwrap().len() {
        session.send(ControlCode::Backspace)?;
    }

    Ok(())
}
