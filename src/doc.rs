//! Reading, updating and writing a generated document.
//!
//! A generated document is mostly hand-written. Only the regions between marker
//! pairs are ours; everything else — the title, the prose, whatever a
//! maintainer added — is left exactly as found. That is the whole contract, and
//! it is why documents are edited in place rather than regenerated wholesale.

use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result};

/// A pair of HTML comments delimiting a region that is rewritten every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Markers {
    pub start: &'static str,
    pub end: &'static str,
}

/// The generated tables.
pub const ACTDOCS: Markers = Markers {
    start: "<!-- actdocs start -->",
    end: "<!-- actdocs end -->",
};

/// The copy-pasteable usage snippet.
pub const USAGE: Markers = Markers {
    start: "<!-- usage start -->",
    end: "<!-- usage end -->",
};

/// The repository index.
pub const INDEX: Markers = Markers {
    start: "<!-- index start -->",
    end: "<!-- index end -->",
};

/// A document that cannot be updated, because its markers are missing or broken.
///
/// This is deliberately not lumped in with IO and parse failures: it means a
/// document needs a human to add the markers, not that anything went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerError {
    /// There is no start marker, so there is nowhere to write.
    Absent(Markers),
    /// The start marker is never closed, so the extent of the region is unknown.
    Unterminated(Markers),
}

impl fmt::Display for MarkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent(markers) => write!(
                formatter,
                "no `{}` / `{}` pair, so there is nowhere to write",
                markers.start, markers.end
            ),
            Self::Unterminated(markers) => write!(
                formatter,
                "`{}` is never closed by `{}`",
                markers.start, markers.end
            ),
        }
    }
}

impl Error for MarkerError {}

/// Whether updating a document actually changed anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// The file already held exactly these bytes.
    Unchanged,
    /// The file was created or rewritten.
    Written,
    /// The file differs, but `--check` meant nothing was written.
    WouldChange,
}

/// Replace the region between a marker pair, leaving the markers in place.
///
/// The body is surrounded by blank lines so that generated Markdown is not
/// glued to the HTML comments, which some renderers would otherwise treat as
/// part of the same block. An empty body collapses the region entirely rather
/// than leaving two stray blank lines behind.
pub fn replace(text: &str, markers: Markers, body: &str) -> Result<String, MarkerError> {
    enum State {
        Before,
        Inside,
        After,
    }

    let mut out = String::with_capacity(text.len() + body.len());
    let mut state = State::Before;

    for line in text.lines() {
        match state {
            State::Before => {
                push_line(&mut out, line);
                // Compared after trimming so an indented marker still works,
                // but the line is emitted as written to preserve the layout.
                if line.trim() == markers.start {
                    state = State::Inside;
                    push_body(&mut out, body);
                }
            }
            // Everything between the markers is ours, so it is simply dropped.
            State::Inside => {
                if line.trim() == markers.end {
                    push_line(&mut out, line);
                    state = State::After;
                }
            }
            State::After => push_line(&mut out, line),
        }
    }

    match state {
        State::Before => Err(MarkerError::Absent(markers)),
        State::Inside => Err(MarkerError::Unterminated(markers)),
        State::After => Ok(out),
    }
}

/// Whether a document has a usable marker pair.
pub fn has_markers(text: &str, markers: Markers) -> bool {
    let mut seen_start = false;
    for line in text.lines() {
        let line = line.trim();
        if !seen_start && line == markers.start {
            seen_start = true;
        } else if seen_start && line == markers.end {
            return true;
        }
    }
    false
}

/// The initial contents of a document that does not exist yet.
///
/// Written exactly once. Everything here outside the markers is a starting
/// point for a human to edit, and regeneration will never touch it again.
pub fn scaffold(title: &str, source: Option<SourceLink<'_>>, with_usage: bool) -> String {
    let mut out = format!("# {title}\n\n");

    if let Some(link) = source {
        // `writeln!` supplies the second newline, which separates the link from
        // whatever the maintainer writes underneath it.
        let _ = writeln!(out, "Generated from [`{}`]({}).\n", link.target, link.href);
    }

    if with_usage {
        out.push_str("## Usage\n\n");
        push_line(&mut out, USAGE.start);
        push_line(&mut out, USAGE.end);
        out.push('\n');
    }

    push_line(&mut out, ACTDOCS.start);
    push_line(&mut out, ACTDOCS.end);
    out
}

/// A link from a generated document back to the file it was generated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLink<'a> {
    /// The source path, as displayed.
    pub target: &'a str,
    /// The href, relative to the document.
    pub href: &'a str,
}

/// Write a document, but only when the bytes actually differ.
///
/// Rewriting an unchanged file would update its mtime and make hook runners
/// report a modification on every run, so the comparison is what keeps repeated
/// runs quiet and makes `--check` a straightforward read-only path.
pub fn write_if_changed(path: &Path, contents: &str, check: bool) -> Result<Update> {
    let current = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read {}", path.display()));
        }
    };

    if current.as_deref() == Some(contents) {
        return Ok(Update::Unchanged);
    }
    if check {
        return Ok(Update::WouldChange);
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }
    fs::write(path, contents).with_context(|| format!("cannot write {}", path.display()))?;

    Ok(Update::Written)
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn push_body(out: &mut String, body: &str) {
    if body.is_empty() {
        return;
    }
    out.push('\n');
    out.push_str(body);
    out.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = "\
# Title

Hand-written prose.

<!-- actdocs start -->
<!-- actdocs end -->

A closing note.
";

    #[test]
    fn a_body_is_surrounded_by_blank_lines() {
        assert_eq!(
            replace(DOCUMENT, ACTDOCS, "## Inputs\n\nN/A").unwrap(),
            "\
# Title

Hand-written prose.

<!-- actdocs start -->

## Inputs

N/A

<!-- actdocs end -->

A closing note.
"
        );
    }

    #[test]
    fn an_empty_body_collapses_the_region() {
        assert_eq!(replace(DOCUMENT, ACTDOCS, "").unwrap(), DOCUMENT);
    }

    #[test]
    fn previous_output_is_replaced_rather_than_appended() {
        let once = replace(DOCUMENT, ACTDOCS, "first").unwrap();
        let twice = replace(&once, ACTDOCS, "second").unwrap();

        assert!(twice.contains("second"));
        assert!(!twice.contains("first"));
    }

    #[test]
    fn replacing_is_idempotent() {
        let once = replace(DOCUMENT, ACTDOCS, "## Inputs\n\nN/A").unwrap();
        let twice = replace(&once, ACTDOCS, "## Inputs\n\nN/A").unwrap();

        assert_eq!(once, twice);
    }

    #[test]
    fn text_outside_the_markers_is_untouched() {
        let updated = replace(DOCUMENT, ACTDOCS, "body").unwrap();

        assert!(updated.starts_with("# Title\n\nHand-written prose.\n"));
        assert!(updated.ends_with("\nA closing note.\n"));
    }

    #[test]
    fn only_the_named_region_is_replaced() {
        let document = "\
<!-- usage start -->
old usage
<!-- usage end -->

<!-- actdocs start -->
old tables
<!-- actdocs end -->
";
        let updated = replace(document, USAGE, "new usage").unwrap();

        assert!(updated.contains("new usage"));
        assert!(updated.contains("old tables"));
    }

    #[test]
    fn an_indented_marker_still_matches_and_keeps_its_indentation() {
        let document = "  <!-- actdocs start -->\n  <!-- actdocs end -->\n";
        let updated = replace(document, ACTDOCS, "body").unwrap();

        assert_eq!(
            updated,
            "  <!-- actdocs start -->\n\nbody\n\n  <!-- actdocs end -->\n"
        );
    }

    #[test]
    fn a_missing_marker_pair_is_reported_rather_than_guessed_at() {
        assert_eq!(
            replace("# Title\n", ACTDOCS, "body"),
            Err(MarkerError::Absent(ACTDOCS))
        );
    }

    #[test]
    fn an_unterminated_marker_is_reported_rather_than_truncating_the_file() {
        assert_eq!(
            replace("<!-- actdocs start -->\nrest\n", ACTDOCS, "body"),
            Err(MarkerError::Unterminated(ACTDOCS))
        );
    }

    #[test]
    fn marker_errors_name_both_markers() {
        let message = MarkerError::Absent(ACTDOCS).to_string();
        assert!(message.contains("<!-- actdocs start -->"), "got {message}");
        assert!(message.contains("<!-- actdocs end -->"), "got {message}");
    }

    #[test]
    fn markers_are_detected_only_as_a_pair() {
        assert!(has_markers(DOCUMENT, ACTDOCS));
        assert!(!has_markers("<!-- actdocs start -->\n", ACTDOCS));
        assert!(!has_markers("# Title\n", ACTDOCS));
    }

    #[test]
    fn an_action_is_scaffolded_with_both_regions() {
        assert_eq!(
            scaffold(
                "pre-commit",
                Some(SourceLink {
                    target: ".github/actions/pre-commit/action.yml",
                    href: "../../.github/actions/pre-commit/action.yml",
                }),
                true,
            ),
            "\
# pre-commit

Generated from [`.github/actions/pre-commit/action.yml`](../../.github/actions/pre-commit/action.yml).

## Usage

<!-- usage start -->
<!-- usage end -->

<!-- actdocs start -->
<!-- actdocs end -->
"
        );
    }

    #[test]
    fn a_scaffold_without_usage_has_only_the_generated_region() {
        let scaffolded = scaffold("ci", None, false);

        assert_eq!(
            scaffolded,
            "# ci\n\n<!-- actdocs start -->\n<!-- actdocs end -->\n"
        );
        assert!(!has_markers(&scaffolded, USAGE));
    }

    #[test]
    fn a_scaffold_is_immediately_writable() {
        let scaffolded = scaffold("x", None, true);

        assert!(replace(&scaffolded, ACTDOCS, "tables").is_ok());
        assert!(replace(&scaffolded, USAGE, "snippet").is_ok());
    }

    #[test]
    fn writing_creates_a_missing_file_and_its_parents() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("docs/actions/x.md");

        assert_eq!(
            write_if_changed(&path, "body", false).unwrap(),
            Update::Written
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "body");
    }

    #[test]
    fn writing_identical_bytes_leaves_the_file_alone() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("x.md");
        fs::write(&path, "body").unwrap();

        assert_eq!(
            write_if_changed(&path, "body", false).unwrap(),
            Update::Unchanged
        );
    }

    #[test]
    fn checking_reports_a_difference_without_writing() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("x.md");
        fs::write(&path, "old").unwrap();

        assert_eq!(
            write_if_changed(&path, "new", true).unwrap(),
            Update::WouldChange
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
    }

    #[test]
    fn checking_a_missing_file_reports_a_difference() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("absent.md");

        assert_eq!(
            write_if_changed(&path, "body", true).unwrap(),
            Update::WouldChange
        );
        assert!(!path.exists());
    }
}
