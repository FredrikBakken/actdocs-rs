//! The repository index: one row per documented action and reusable workflow.
//!
//! Every other renderer describes a single file. This one describes the
//! repository, so it is rebuilt from a full listing on every run rather than
//! from whatever the hook happened to pass in — an index that only knew about
//! the files in the current commit would lose a row the moment one was not
//! touched.

use crate::scalar::{Scalar, escape_pipes};

const ACTIONS_TITLE: &str = "## Available actions";
const ACTIONS_HEADER: &str = "| Action | Description |";
const WORKFLOWS_TITLE: &str = "## Reusable workflows";
const WORKFLOWS_HEADER: &str = "| Workflow | Description |";
const ALIGNMENT: &str = "| :--- | :--- |";

/// One row of the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Shown in the first column, as a code span.
    pub label: String,
    /// Where that code span links, relative to the index document.
    pub href: String,
    /// The second column. An action supplies its `description`; a workflow has
    /// no such key, so it supplies its `name`.
    pub summary: Scalar,
}

/// Render the index.
///
/// A section with no entries is left out entirely rather than emitted as a bare
/// table header, so a repository with no reusable workflows says nothing about
/// them instead of advertising an empty table.
pub fn index(actions: &[Entry], workflows: &[Entry]) -> String {
    [
        section(ACTIONS_TITLE, ACTIONS_HEADER, actions),
        section(WORKFLOWS_TITLE, WORKFLOWS_HEADER, workflows),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn section(title: &str, header: &str, entries: &[Entry]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }

    let mut out = format!("{title}\n\n{header}\n{ALIGNMENT}");
    for entry in entries {
        out.push('\n');
        out.push_str(&row(entry));
    }
    Some(out)
}

/// The label is escaped but the href is not: a pipe in a link destination
/// cannot be escaped without changing where the link points, and a path
/// containing one is pathological in a way this tool should not paper over.
fn row(entry: &Entry) -> String {
    format!(
        "| [`{}`]({}) | {} |",
        escape_pipes(&entry.label),
        entry.href,
        entry.summary.cell_summary()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action() -> Entry {
        Entry {
            label: "pre-commit".to_owned(),
            href: ".github/actions/pre-commit/README.md".to_owned(),
            summary: Scalar::new(
                "Run pre-commit hooks, preferring prek over pre-commit when both are available.",
            ),
        }
    }

    fn workflow() -> Entry {
        Entry {
            label: "lint".to_owned(),
            href: "docs/workflows/lint.md".to_owned(),
            summary: Scalar::new("Lint YAML"),
        }
    }

    #[test]
    fn an_action_links_to_the_readme_beside_it() {
        assert_eq!(
            index(&[action()], &[]),
            "\
## Available actions

| Action | Description |
| :--- | :--- |
| [`pre-commit`](.github/actions/pre-commit/README.md) | Run pre-commit hooks, preferring prek over pre-commit when both are available. |"
        );
    }

    #[test]
    fn sections_are_separated_by_one_blank_line() {
        assert_eq!(
            index(&[action()], &[workflow()]),
            "\
## Available actions

| Action | Description |
| :--- | :--- |
| [`pre-commit`](.github/actions/pre-commit/README.md) | Run pre-commit hooks, preferring prek over pre-commit when both are available. |

## Reusable workflows

| Workflow | Description |
| :--- | :--- |
| [`lint`](docs/workflows/lint.md) | Lint YAML |"
        );
    }

    #[test]
    fn a_section_with_no_entries_is_left_out() {
        let rendered = index(&[action()], &[]);
        assert!(!rendered.contains("Reusable workflows"), "got {rendered}");
    }

    #[test]
    fn an_index_of_nothing_is_empty() {
        assert_eq!(index(&[], &[]), "");
    }

    #[test]
    fn a_multiline_summary_stays_on_its_row() {
        let entry = Entry {
            summary: Scalar::new("Runs\nhooks."),
            ..action()
        };

        assert!(index(&[entry], &[]).ends_with("| Runs hooks. |"));
    }

    #[test]
    fn an_absent_summary_leaves_the_cell_empty() {
        let entry = Entry {
            summary: Scalar::null(),
            ..action()
        };

        assert!(index(&[entry], &[]).ends_with("|  |"));
    }
}
