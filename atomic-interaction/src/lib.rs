//! Wrapper functions around `dialoguer` to support Atomic's different modes of interactivity.

mod input;
mod progress;

use input::{DefaultPrompt, PasswordPrompt, SelectionPrompt, TextPrompt};
use progress::{ProgressBarTrait, SpinnerTrait};
use std::sync::OnceLock;

// TODO: these should be replaced with a more sophisticated localization system
pub const DOWNLOAD_MESSAGE: &str = "Downloading changes";
pub const APPLY_MESSAGE: &str = "Applying changes";
pub const UPLOAD_MESSAGE: &str = "Uploading changes";
pub const COMPLETE_MESSAGE: &str = "Completing changes";
pub const OUTPUT_MESSAGE: &str = "Outputting repository";

/// Global state for setting interactivity. Should be set to `Option::None`
/// if no interactivity is possible, for example running Atomic with `--no-prompt`.
static INTERACTIVE_CONTEXT: OnceLock<InteractiveContext> = OnceLock::new();

/// Get the interactive context. If not set, returns an error.
pub fn get_context() -> Result<InteractiveContext, InteractionError> {
    if let Some(context) = INTERACTIVE_CONTEXT.get() {
        Ok(*context)
    } else {
        Err(InteractionError::NoContext)
    }
}

/// Set the interactive context, panicking if already set.
pub fn set_context(value: InteractiveContext) {
    // There probably isn't any reason for changing contexts at runtime
    INTERACTIVE_CONTEXT
        .set(value)
        .expect("Interactive context is already set!");
}

/// The different kinds of available prompts
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum PromptType {
    Confirm,
    Input,
    Select,
    Password,
}

impl core::fmt::Display for PromptType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match *self {
            Self::Confirm => "confirm",
            Self::Input => "input",
            Self::Select => "fuzzy selection",
            Self::Password => "password",
        };

        write!(f, "{name}")
    }
}

/// Errors that can occur while attempting to interact with the user
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum InteractionError {
    #[error("mode of interactivity not set")]
    NoContext,
    #[error("unable to provide interactivity in this context, and no valid default value for {0} prompt `{1}`")]
    NotInteractive(PromptType, String),
    #[error("I/O error while interacting with terminal")]
    IO(#[from] std::io::Error),
}

/// Different contexts for interacting with Atomic, for example terminal or web browser
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum InteractiveContext {
    Terminal,
    NotInteractive,
}

/// A prompt that asks the user to select yes or no
pub struct Confirm(Box<dyn DefaultPrompt<bool>>);

/// A prompt that asks the user to choose from a list of items.
pub struct Select(Box<dyn SelectionPrompt<usize>>);

/// A prompt that asks the user to enter text input
pub struct Input(Box<dyn TextPrompt<String>>);

/// A prompt that asks the user to enter a password
pub struct Password(Box<dyn PasswordPrompt<String>>);

/// A progress bar that is controlled by code
pub struct ProgressBar(Box<dyn ProgressBarTrait>);

/// An animated progress bar to indicate activity
pub struct Spinner(Box<dyn SpinnerTrait>);
