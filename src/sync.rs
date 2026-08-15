//! Regenerating every document a set of source files owns.
//!
//! This is the whole pipeline: read a source, work out what it documents,
//! rewrite the regions between the markers, and rebuild the repository index.
//! It lives here rather than in the binary so that it can be exercised against
//! a temporary directory instead of a real checkout.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::doc::{self, Update};
use crate::parse::{self, Document};
use crate::render::{index, table, usage};
use crate::target::{self, Kind, Plan, Target};

/// Generated documents leave out sections with no entries, so an action with
/// no outputs says nothing about outputs rather than showing an empty table.
const OMIT_EMPTY_SECTIONS: bool = true;

/// Everything a run needs beyond the list of targets.
#[derive(Debug, Clone)]
pub struct Options {
    /// The repository root that every generated path is resolved against.
    pub root: PathBuf,
    /// Also write documentation under this directory, mirroring the source
    /// layout. `None` means only the document beside each source is written.
    pub docs_dir: Option<PathBuf>,
    /// Rebuild the repository index in this document, resolved against `root`.
    /// `None` means no index is written.
    pub index: Option<PathBuf>,
    /// Report what would change, and write nothing.
    pub check: bool,
    /// Stamped into usage snippets. Deliberately a placeholder by default, so
    /// the output does not differ between a fork, a local clone and CI.
    pub repo_slug: String,
    pub ref_sha: String,
    pub ref_version: String,
    /// How usage snippets pin the action.
    pub pin: usage::Pin,
}

/// What a run found, beyond the files it wrote.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// A document differs, and `--check` meant it was left alone.
    pub would_change: bool,
    /// A document could not be updated and needs a human to fix it.
    pub unwritable: bool,
}

impl Report {
    /// Whether the run left the working tree as it found it, with nothing to
    /// report. Writing a file is not a complaint; failing to is.
    pub fn is_clean(self) -> bool {
        !self.would_change && !self.unwritable
    }
}

/// Document every target, then rebuild the index if one was asked for.
///
/// Targets are never discovered: the set is defined once, by the hook that
/// invokes this. The index is the exception, because by nature it has to list
/// every action in the repository rather than only the ones that just changed.
/// That makes it expensive and surprising, so it is opt-in rather than a side
/// effect of every run.
pub fn run(targets: &[PathBuf], options: &Options, log: &mut dyn io::Write) -> Result<Report> {
    let mut report = Report::default();

    for source in targets {
        document(source, options, log, &mut report)?;
    }
    update_index(options, log, &mut report)?;

    Ok(report)
}

fn document(
    source: &Path,
    options: &Options,
    log: &mut dyn io::Write,
    report: &mut Report,
) -> Result<()> {
    // The hook is handed every workflow the commit touched, most of which
    // document nothing. Skipping them silently is the common case, not a fault.
    let Some(parsed) = read(&options.root, source)? else {
        return Ok(());
    };
    let Some(target) = target::classify(source, options.docs_dir.as_deref()) else {
        writeln!(
            log,
            "{}: not a path this tool can document",
            source.display()
        )?;
        return Ok(());
    };

    // Routing is by path, so contents that disagree with the file name would
    // put the tables somewhere nobody is looking.
    if !is_consistent(&parsed, target.kind) {
        writeln!(
            log,
            "{}: contents do not match the file name, so it was left alone",
            source.display()
        )?;
        report.unwritable = true;
        return Ok(());
    }

    for plan in &target.plans {
        write(&target, &parsed, plan, options, log, report)?;
    }

    Ok(())
}

fn write(
    target: &Target,
    parsed: &Document,
    plan: &Plan,
    options: &Options,
    log: &mut dyn io::Write,
    report: &mut Report,
) -> Result<()> {
    let path = options.root.join(&plan.path);

    // Everything outside the markers is hand-editable and survives every
    // regeneration, so a document is scaffolded exactly once.
    let existing = match read_optional(&path)? {
        Some(text) => text,
        None => doc::scaffold(
            &target.title,
            plan.link.as_ref().map(source_link),
            plan.usage,
        ),
    };

    let tables = table::document(parsed, OMIT_EMPTY_SECTIONS);
    let Some(mut contents) = replace(&existing, doc::ACTDOCS, &tables, &plan.path, log, report)?
    else {
        return Ok(());
    };

    // A document without the usage markers has simply opted out, which is not
    // something to complain about.
    if plan.usage && doc::has_markers(&contents, doc::USAGE) {
        let uses = target.uses_path().unwrap_or_default();
        let reference = usage::Reference {
            repo_slug: &options.repo_slug,
            path: &uses,
            sha: &options.ref_sha,
            version: &options.ref_version,
            pin: options.pin,
        };
        let snippet = match parsed {
            Document::Action(spec) => usage::action(&target.title, spec, reference),
            Document::Workflow(spec) => usage::workflow(&target.title, spec, reference),
        };

        let Some(updated) = replace(&contents, doc::USAGE, &snippet, &plan.path, log, report)?
        else {
            return Ok(());
        };
        contents = updated;
    }

    if doc::write_if_changed(&path, &contents, options.check)? == Update::WouldChange {
        report.would_change = true;
    }

    Ok(())
}

/// Rebuild the index from a full listing of the repository.
fn update_index(options: &Options, log: &mut dyn io::Write, report: &mut Report) -> Result<()> {
    // Without a named document there is nowhere to publish an index, and no
    // reason to walk the repository looking for entries to put in it.
    let Some(path) = options.index.as_deref() else {
        return Ok(());
    };

    // A document that was asked for by name and is not there is a mistake, not
    // a choice — unlike the per-source documents, which are scaffolded. One
    // that exists without the markers is a third case, and `replace` reports it.
    let full = options.root.join(path);
    let existing =
        fs::read_to_string(&full).with_context(|| format!("cannot read {}", full.display()))?;

    let actions = entries(&target::discover_actions(&options.root)?, options)?;
    let workflows = entries(&target::discover_workflows(&options.root)?, options)?;
    let body = index::index(&actions, &workflows);

    let Some(updated) = replace(&existing, doc::INDEX, &body, path, log, report)? else {
        return Ok(());
    };

    if doc::write_if_changed(&full, &updated, options.check)? == Update::WouldChange {
        report.would_change = true;
    }

    Ok(())
}

/// One index row per source that has something to document.
fn entries(sources: &[PathBuf], options: &Options) -> Result<Vec<index::Entry>> {
    let mut entries = Vec::new();

    for source in sources {
        let Some(parsed) = read(&options.root, source)? else {
            continue;
        };
        let Some(target) = target::classify(source, options.docs_dir.as_deref()) else {
            continue;
        };

        // An action summarises itself with `description`; a workflow has no
        // such key, so its `name` is the only summary available.
        let summary = match (&parsed, target.kind) {
            (Document::Action(spec), Kind::Action) => spec.description.clone(),
            (Document::Workflow(spec), Kind::Workflow) => spec.name.clone(),
            _ => continue,
        };

        entries.push(index::Entry {
            label: target.title.clone(),
            href: target.index_href(),
            summary,
        });
    }

    Ok(entries)
}

/// Rewrite one region, reporting a document that has nowhere to write to.
///
/// `Ok(None)` means the caller should give up on this document and move on:
/// missing markers need a human, and there is nothing to retry.
fn replace(
    contents: &str,
    markers: doc::Markers,
    body: &str,
    path: &Path,
    log: &mut dyn io::Write,
    report: &mut Report,
) -> Result<Option<String>> {
    match doc::replace(contents, markers, body) {
        Ok(updated) => Ok(Some(updated)),
        Err(error) => {
            writeln!(log, "{}: {error}", path.display())?;
            report.unwritable = true;
            Ok(None)
        }
    }
}

fn read(root: &Path, source: &Path) -> Result<Option<Document>> {
    let path = root.join(source);
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    parse::try_parse(&text).with_context(|| format!("cannot parse {}", path.display()))
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn source_link(link: &target::Link) -> doc::SourceLink<'_> {
    doc::SourceLink {
        target: &link.target,
        href: &link.href,
    }
}

fn is_consistent(parsed: &Document, kind: Kind) -> bool {
    matches!(
        (parsed, kind),
        (Document::Action(_), Kind::Action) | (Document::Workflow(_), Kind::Workflow)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTION: &str = "\
name: Pre-commit
description: Run hooks.

inputs:
  all-files:
    description: Run against every file.
    default: false

runs:
  using: composite
";

    const MANIFEST: &str = ".github/actions/pre-commit/action.yml";

    /// The document the index tests opt in to.
    const INDEX_DOC: &str = "README.md";

    fn options(root: &Path) -> Options {
        Options {
            root: root.to_path_buf(),
            docs_dir: None,
            index: None,
            check: false,
            repo_slug: "<owner>/<repo>".to_owned(),
            ref_sha: "<sha>".to_owned(),
            ref_version: "<version>".to_owned(),
            pin: usage::Pin::default(),
        }
    }

    /// A run that opts in to the index, as the repository's own hook does.
    fn indexed(root: &Path) -> Options {
        Options {
            index: Some(PathBuf::from(INDEX_DOC)),
            ..options(root)
        }
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".github/actions/pre-commit");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("action.yml"), ACTION).unwrap();
        fs::write(
            root.path().join(INDEX_DOC),
            "# Repo\n\n<!-- index start -->\n<!-- index end -->\n",
        )
        .unwrap();
        root
    }

    fn sync(root: &Path, check: bool) -> (Report, String) {
        let targets = [PathBuf::from(MANIFEST)];
        let mut log = Vec::new();
        let report = run(
            &targets,
            &Options {
                check,
                ..options(root)
            },
            &mut log,
        )
        .unwrap();
        (report, String::from_utf8(log).unwrap())
    }

    fn read_doc(root: &Path, path: &str) -> String {
        fs::read_to_string(root.path_join(path)).unwrap()
    }

    // Helper so the tests read as paths rather than joins.
    trait Join {
        fn path_join(&self, path: &str) -> PathBuf;
    }
    impl Join for Path {
        fn path_join(&self, path: &str) -> PathBuf {
            self.join(path)
        }
    }

    #[test]
    fn a_missing_document_is_scaffolded_and_filled_in() {
        let root = repository();
        let (report, log) = sync(root.path(), false);

        assert!(report.is_clean(), "{report:?} {log}");

        let readme = read_doc(root.path(), ".github/actions/pre-commit/README.md");
        assert!(readme.starts_with("# pre-commit\n"), "got {readme}");
        assert!(
            readme.contains("## Description\n\nRun hooks."),
            "got {readme}"
        );
        assert!(readme.contains("| all-files |"), "got {readme}");
        assert!(
            readme.contains("uses: <owner>/<repo>/.github/actions/pre-commit@<sha>  # <version>"),
            "got {readme}"
        );
    }

    #[test]
    fn the_mirrored_document_links_back_to_its_source() {
        let root = repository();
        let mut log = Vec::new();
        run(
            &[PathBuf::from(".github/actions/pre-commit/action.yml")],
            &Options {
                docs_dir: Some(PathBuf::from("docs")),
                ..options(root.path())
            },
            &mut log,
        )
        .unwrap();

        let mirrored = read_doc(root.path(), "docs/actions/pre-commit.md");
        assert!(
                mirrored.contains("Generated from [`.github/actions/pre-commit/action.yml`](../../.github/actions/pre-commit/action.yml)."),
                "got {mirrored}"
            );
    }

    #[test]
    fn no_mirror_is_written_without_a_documentation_root() {
        let root = repository();
        sync(root.path(), false);

        assert!(!root.path().join("docs").exists());
    }

    #[test]
    fn an_action_with_no_outputs_says_nothing_about_outputs() {
        let root = repository();
        sync(root.path(), false);

        let readme = read_doc(root.path(), ".github/actions/pre-commit/README.md");
        assert!(!readme.contains("## Outputs"), "got {readme}");
    }

    #[test]
    fn the_index_lists_every_action() {
        let root = repository();
        let mut log = Vec::new();
        run(&[PathBuf::from(MANIFEST)], &indexed(root.path()), &mut log).unwrap();

        let index = read_doc(root.path(), INDEX_DOC);
        assert!(
            index.contains("| [`pre-commit`](.github/actions/pre-commit/README.md) | Run hooks. |"),
            "got {index}"
        );
    }

    #[test]
    fn no_index_is_written_without_a_target() {
        let root = repository();
        let before = read_doc(root.path(), INDEX_DOC);

        sync(root.path(), false);

        assert_eq!(read_doc(root.path(), INDEX_DOC), before);
    }

    #[test]
    fn a_named_index_that_is_missing_is_an_error() {
        let root = repository();
        let mut log = Vec::new();

        let result = run(
            &[],
            &Options {
                index: Some(PathBuf::from("docs/INDEX.md")),
                ..options(root.path())
            },
            &mut log,
        );

        assert!(result.is_err(), "a named index should not be invented");
    }

    #[test]
    fn a_second_run_changes_nothing() {
        let root = repository();
        sync(root.path(), false);
        let once = read_doc(root.path(), ".github/actions/pre-commit/README.md");

        let (report, log) = sync(root.path(), false);

        assert!(report.is_clean(), "{report:?} {log}");
        assert_eq!(
            read_doc(root.path(), ".github/actions/pre-commit/README.md"),
            once
        );
    }

    #[test]
    fn checking_reports_a_difference_without_writing() {
        let root = repository();
        let (report, _) = sync(root.path(), true);

        assert!(report.would_change);
        assert!(!report.unwritable);
        assert!(!root.path().join("docs/actions/pre-commit.md").exists());
    }

    #[test]
    fn checking_an_up_to_date_repository_is_clean() {
        let root = repository();
        sync(root.path(), false);

        let (report, log) = sync(root.path(), true);

        assert!(report.is_clean(), "{report:?} {log}");
    }

    #[test]
    fn a_document_with_no_markers_is_reported_rather_than_overwritten() {
        let root = repository();
        let readme = root.path().join(".github/actions/pre-commit/README.md");
        fs::write(&readme, "# Hand written\n").unwrap();

        let (report, log) = sync(root.path(), false);

        assert!(report.unwritable, "{log}");
        assert!(log.contains("README.md"), "got {log}");
        assert_eq!(fs::read_to_string(&readme).unwrap(), "# Hand written\n");
    }

    #[test]
    fn an_ordinary_workflow_is_skipped_silently() {
        let root = repository();
        let workflows = root.path().join(".github/workflows");
        fs::create_dir_all(&workflows).unwrap();
        fs::write(workflows.join("ci.yml"), "name: CI\non:\n  push:\n").unwrap();

        let mut log = Vec::new();
        let report = run(
            &[PathBuf::from(".github/workflows/ci.yml")],
            &options(root.path()),
            &mut log,
        )
        .unwrap();

        assert!(report.is_clean());
        assert!(log.is_empty());
        assert!(!root.path().join("docs/workflows/ci.md").exists());
    }

    #[test]
    fn the_index_is_rebuilt_even_with_no_targets() {
        let root = repository();
        let mut log = Vec::new();

        let report = run(&[], &indexed(root.path()), &mut log).unwrap();

        assert!(report.is_clean());
        assert!(read_doc(root.path(), INDEX_DOC).contains("## Available actions"));
    }

    #[test]
    fn a_version_pin_reaches_the_snippet() {
        let root = repository();
        let mut log = Vec::new();
        run(
            &[PathBuf::from(MANIFEST)],
            &Options {
                pin: usage::Pin::Version,
                ..options(root.path())
            },
            &mut log,
        )
        .unwrap();

        let readme = read_doc(root.path(), ".github/actions/pre-commit/README.md");
        assert!(
            readme.contains("uses: <owner>/<repo>/.github/actions/pre-commit@<version>\n"),
            "got {readme}"
        );
        assert!(!readme.contains("<sha>"), "got {readme}");
    }

    #[test]
    fn a_reusable_workflow_gets_a_usage_snippet_too() {
        let root = repository();
        let workflows = root.path().join(".github/workflows");
        fs::create_dir_all(&workflows).unwrap();
        fs::write(
            workflows.join("release.yml"),
            "name: Release\non:\n  workflow_call:\n    inputs:\n      \
             dry-run:\n        type: boolean\n        default: true\n",
        )
        .unwrap();

        let mut log = Vec::new();
        let report = run(
            &[PathBuf::from(".github/workflows/release.yml")],
            &options(root.path()),
            &mut log,
        )
        .unwrap();
        assert!(report.is_clean(), "{report:?}");

        let doc = read_doc(root.path(), ".github/workflows/release.md");
        assert!(doc.contains("jobs:\n  release:"), "got {doc}");
        assert!(
            doc.contains("uses: <owner>/<repo>/.github/workflows/release.yml@<sha>  # <version>"),
            "got {doc}"
        );
        assert!(doc.contains("dry-run: true"), "got {doc}");
    }
}
