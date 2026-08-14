//! The binary's contract with a hook runner: exit codes and stderr.
//!
//! The library tests cover what gets written. These cover what the process
//! reports, which is what prek and CI actually branch on, and which no amount
//! of unit testing can confirm.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Cargo builds the binary before integration tests run and points this at it.
const BIN: &str = env!("CARGO_BIN_EXE_actdocs-rs");

const MANIFEST: &str = ".github/actions/pre-commit/action.yml";

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

fn repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join(".github/actions/pre-commit");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("action.yml"), ACTION).unwrap();
    fs::write(
        root.path().join("README.md"),
        "# Repo\n\n<!-- index start -->\n<!-- index end -->\n",
    )
    .unwrap();
    root
}

/// Every path is resolved against `--root`, so the tests do not depend on the
/// working directory the harness happens to run them in.
fn sync(root: &Path, extra: &[&str]) -> Output {
    Command::new(BIN)
        .arg("sync")
        .arg("--root")
        .arg(root)
        .args(extra)
        .arg(MANIFEST)
        .output()
        .expect("the binary should be runnable")
}

fn code(output: &Output) -> i32 {
    output
        .status
        .code()
        .expect("the process should not be signalled")
}

#[test]
fn writing_documents_succeeds() {
    let root = repository();
    let output = sync(root.path(), &[]);

    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        root.path()
            .join(".github/actions/pre-commit/README.md")
            .exists()
    );
}

#[test]
fn a_mirror_appears_only_when_asked_for() {
    let root = repository();

    sync(root.path(), &[]);
    assert!(!root.path().join("docs").exists());

    sync(root.path(), &["--docs-dir-target", "docs"]);
    assert!(root.path().join("docs/actions/pre-commit.md").exists());
}

#[test]
fn checking_an_unwritten_repository_reports_a_difference() {
    let root = repository();
    let output = sync(root.path(), &["--check"]);

    assert_eq!(code(&output), 1);
    assert!(
        !root
            .path()
            .join(".github/actions/pre-commit/README.md")
            .exists()
    );
}

#[test]
fn checking_is_clean_once_everything_is_written() {
    let root = repository();
    sync(root.path(), &[]);

    let output = sync(root.path(), &["--check"]);

    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_document_without_markers_fails_without_overwriting_it() {
    let root = repository();
    let readme = root.path().join(".github/actions/pre-commit/README.md");
    fs::write(&readme, "# Hand written\n").unwrap();

    let output = sync(root.path(), &[]);

    assert_eq!(code(&output), 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("README.md"));
    assert_eq!(fs::read_to_string(&readme).unwrap(), "# Hand written\n");
}

#[test]
fn an_unreadable_source_is_a_hard_failure() {
    let root = repository();
    let output = Command::new(BIN)
        .arg("sync")
        .arg("--root")
        .arg(root.path())
        .arg(".github/actions/absent/action.yml")
        .output()
        .unwrap();

    assert_eq!(code(&output), 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}

#[test]
fn unparseable_yaml_is_a_hard_failure() {
    let root = repository();
    fs::write(root.path().join(MANIFEST), "runs:\n  - [unterminated\n").unwrap();

    let output = sync(root.path(), &[]);

    assert_eq!(code(&output), 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid YAML"));
}
