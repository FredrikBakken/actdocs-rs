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

use actdocs_rs::config::Config;
use actdocs_rs::parse::{self, Document};
use actdocs_rs::render::{table, usage};
use actdocs_rs::sync;
use actdocs_rs::target::Placement;
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

    /// Read settings from this file instead of searching for one.
    ///
    /// A file named here must exist, and replaces the search entirely. The
    /// search looks for `.actdocs.toml`, `actdocs.toml`, `config/actdocs.toml`
    /// and `.config/actdocs.toml`, in that order, under `--root`.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Also write documentation under this directory, mirroring the source
    /// layout as `<DIR>/actions/<name>.md` and `<DIR>/workflows/<name>.md`.
    ///
    /// Without it, only the document beside each source is written.
    #[arg(long, value_name = "DIR")]
    docs_dir_target: Option<PathBuf>,

    /// Where workflow documents are written [default: beside].
    ///
    /// `beside` writes `.github/workflows/lint.md` next to the workflow, and
    /// the mirror as well if `--docs-dir-target` was given. `docs-dir` writes
    /// only the mirror, which keeps the workflow directory to workflows and
    /// therefore requires that flag. Actions are unaffected: each already has
    /// a directory of its own.
    #[arg(long, value_enum)]
    workflow_docs: Option<WorkflowDocs>,

    /// Rebuild the repository index between the index markers of this file.
    ///
    /// The index lists every action and reusable workflow in the repository,
    /// not only the targets given. Without this flag no index is written.
    #[arg(long, value_name = "FILE")]
    index_target: Option<PathBuf>,

    /// Repository slug stamped into usage snippets [default: <owner>/<repo>].
    #[arg(long, env = "ACTDOCS_REPO_SLUG")]
    repo_slug: Option<String>,

    /// Commit SHA stamped into usage snippets [default: <sha>].
    #[arg(long, env = "ACTDOCS_REF_SHA")]
    ref_sha: Option<String>,

    /// Version stamped into usage snippets [default: <version>].
    #[arg(long, env = "ACTDOCS_REF_VERSION")]
    ref_version: Option<String>,

    /// How usage snippets pin the action [default: sha].
    ///
    /// `sha` writes `@<sha>  # <version>`, which resolves to one commit
    /// whatever happens to the tag. `version` writes `@<version>`, which is
    /// only as strong where the publishing repository has enabled immutable
    /// releases.
    #[arg(long, value_enum)]
    pin: Option<Pin>,

    /// Report whether any file would change, and write nothing.
    ///
    /// Deliberately not settable from a file: it is a mode of one invocation,
    /// not a property of the repository.
    #[arg(long)]
    check: bool,

    /// Repository root that generated paths resolve against.
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

/// Mirrors `usage::Pin` rather than deriving `ValueEnum` on it, so the library
/// stays free of a command line parser it has no other use for.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Pin {
    Sha,
    Version,
}

impl From<Pin> for usage::Pin {
    fn from(pin: Pin) -> Self {
        match pin {
            Pin::Sha => Self::Sha,
            Pin::Version => Self::Version,
        }
    }
}

/// Mirrors `target::Placement`, for the same reason `Pin` is mirrored.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum WorkflowDocs {
    Beside,
    DocsDir,
}

impl From<WorkflowDocs> for Placement {
    fn from(placement: WorkflowDocs) -> Self {
        match placement {
            WorkflowDocs::Beside => Self::Beside,
            WorkflowDocs::DocsDir => Self::DocsDir,
        }
    }
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
    // The flags are just another layer, so precedence is one expression rather
    // than a pile of conditionals. Clap has already resolved flag-over-env.
    let cli = Config {
        docs_dir_target: args.docs_dir_target,
        index_target: args.index_target,
        workflow_docs: args.workflow_docs.map(Into::into),
        repo_slug: args.repo_slug,
        ref_sha: args.ref_sha,
        ref_version: args.ref_version,
        pin: args.pin.map(Into::into),
    };
    let file = Config::load(&args.root, args.config.as_deref(), &mut io::stderr())?;
    let options = cli.or(file).into_options(args.root, args.check)?;

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
