mod commands;

use atomic_interaction::InteractiveContext;
use clap::{ColorChoice, Parser};
use env_logger::fmt::Color;
use human_panic::setup_panic;
use std::ffi::OsString;
use std::io::Write;

use crate::commands::*;

#[derive(Parser, Debug)]
#[clap(version, author, color(ColorChoice::Auto), infer_subcommands = true)]
pub struct Opts {
    #[clap(subcommand)]
    pub subcmd: SubCommand,
    /// Abort rather than prompt for input
    #[clap(long, global = true)]
    pub no_prompt: bool,
}

#[derive(Parser, Debug)]
pub enum SubCommand {
    /// Initializes an empty atomic repository
    Init(Init),

    /// Clones an existing atomic repository
    Clone(Clone),

    /// Creates a new change
    Record(Record),

    /// Shows difference between two channels/changes
    Diff(Diff),

    /// Show the entire log of changes
    Log(Log),

    /// Pushes changes to a remote upstream
    Push(Push),

    /// Pulls changes from a remote upstream
    Pull(Pull),

    /// Shows information about a particular change
    Change(Change),

    /// Lists the transitive closure of the reverse dependency relation
    Dependents(Dependents),

    /// Manages different channels
    Channel(Channel),

    #[clap(hide = true)]
    Protocol(Protocol),

    #[cfg(feature = "git")]
    /// Imports a git repository into atomic
    Git(Git),

    /// Moves a file in the working copy and the tree
    #[clap(alias = "mv")]
    Move(Move),

    /// Lists files tracked by atomic
    #[clap(alias = "ls")]
    List(List),

    /// Adds a path to the tree.
    ///
    /// Atomic has an internal tree to represent the files currently
    /// tracked. This command adds files and directories to that tree.
    Add(Add),

    /// Removes a file from the tree of tracked files (`atomic record`
    /// will then record this as a deletion).
    #[clap(alias = "rm")]
    Remove(Remove),

    /// Resets the working copy to the last recorded change.
    ///
    /// In other words, discards all unrecorded changes.
    Reset(Reset),

    // #[cfg(debug_assertions)]
    Debug(Debug),

    /// Create a new channel
    Fork(Fork),

    /// Unrecords a list of changes.
    ///
    /// The changes will be removed from your log, but your working
    /// copy will stay exactly the same, unless the
    /// `--reset` flag was passed. A change can only be unrecorded
    /// if all changes that depend on it are also unrecorded in the
    /// same operation. There are two ways to call `atomic-unrecord`:
    ///
    /// * With a list of <change-id>s. The given changes will be
    /// unrecorded, if possible.
    ///
    /// * Without listing any <change-id>s. You will be
    /// presented with a list of changes to choose from.
    /// The length of the list is determined by the `unrecord_changes`
    /// setting in your global config or the `--show-changes` option,
    /// with the latter taking precedence.
    Unrecord(Unrecord),

    /// Applies changes to a channel
    Apply(Apply),

    /// Manages remote repositories
    Remote(Remote),

    /// Creates an archive of the repository
    Archive(Archive),

    /// Shows which change last affected each line of the given file(s)
    Credit(Credit),

    /// Manage tags (create tags, check out a tag)
    Tag(Tag),

    /// A collection of tools for interactively managing the user's identities.
    /// This may be useful if you use Atomic in multiple contexts, for example
    /// both work & personal projects.
    #[clap(alias = "id", alias = "key")]
    Identity(IdentityCommand),

    /// Authenticates with a HTTP server.
    Client(Client),

    /// Shell completion script generation
    Completion(Completion),

    /// Shows AI attribution statistics and information
    Attribution(Attribution),

    /// Outputs the current channel for shell prompt integration
    Prompt(Prompt),

    #[clap(external_subcommand)]
    ExternalSubcommand(Vec<OsString>),
}

#[test]
/// Make sure all clap derive macros are (reasonably) correct
fn clap_debug_assert() {
    use clap::CommandFactory;
    Opts::command().debug_assert();
}

#[tokio::main]
async fn main() {
    setup_panic!();
    if cfg!(debug_assertions) {
        env_logger::init();
    } else {
        env_logger_init();
    }

    let opts = Opts::parse();
    if opts.no_prompt {
        atomic_interaction::set_context(InteractiveContext::NotInteractive);
    } else {
        atomic_interaction::set_context(InteractiveContext::Terminal);
    }

    if let Err(e) = run(opts).await {
        // This will only activate with the following environment variables:
        // RUST_BACKTRACE=1 RUST_LOG=error
        log::error!("Error: {:#?}", e);
        match e.downcast::<std::io::Error>() {
            Ok(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            Ok(e) => writeln!(std::io::stderr(), "Error: {}", e).unwrap_or(()),
            Err(e) => writeln!(std::io::stderr(), "Error: {}", e).unwrap_or(()),
        }
        std::process::exit(1);
    } else {
        std::process::exit(0);
    }
}

fn env_logger_init() {
    let mut builder = env_logger::builder();
    builder.filter(Some("atomic::commands::git"), log::LevelFilter::Info);
    builder.format(|buf, record| {
        let target = record.metadata().target();
        if target == "atomic::commands::git" {
            let mut level_style = buf.style();
            level_style.set_color(Color::Green);
            writeln!(
                buf,
                "{} {}",
                level_style.value(record.level()),
                record.args()
            )
        } else {
            let mut level_style = buf.style();
            level_style.set_color(Color::Black).set_intense(true);
            let op = level_style.value("[");
            let cl = level_style.value("]");
            writeln!(
                buf,
                "{}{} {} {}{} {}",
                op,
                buf.timestamp(),
                buf.default_styled_level(record.level()),
                target,
                cl,
                record.args()
            )
        }
    });
    builder.init();
}

#[cfg(unix)]
fn run_external_command(mut command: Vec<OsString>) -> Result<(), std::io::Error> {
    let args = command.split_off(1);
    let mut cmd: OsString = "atomic-".into();
    cmd.push(&command[0]);

    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(&cmd).args(args).exec();
    report_external_command_error(&command[0], err);
}

#[cfg(windows)]
fn run_external_command(mut command: Vec<OsString>) -> Result<(), std::io::Error> {
    let args = command.split_off(1);
    let mut cmd: OsString = "atomic-".into();
    cmd.push(&command[0]);

    let mut spawned = match std::process::Command::new(&cmd).args(args).spawn() {
        Ok(spawned) => spawned,
        Err(e) => {
            report_external_command_error(&command[0], e);
        }
    };
    let status = spawned.wait()?;
    std::process::exit(status.code().unwrap_or(1))
}

fn report_external_command_error(cmd: &OsString, err: std::io::Error) -> ! {
    match err.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
            writeln!(std::io::stderr(), "No such subcommand: {:?}", cmd).unwrap_or(())
        }
        _ => writeln!(std::io::stderr(), "Error while running {:?}: {}", cmd, err).unwrap_or(()),
    }
    std::process::exit(1)
}

async fn run(opts: Opts) -> Result<(), anyhow::Error> {
    match opts.subcmd {
        SubCommand::Log(l) => l.run(),
        SubCommand::Init(init) => init.run(),
        SubCommand::Clone(clone) => clone.run().await,
        SubCommand::Record(record) => record.run().await,
        SubCommand::Diff(diff) => diff.run(),
        SubCommand::Push(push) => push.run().await,
        SubCommand::Pull(pull) => pull.run().await,
        SubCommand::Change(change) => change.run(),
        SubCommand::Dependents(deps) => deps.run(),
        SubCommand::Channel(channel) => channel.run(),
        SubCommand::Protocol(protocol) => protocol.run(),
        #[cfg(feature = "git")]
        SubCommand::Git(git) => git.run(),
        SubCommand::Move(move_cmd) => move_cmd.run(),
        SubCommand::List(list) => list.run(),
        SubCommand::Add(add) => add.run(),
        SubCommand::Remove(remove) => remove.run(),
        SubCommand::Reset(reset) => reset.run(),
        SubCommand::Debug(debug) => debug.run(),
        SubCommand::Fork(fork) => fork.run(),
        SubCommand::Unrecord(unrecord) => unrecord.run(),
        SubCommand::Apply(apply) => apply.run(),
        SubCommand::Remote(remote) => remote.run(),
        SubCommand::Archive(archive) => archive.run().await,
        SubCommand::Credit(credit) => credit.run(),
        SubCommand::Tag(tag) => tag.run().await,
        SubCommand::Identity(identity_wizard) => identity_wizard.run().await,
        SubCommand::Client(client) => client.run().await,
        SubCommand::ExternalSubcommand(command) => Ok(run_external_command(command)?),
        SubCommand::Completion(completion) => completion.run(),
        SubCommand::Attribution(attribution) => attribution.run(),
        SubCommand::Prompt(prompt) => prompt.run(),
    }
}
