use super::{
    BasePrompt, DefaultPrompt, InteractionError, PasswordPrompt, PromptType, SelectionPrompt,
    TextPrompt, ValidationPrompt,
};
use core::fmt::Debug;
use log::{error, info, warn};

/// Holds state for non-interactive contexts so that non-interactive contexts
/// such as `atomic XXX --no-prompt` can use the same interface, and to produce
/// nicer debugging output.
pub struct PseudoInteractive<T: Clone + Debug> {
    prompt_type: PromptType,
    prompt: Option<String>,
    default: Option<T>,
    items: Vec<String>,
    validator: Option<Box<dyn Fn(&T) -> Result<(), String>>>,
    confirmation: Option<(String, String)>,
    allow_empty: bool,
    initial_value: Option<T>,
}

impl<T: Clone + Debug> PseudoInteractive<T> {
    pub fn new(prompt_type: PromptType) -> Self {
        Self {
            prompt_type,
            prompt: None,
            default: None,
            items: Vec::new(),
            validator: None,
            confirmation: None,
            allow_empty: false,
            initial_value: None,
        }
    }
}

impl<T: Clone + Debug> BasePrompt<T> for PseudoInteractive<T> {
    fn set_prompt(&mut self, prompt: String) {
        self.prompt = Some(prompt);
    }

    fn interact(&mut self) -> Result<T, InteractionError> {
        let prompt = self
            .prompt
            .clone()
            .unwrap_or_else(|| "[NO PROMPT SET]".to_owned());

        let default = if let Some(initial_value) = &self.initial_value {
            Some(initial_value.clone())
        } else if let Some(default) = &self.default {
            Some(default.clone())
        } else {
            None
        };

        if let Some(default) = default {
            warn!(
                "Non-interactive context. The {:?} prompt `{prompt}` will default to {default:#?} .",
                self.prompt_type
            );

            if let Some(validator) = self.validator.as_mut() {
                warn!(
                    "Non-interactive context. The {:?} prompt `{prompt}` will default to {default:#?} if valid.",
                    self.prompt_type
                );
                match validator(&default) {
                    Ok(_) => {
                        info!("Default value passed validation.");
                        Ok(default.to_owned())
                    }
                    Err(err) => {
                        error!("Default value failed validation: {err}");
                        Err(InteractionError::NotInteractive(self.prompt_type, prompt))
                    }
                }
            } else {
                warn!(
                    "Non-interactive context. The {:?} prompt `{prompt}` will default to {default:#?}.",
                    self.prompt_type
                );
                Ok(default.to_owned())
            }
        } else {
            error!("No default value found.");
            Err(InteractionError::NotInteractive(self.prompt_type, prompt))
        }
    }
}

impl<T: Clone + Debug> DefaultPrompt<T> for PseudoInteractive<T> {
    fn set_default(&mut self, value: T) {
        self.default = Some(value);
    }
}

impl<T: Clone + Debug> SelectionPrompt<T> for PseudoInteractive<T> {
    fn add_items(&mut self, items: &[String]) {
        self.items = Vec::from(items);
    }
}

impl<T: Clone + Debug> ValidationPrompt<T> for PseudoInteractive<T> {
    fn allow_empty(&mut self, empty: bool) {
        self.allow_empty = empty;
    }

    fn set_validator(&mut self, validator: Box<dyn Fn(&T) -> Result<(), String>>) {
        self.validator = Some(validator);
    }
}

impl<T: Clone + Debug> PasswordPrompt<T> for PseudoInteractive<T> {
    fn set_confirmation(&mut self, confirm_prompt: String, mismatch_err: String) {
        self.confirmation = Some((confirm_prompt, mismatch_err));
    }
}

impl TextPrompt<String> for PseudoInteractive<String> {
    fn set_inital_text(&mut self, text: String) {
        self.initial_value = Some(text);
    }
}
