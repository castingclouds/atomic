use std::sync::Arc;
use std::time::Duration;

use super::{ProgressBarTrait, SpinnerTrait};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use lazy_static::lazy_static;

lazy_static! {
    static ref MULTI_PROGRESS: MultiProgress = MultiProgress::new();
}

pub fn new_progress(len: u64, message: String) -> Arc<ProgressBar> {
    let style =
        ProgressStyle::with_template("{msg:<20} [{bar:50}] {pos}/{len} [{elapsed_precise}]")
            .unwrap()
            .progress_chars("=> ");
    let progress_bar = ProgressBar::new(len)
        .with_style(style)
        .with_message(message);
    MULTI_PROGRESS.add(progress_bar.clone());
    progress_bar.enable_steady_tick(Duration::from_millis(15));

    Arc::new(progress_bar)
}

impl ProgressBarTrait for Arc<ProgressBar> {
    fn inc(&self, delta: u64) {
        self.as_ref().inc(delta);
    }

    fn finish(&self) {
        // Only finish the progress bar if it's the last reference
        if Arc::strong_count(self) == 1 {
            self.as_ref().finish();
        }
    }

    fn boxed_clone(&self) -> Box<dyn ProgressBarTrait> {
        Box::new(self.clone())
    }
}

pub fn new_spinner(message: String) -> Arc<ProgressBar> {
    let style = ProgressStyle::with_template("{msg}{spinner}")
        .unwrap()
        .tick_strings(&[".  ", ".. ", "...", "   "]);
    let spinner = ProgressBar::new_spinner()
        .with_style(style)
        .with_message(message);
    spinner.enable_steady_tick(Duration::from_millis(200));
    MULTI_PROGRESS.add(spinner.clone());

    Arc::new(spinner)
}

impl SpinnerTrait for Arc<ProgressBar> {
    fn finish(&self) {
        // Only display finish message if it's the last reference
        if Arc::strong_count(self) == 1 {
            self.set_style(ProgressStyle::with_template("{msg}").unwrap());
            self.finish_with_message(format!("{}... done!", self.message()));
        }
    }

    fn boxed_clone(&self) -> Box<dyn SpinnerTrait> {
        Box::new(self.clone())
    }
}
