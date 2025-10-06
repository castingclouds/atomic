mod terminal;

use super::{ProgressBar, Spinner};
use crate::{InteractionError, InteractiveContext};

pub trait ProgressBarTrait: Send {
    fn inc(&self, delta: u64);
    fn finish(&self);
    fn boxed_clone(&self) -> Box<dyn ProgressBarTrait>;
}

impl ProgressBar {
    pub fn new<S: ToString>(len: u64, message: S) -> Result<ProgressBar, InteractionError> {
        Ok(Self(match crate::get_context()? {
            InteractiveContext::Terminal | InteractiveContext::NotInteractive => {
                Box::new(terminal::new_progress(len, message.to_string()))
            }
        }))
    }

    pub fn inc(&self, delta: u64) {
        self.0.inc(delta);
    }

    fn finish(&self) {
        self.0.finish()
    }
}

impl Drop for ProgressBar {
    fn drop(&mut self) {
        self.finish();
    }
}

impl Clone for ProgressBar {
    fn clone(&self) -> Self {
        Self(self.0.boxed_clone())
    }
}

pub trait SpinnerTrait: Send {
    fn finish(&self);
    fn boxed_clone(&self) -> Box<dyn SpinnerTrait>;
}

impl Spinner {
    pub fn new<S: ToString>(message: S) -> Result<Spinner, InteractionError> {
        Ok(Self(match crate::get_context()? {
            InteractiveContext::Terminal | InteractiveContext::NotInteractive => {
                Box::new(terminal::new_spinner(message.to_string()))
            }
        }))
    }

    fn finish(&self) {
        self.0.finish();
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.finish();
    }
}

impl Clone for Spinner {
    fn clone(&self) -> Self {
        Self(self.0.boxed_clone())
    }
}
