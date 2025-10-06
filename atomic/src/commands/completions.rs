use std::io;

use clap::CommandFactory;
use clap::Parser;
use clap_complete::{
    generate,
    shells::{Bash, Elvish, Fish, PowerShell, Zsh},
};

use crate::Opts;

#[derive(Parser, Debug)]
pub struct Completion {
    #[clap(subcommand)]
    subcmd: Option<SubCommand>,
}

#[derive(Parser, Debug)]
pub enum SubCommand {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

impl Completion {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let mut app = Opts::command();
        match self.subcmd {
            Some(SubCommand::Bash) => {
                generate(Bash, &mut app, "atomic", &mut io::stdout());
            }
            Some(SubCommand::Elvish) => {
                generate(Elvish, &mut app, "atomic", &mut io::stdout());
            }
            Some(SubCommand::Fish) => {
                generate(Fish, &mut app, "atomic", &mut io::stdout());
            }
            Some(SubCommand::PowerShell) => {
                generate(PowerShell, &mut app, "atomic", &mut io::stdout());
            }
            Some(SubCommand::Zsh) => {
                generate(Zsh, &mut app, "atomic", &mut io::stdout());
            }
            None => {}
        }
        Ok(())
    }
}
