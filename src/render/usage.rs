//! The copy-pasteable usage snippet for an action.

use std::fmt::Write as _;

use crate::model::{ActionInput, ActionSpec};

/// How a published action is referenced from a workflow.
///
/// The SHA and version are supplied rather than discovered. Deriving them from
/// the local clone would make the snippet depend on who ran the generator and
/// how deep their checkout is, so committed documentation would churn between
/// a fork, a working copy and CI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference<'a> {
    /// The `owner/repo` the action is published from.
    pub repo_slug: &'a str,
    /// The action's directory within that repository.
    pub path: &'a str,
    /// The commit SHA to pin to.
    pub sha: &'a str,
    /// The human-readable version, shown as a trailing comment.
    pub version: &'a str,
}

/// Render a fenced YAML step that calls the action.
///
/// Inputs are split into required and optional, because the two demand
/// different things of whoever pastes this: one set has to be filled in, the
/// other is only there to be discovered and overridden.
pub fn snippet(title: &str, spec: &ActionSpec, reference: Reference<'_>) -> String {
    let mut out = String::from("```yaml\n");
    let _ = writeln!(out, "- name: \"{title}\"");
    let _ = writeln!(
        out,
        "  uses: {}/{}@{}  # {}",
        reference.repo_slug, reference.path, reference.sha, reference.version
    );

    // Already ordered required-first, then by name, so filtering preserves the
    // ordering within each group.
    let required = inputs(spec, true);
    let optional = inputs(spec, false);

    if !required.is_empty() || !optional.is_empty() {
        out.push_str("  with:\n");

        if !required.is_empty() {
            out.push_str("    # Required\n");
            for input in required {
                out.push_str(&with_line(input));
            }
        }
        if !optional.is_empty() {
            out.push_str("    # Optional, shown with their defaults\n");
            for input in optional {
                out.push_str(&with_line(input));
            }
        }
    }

    out.push_str("```");
    out
}

fn inputs(spec: &ActionSpec, required: bool) -> Vec<&ActionInput> {
    spec.inputs
        .iter()
        .filter(|input| input.required.is_true() == required)
        .collect()
}

/// One `with:` entry.
///
/// Values are quoted as JSON, which is also valid YAML, so a default containing
/// a quote, a colon or a line break cannot break the snippet it appears in.
fn with_line(input: &ActionInput) -> String {
    let value = input.default.as_deref().unwrap_or_default();
    let quoted = serde_json::to_string(value).expect("a string always serialises");
    format!("    {}: {quoted}\n", input.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scalar::Scalar;

    fn reference() -> Reference<'static> {
        Reference {
            repo_slug: "<owner>/<repo>",
            path: ".github/actions/example",
            sha: "<sha>",
            version: "<version>",
        }
    }

    fn input(name: &str, default: Scalar, required: bool) -> ActionInput {
        ActionInput {
            name: name.to_owned(),
            description: Scalar::null(),
            default,
            required: Scalar::new(if required { "true" } else { "false" }),
        }
    }

    #[test]
    fn an_action_without_inputs_has_no_with_block() {
        let spec = ActionSpec::default();

        assert_eq!(
            snippet("example", &spec, reference()),
            "```yaml\n\
             - name: \"example\"\n  \
             uses: <owner>/<repo>/.github/actions/example@<sha>  # <version>\n\
             ```"
        );
    }

    #[test]
    fn required_inputs_come_first_under_their_own_heading() {
        let mut spec = ActionSpec {
            inputs: vec![
                input("optional", Scalar::new("default"), false),
                input("needed", Scalar::null(), true),
            ],
            ..ActionSpec::default()
        };
        spec.sort();

        assert_eq!(
            snippet("example", &spec, reference()),
            "```yaml\n\
             - name: \"example\"\n  \
             uses: <owner>/<repo>/.github/actions/example@<sha>  # <version>\n  \
             with:\n    \
             # Required\n    \
             needed: \"\"\n    \
             # Optional, shown with their defaults\n    \
             optional: \"default\"\n\
             ```"
        );
    }

    #[test]
    fn an_input_with_no_default_is_shown_as_empty() {
        let spec = ActionSpec {
            inputs: vec![input("thing", Scalar::null(), true)],
            ..ActionSpec::default()
        };

        assert!(snippet("example", &spec, reference()).contains("    thing: \"\"\n"));
    }

    #[test]
    fn a_numeric_default_keeps_its_quotes() {
        // Actions pass every input as a string, so an unquoted 72 would be a
        // lie about the type the action receives.
        let spec = ActionSpec {
            inputs: vec![input("max-length", Scalar::new("72"), false)],
            ..ActionSpec::default()
        };

        assert!(snippet("example", &spec, reference()).contains("    max-length: \"72\"\n"));
    }

    #[test]
    fn an_awkward_default_cannot_break_the_snippet() {
        let spec = ActionSpec {
            inputs: vec![input("tricky", Scalar::new("a \"b\": c\nd"), false)],
            ..ActionSpec::default()
        };

        assert!(snippet("example", &spec, reference()).contains(r#"    tricky: "a \"b\": c\nd""#));
    }
}
