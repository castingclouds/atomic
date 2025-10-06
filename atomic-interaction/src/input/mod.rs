//! Implement the various prompt types defined in `lib.rs`
mod non_interactive;
mod terminal;

use crate::{Confirm, Input, Password, Select};
use crate::{InteractionError, InteractiveContext, PromptType};
use dialoguer::theme;
use duplicate::duplicate_item;
use lazy_static::lazy_static;
use non_interactive::PseudoInteractive;

lazy_static! {
    static ref THEME: Box<dyn theme::Theme + Send + Sync> = {
        use dialoguer::theme;
        use atomic_config::{self as config, Choice};

        if let Ok((config, _)) = config::Global::load() {
            let color_choice = config.colors.unwrap_or_default();

            match color_choice {
                Choice::Auto | Choice::Always => Box::<theme::ColorfulTheme>::default(),
                Choice::Never => Box::new(theme::SimpleTheme),
            }
        } else {
            Box::<theme::ColorfulTheme>::default()
        }
    };
}

/// A common interface shared by every prompt type.
/// May be useful if you wish to abstract over different kinds of prompt.
pub trait BasePrompt<T> {
    fn set_prompt(&mut self, prompt: String);
    fn interact(&mut self) -> Result<T, InteractionError>;
}

/// A trait for prompts that allow a default selection.
pub trait DefaultPrompt<T>: BasePrompt<T> {
    fn set_default(&mut self, value: T);
}

/// A trait for prompts that may need validation of user input.
///
/// This is mostly useful in contexts such as plain-text input or passwords,
/// rather than on controlled input such as confirmation prompts.
pub trait ValidationPrompt<T>: BasePrompt<T> {
    fn allow_empty(&mut self, empty: bool);
    fn set_validator(&mut self, validator: Box<dyn Fn(&T) -> Result<(), String>>);
}

/// A trait for prompts that accept a password.
pub trait PasswordPrompt<T>: ValidationPrompt<T> {
    fn set_confirmation(&mut self, confirm_prompt: String, mismatch_err: String);
}

/// A trait for prompts that accept text with a default value.
/// Notably, this does NOT include passwords.
pub trait TextPrompt<T>: ValidationPrompt<T> + DefaultPrompt<T> {
    fn set_inital_text(&mut self, text: String);
}

/// A trait for prompts where the user may choose from a selection of items.
pub trait SelectionPrompt<T>: DefaultPrompt<T> {
    fn add_items(&mut self, items: &[String]);
}

#[duplicate_item(
    handler         prompt_type                 return_type;
    [Confirm]       [PromptType::Confirm]       [bool];
    [Input]         [PromptType::Input]         [String];
    [Select]        [PromptType::Select]        [usize];
    [Password]      [PromptType::Password]      [String];
)]
impl handler {
    /// Create the prompt, returning an error if interactive context is incorrectly set.
    pub fn new() -> Result<Self, InteractionError> {
        Ok(Self(match crate::get_context()? {
            InteractiveContext::Terminal => Box::new(terminal::handler::with_theme(THEME.as_ref())),
            InteractiveContext::NotInteractive => Box::new(PseudoInteractive::new(prompt_type)),
        }))
    }

    /// Set the prompt.
    pub fn set_prompt(&mut self, prompt: String) {
        self.0.set_prompt(prompt);
    }

    /// Builder pattern for [`Self::set_prompt`]
    pub fn with_prompt<S: ToString>(&mut self, prompt: S) -> &mut Self {
        self.set_prompt(prompt.to_string());
        self
    }

    /// Present the prompt to the user. May return an error if in a non-interactive context,
    /// or interaction fails for any other reason
    pub fn interact(&mut self) -> Result<return_type, InteractionError> {
        self.0.interact()
    }
}

#[duplicate_item(
    handler         return_type;
    [Confirm]       [bool];
    [Input]         [String];
    [Select]   [usize];
)]
impl handler {
    /// Set the default selection. If the user does not input anything, this value will be used instead.
    pub fn set_default(&mut self, value: return_type) {
        self.0.set_default(value);
    }

    /// Builder pattern for [`Self::set_default`]
    pub fn with_default<I: Into<return_type>>(&mut self, value: I) -> &mut Self {
        self.set_default(value.into());
        self
    }
}

impl Select {
    /// Add items to be displayed in the selection prompt.
    pub fn add_items<S: ToString>(&mut self, items: &[S]) {
        let string_items: Vec<String> = items.iter().map(ToString::to_string).collect();
        self.0.add_items(string_items.as_slice());
    }

    /// Builder pattern for [`Self::add_items`].
    ///
    /// NOTE: if this function is called multiple times, it will add ALL items to the builder.
    pub fn with_items<S: ToString>(&mut self, items: &[S]) -> &mut Self {
        self.add_items(items);
        self
    }
}

impl Password {
    /// Ask the user to confirm the password with the provided prompt & error message.
    pub fn set_confirmation<S: ToString>(&mut self, confirm_prompt: S, mismatch_err: S) {
        self.0
            .set_confirmation(confirm_prompt.to_string(), mismatch_err.to_string());
    }

    /// Builder pattern for [`Self::set_confirmation`]
    pub fn with_confirmation<S: ToString>(
        &mut self,
        confirm_prompt: S,
        mismatch_err: S,
    ) -> &mut Self {
        self.set_confirmation(confirm_prompt, mismatch_err);
        self
    }
}

#[duplicate_item(
    handler         prompt_type;
    [Input]         [PromptType::Input];
    [Password]      [PromptType::Password];
)]
impl handler {
    /// Sets if no input is a valid input. Default: `false`.
    pub fn set_allow_empty(&mut self, empty: bool) {
        self.0.allow_empty(empty);
    }

    /// Builder pattern for [`Self::set_allow_empty`]
    pub fn with_allow_empty(&mut self, empty: bool) -> &mut Self {
        self.set_allow_empty(empty);
        self
    }

    /// Set a validator to be run on input. If the validator returns [`Ok`], the input will be deemed
    /// valid. If the validator returns [`Err`], the prompt will display the error message
    pub fn set_validator<V, E>(&mut self, validator: V)
    where
        V: Fn(&String) -> Result<(), E> + 'static,
        E: ToString,
    {
        self.0
            .set_validator(Box::new(move |input| match validator(input) {
                Ok(()) => Ok(()),
                Err(e) => Err(e.to_string()),
            }));
    }

    /// Builder pattern for [`Self::set_validator`]
    pub fn with_validator<V, E>(&mut self, validator: V) -> &mut Self
    where
        V: Fn(&String) -> Result<(), E> + 'static,
        E: ToString,
    {
        self.set_validator(validator);
        self
    }
}

impl Input {
    pub fn set_inital_text<S: ToString>(&mut self, text: S) {
        self.0.set_inital_text(text.to_string());
    }

    pub fn with_initial_text<S: ToString>(&mut self, text: S) -> &mut Self {
        self.set_inital_text(text);
        self
    }
}
