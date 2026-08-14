//! Command line entry point.
//!
//! Exit codes are part of the contract with hook runners and CI:
//!
//! | Code | Meaning |
//! | :--- | :--- |
//! | 0 | Nothing to do, or files were rewritten successfully |
//! | 1 | `--check` found a difference, or a document is missing its markers |
//! | 2 | A file could not be read, parsed or written |
//!
//! Rewriting a file is deliberately *not* and error. Hook runners detect
//! modified files themselves, and conflating the two makes `--check` useless in
//! CI, where "would change" is exactly the signal worth having.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use actdocs_rs::parse::{self, Document};
use actdocs_rs::render::table;
use actdocs_rs::sync::{self, Options};
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "actdocs-rs", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Regenerate documentation for the given actions and workflows.
    Sync(SyncArgs),
    /// Print the generated documentation for a single file to stdout.
    Generate(GenerateArgs),
}

// Fields are consumed by the command implementations, which land in later
// milestones. They are declared now so the interface can be reviewed and so
// `--help` describes the real thing.
#[derive(Debug, Args)]
struct SyncArgs {
    /// Action or workflow files to document.
    ///
    /// Targets are never discovered: the set is defined by the caller. Passing
    /// none is only useful together with `--index-target`.
    targets: Vec<PathBuf>,

    /// Also write documentation under this directory, mirroring the source
    /// layout as `<DIR>/actions/<name>.md` and `<DIR>/workflows/<name>.md`.
    ///
    /// Without it, only the document beside each source is written.
    #[arg(long, value_name = "DIR")]
    docs_dir_target: Option<PathBuf>,

    /// Rebuild the repository index between the index markers of this file.
    ///
    /// The index lists every action and reusable workflow in the repository,
    /// not only the targets given. Without this flag no index is written.
    #[arg(long, value_name = "FILE")]
    index_target: Option<PathBuf>,

    /// Report whether any file would change, and write nothing.
    #[arg(long)]
    check: bool,

    /// Repository slug stamped into usage snippets.
    #[arg(long, env = "ACTION_REPO_SLUG", default_value = "<owner>/<repo>")]
    repo_slug: String,

    /// Commit SHA stamped into usage snippets.
    #[arg(long, env = "ACTION_REF_SHA", default_value = "<sha>")]
    ref_sha: String,

    /// Version stamped into usage snippets, as a trailing comment.
    #[arg(long, env = "ACTION_REF_VERSION", default_value = "<version>")]
    ref_version: String,

    /// Repository root that generated paths are resolved against.
    #[arg(long, default_value = ".")]
    root: PathBuf,
}

#[allow(dead_code)]
#[derive(Debug, Args)]
struct GenerateArgs {
    /// The `action.yml` or reusable workflow to read.
    file: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Markdown)]
    format: Format,

    /// Leave out sections that have no entries.
    #[arg(long)]
    omit: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Markdown,
    Json,
}

/// Whether the run left the working tree as it found it.
///
/// Nothing constructs these until the commands are implemented.
enum Outcome {
    Clean,
    Diff,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(Outcome::Clean) => ExitCode::SUCCESS,
        Ok(Outcome::Diff) => ExitCode::from(1),
        Err(error) => {
            eprintln!("actdocs-rs: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<Outcome> {
    match cli.command {
        Command::Sync(args) => sync(args),
        Command::Generate(args) => generate(&args),
    }
}

fn sync(args: SyncArgs) -> Result<Outcome> {
    let options = Options {
        root: args.root,
        docs_dir: args.docs_dir_target,
        index: args.index_target,
        check: args.check,
        repo_slug: args.repo_slug,
        ref_sha: args.ref_sha,
        ref_version: args.ref_version,
    };

    // Diagnostics go to stderr so that a hook runner shows them without them
    // being mistaken for output.
    let report = sync::run(&args.targets, &options, &mut io::stderr())?;

    Ok(if report.is_clean() {
        Outcome::Clean
    } else {
        Outcome::Diff
    })
}

fn generate(args: &GenerateArgs) -> Result<Outcome> {
    let path = args.file.display();
    let source = fs::read_to_string(&args.file).with_context(|| format!("cannot read {path}"))?;
    let document = parse::parse(&source).with_context(|| format!("cannot parse {path}"))?;

    let rendered = match args.format {
        Format::Markdown => table::document(&document, args.omit),
        Format::Json => match &document {
            Document::Action(spec) => serde_json::to_string_pretty(spec)?,
            Document::Workflow(spec) => serde_json::to_string_pretty(spec)?,
        },
    };

    println!("{rendered}");
    Ok(Outcome::Clean)
}
