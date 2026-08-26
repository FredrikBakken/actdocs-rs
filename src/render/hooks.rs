//! The hooks a repository publishes to a hook runner.
//!
//! Adjacent to the index, and deliberately not part of it: the index answers
//! what a workflow can call, this answers what pre-commit and prek can install.
//! Different readers, different places on the page, and a repository may want
//! one and not the other.

use crate::model::Hook;
use crate::scalar::escape_pipes;

const HEADER: &str = "| Hook | Name | Description |";
const ALIGNMENT: &str = "| :--- | :--- | :---------- |";

/// Render the hooks table.
///
/// No heading of its own, like the usage snippet and unlike the index: the
/// region is placed by hand, under whatever the document already calls this.
///
/// A manifest declaring nothing renders nothing, which collapses the region
/// rather than leaving a bare header behind.
pub fn hooks(hooks: &[Hook]) -> String {
    if hooks.is_empty() {
        return String::new();
    }

    let mut out = format!("{HEADER}\n{ALIGNMENT}");
    for hook in hooks {
        out.push('\n');
        out.push_str(&row(hook));
    }
    out
}

/// The id is a code span: it is copied verbatim into a consumer's own
/// configuration, where the other two columns are only read.
///
/// Both descriptions are summarised rather than folded into a `<pre>` block.
/// A hook description is conventionally a folded scalar, so it arrives with
/// line breaks that mean nothing, and a table this wide is skimmed.
fn row(hook: &Hook) -> String {
    format!(
        "| `{}` | {} | {} |",
        escape_pipes(&hook.id),
        hook.name.cell_summary(),
        hook.description.cell_summary()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scalar::Scalar;

    fn hook(id: &str) -> Hook {
        Hook {
            id: id.to_owned(),
            name: Scalar::new("Generate documentation"),
            description: Scalar::new("Rewrites the generated regions."),
        }
    }

    #[test]
    fn a_hook_is_rendered_as_id_name_and_description() {
        assert_eq!(
            hooks(&[hook("actdocs")]),
            "\
| Hook | Name | Description |
| :--- | :--- | :---------- |
| `actdocs` | Generate documentation | Rewrites the generated regions. |"
        );
    }

    #[test]
    fn a_manifest_declaring_nothing_renders_nothing() {
        assert!(hooks(&[]).is_empty());
    }

    #[test]
    fn declaration_order_is_preserved() {
        let table = hooks(&[hook("second"), hook("first")]);
        let first = table.find("second").unwrap();
        let second = table.find("first").unwrap();

        assert!(first < second, "got {table}");
    }

    #[test]
    fn a_multiline_description_stays_on_its_row() {
        let folded = Hook {
            description: Scalar::new("one\ntwo"),
            ..hook("x")
        };

        assert!(hooks(&[folded]).ends_with("| one two |"));
    }

    #[test]
    fn a_pipe_cannot_break_the_table() {
        let piped = Hook {
            name: Scalar::new("a | b"),
            ..hook("x")
        };

        assert!(hooks(&[piped]).contains("| a \\| b |"));
    }
}
