use super::{BasePrompt, InteractionError, PasswordPrompt, TextPrompt, ValidationPrompt};
use super::{DefaultPrompt, SelectionPrompt};
pub use dialoguer::{Confirm, FuzzySelect as Select, Input, Password};
use duplicate::duplicate_item;

#[duplicate_item(
    handler       with_generics         return_type;
    [Confirm]     [Confirm<'_>]         [bool];
    [Input]       [Input<'_, String>]   [String];
    [Select] [Select<'_>]     [usize];
    [Password]    [Password<'_>]        [String];
)]
impl BasePrompt<return_type> for with_generics {
    fn set_prompt(&mut self, prompt: String) {
        self.with_prompt(prompt);
    }

    fn interact(&mut self) -> Result<return_type, InteractionError> {
        Ok(handler::interact(self)?)
    }
}

#[duplicate_item(
    handler       with_generics         return_type;
    [Confirm]     [Confirm<'_>]         [bool];
    [Input]       [Input<'_, String>]   [String];
    [Select] [Select<'_>]     [usize];
)]
impl DefaultPrompt<return_type> for with_generics {
    fn set_default(&mut self, value: return_type) {
        self.default(value);
    }
}

impl SelectionPrompt<usize> for Select<'_> {
    fn add_items(&mut self, items: &[String]) {
        Select::items(self, items);
    }
}

impl ValidationPrompt<String> for Input<'_, String> {
    fn allow_empty(&mut self, empty: bool) {
        self.allow_empty(empty);
    }

    fn set_validator(&mut self, validator: Box<dyn Fn(&String) -> Result<(), String>>) {
        self.validate_with(validator);
    }
}

impl ValidationPrompt<String> for Password<'_> {
    fn allow_empty(&mut self, empty: bool) {
        self.allow_empty_password(empty);
    }

    fn set_validator(&mut self, validator: Box<dyn Fn(&String) -> Result<(), String>>) {
        self.validate_with(validator);
    }
}

impl PasswordPrompt<String> for Password<'_> {
    fn set_confirmation(&mut self, confirm_prompt: String, mismatch_err: String) {
        self.with_confirmation(confirm_prompt, mismatch_err);
    }
}

impl TextPrompt<String> for Input<'_, String> {
    fn set_inital_text(&mut self, text: String) {
        self.with_initial_text(text);
    }
}
