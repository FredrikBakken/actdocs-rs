//! The copy-pasteable usage snippet for an action.

use std::fmt::Write as _;

use crate::model::{ALL_SCOPES, ActionInput, ActionSpec, Secret, WorkflowInput, WorkflowSpec};

/// How a `uses:` reference is pinned.
///
/// Both forms can name the same commit; they differ in what a reader has to
/// trust. This is a policy the caller states, not something discovered: asking
/// GitHub which repositories have immutable releases would put a network call
/// in the middle of a generator whose whole point is reproducible output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pin {
    /// `@<sha>  # <version>`. Resolves to one commit whatever happens to the
    /// tag afterwards, with the version alongside so the line stays readable.
    #[default]
    Sha,
    /// `@<version>`. Shorter, and equivalent in integrity to a SHA only where
    /// the publishing repository has immutable releases enabled, which locks a
    /// release tag to its commit permanently and forbids reusing the name.
    Version,
}

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
    /// The commit SHA. Printed only under `Pin::Sha`.
    pub sha: &'a str,
    /// The human-readable version. Printed under either pin, as the reference
    /// itself or as a trailing comment.
    pub version: &'a str,
    /// Which of the two the snippet pins to.
    pub pin: Pin,
}

/// The `workflow_call` input types that are not strings. Everything else,
/// including an undeclared type, is quoted.
const BOOLEAN: &str = "boolean";
const NUMBER: &str = "number";

/// Render a fenced YAML step that calls the action.
///
/// Inputs are split into required and optional, because the two demand
/// different things of whoever pastes this: one set has to be filled in, the
/// other is only there to be discovered and overridden.
pub fn action(title: &str, spec: &ActionSpec, reference: Reference<'_>) -> String {
    let mut out = String::from("```yaml\n");
    let _ = writeln!(out, "- name: \"{title}\"");
    let _ = writeln!(out, "  uses: {}", called(reference));

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

/// Render a fenced YAML job that calls the reusable workflow.
///
/// A workflow is called as a job, not a step, and its blocks arrive in the
/// order a reader needs them: what is being called, what it is allowed to do,
/// what it is given, and what it can read back.
pub fn workflow(title: &str, spec: &WorkflowSpec, reference: Reference<'_>) -> String {
    let mut out = String::from("```yaml\njobs:\n");
    let _ = writeln!(out, "  {title}:");
    let _ = writeln!(out, "    uses: {}", called(reference));

    permissions_block(&mut out, spec);
    with_block(&mut out, spec);
    secrets_block(&mut out, spec);
    outputs_note(&mut out, title, spec);

    out.push_str("```");
    out
}

/// The `owner/repo/path@ref` half of a `uses:` line, pinned as asked.
fn called(reference: Reference<'_>) -> String {
    let Reference {
        repo_slug,
        path,
        sha,
        version,
        pin,
    } = reference;

    match pin {
        Pin::Sha => format!("{repo_slug}/{path}@{sha}  # {version}"),
        Pin::Version => format!("{repo_slug}/{path}@{version}"),
    }
}

/// The permissions a caller has to grant, which is the commonest reason a
/// reusable workflow fails at runtime rather than at parse time.
fn permissions_block(out: &mut String, spec: &WorkflowSpec) {
    let [first, rest @ ..] = spec.permissions.as_slice() else {
        return;
    };

    // The blanket forms record a sentinel scope, and `-: read-all` is not
    // something a caller can write.
    if rest.is_empty() && first.scope == ALL_SCOPES {
        let _ = writeln!(out, "    permissions: {}", first.access);
        return;
    }

    out.push_str("    permissions:\n");
    for permission in &spec.permissions {
        let _ = writeln!(out, "      {}: {}", permission.scope, permission.access);
    }
}

fn with_block(out: &mut String, spec: &WorkflowSpec) {
    let required = workflow_inputs(spec, true);
    let optional = workflow_inputs(spec, false);

    if required.is_empty() && optional.is_empty() {
        return;
    }

    out.push_str("    with:\n");
    if !required.is_empty() {
        out.push_str("      # Required\n");
        for input in required {
            let _ = writeln!(out, "      {}: {}", input.name, typed(input));
        }
    }
    if !optional.is_empty() {
        out.push_str("      # Optional, shown with their defaults\n");
        for input in optional {
            let _ = writeln!(out, "      {}: {}", input.name, typed(input));
        }
    }
}

fn secrets_block(out: &mut String, spec: &WorkflowSpec) {
    let required = secrets(spec, true);
    let optional = secrets(spec, false);

    if required.is_empty() && optional.is_empty() {
        return;
    }

    out.push_str("    secrets:\n");
    if !required.is_empty() {
        out.push_str("      # Required\n");
        for secret in required {
            out.push_str(&secret_line(secret));
        }
    }
    if !optional.is_empty() {
        out.push_str("      # Optional\n");
        for secret in optional {
            out.push_str(&secret_line(secret));
        }
    }
}

/// Where an output is read from, which is the one thing about a reusable
/// workflow that cannot be worked out by looking at the call.
fn outputs_note(out: &mut String, title: &str, spec: &WorkflowSpec) {
    if spec.outputs.is_empty() {
        return;
    }

    let names: Vec<&str> = spec
        .outputs
        .iter()
        .map(|output| output.name.as_str())
        .collect();
    let names = names.join(", ");

    let _ = writeln!(
        out,
        "# Outputs, read as needs.{title}.outputs.<name>: {names}"
    );
}

fn inputs(spec: &ActionSpec, required: bool) -> Vec<&ActionInput> {
    spec.inputs
        .iter()
        .filter(|input| input.required.is_true() == required)
        .collect()
}

fn workflow_inputs(spec: &WorkflowSpec, required: bool) -> Vec<&WorkflowInput> {
    spec.inputs
        .iter()
        .filter(|input| input.required.is_true() == required)
        .collect()
}

fn secrets(spec: &WorkflowSpec, required: bool) -> Vec<&Secret> {
    spec.secrets
        .iter()
        .filter(|secret| secret.required.is_true() == required)
        .collect()
}

/// A `with:` value written the way the declared type demands.
///
/// The action snippet quotes everything, because an action receives every
/// input as a string and an unquoted `72` would misdescribe that. A workflow
/// input is typed, so here the quoting is the lie: `retries: "3"` against
/// `type: number` says the wrong thing. Anything undeclared is quoted, because
/// a string is the reading that cannot be wrong about a value's shape.
///
/// An input with no default falls back to its type's zero. It is a placeholder
/// like the action's `""`, and the `# Required` heading is what says so.
fn typed(input: &WorkflowInput) -> String {
    let value = input.default.as_deref().unwrap_or_default();

    match input.r#type.as_deref() {
        Some(BOOLEAN) => zeroed(value, "false"),
        Some(NUMBER) => zeroed(value, "0"),
        _ => serde_json::to_string(value).expect("a string always serialises"),
    }
}

fn zeroed(value: &str, zero: &str) -> String {
    if value.is_empty() {
        zero.to_owned()
    } else {
        value.to_owned()
    }
}

/// One `secrets:` entry.
///
/// The value is the only thing in this snippet that is guessed. A caller names
/// its own secrets and this cannot know what it called them; upper snake case
/// of the declared name is the conventional spelling, wrong often enough to be
/// worth editing and right often enough to be worth writing.
fn secret_line(secret: &Secret) -> String {
    let name = &secret.name;
    let holder = placeholder(name);
    format!("      {name}: \"${{{{ secrets.{holder} }}}}\"\n")
}

fn placeholder(name: &str) -> String {
    name.replace(['-', '.', ' '], "_").to_uppercase()
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
    use crate::model::{Output, Permission};
    use crate::scalar::Scalar;

    fn reference() -> Reference<'static> {
        Reference {
            repo_slug: "<owner>/<repo>",
            path: ".github/actions/example",
            sha: "<sha>",
            version: "<version>",
            pin: Pin::Sha,
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
    fn a_sha_pin_is_what_a_caller_gets_by_default() {
        // The default is a security posture, not a formatting preference, so
        // it is asserted rather than left to whichever variant comes first.
        assert_eq!(Pin::default(), Pin::Sha);
    }

    #[test]
    fn an_action_without_inputs_has_no_with_block() {
        let spec = ActionSpec::default();

        assert_eq!(
            action("example", &spec, reference()),
            "```yaml\n\
             - name: \"example\"\n  \
             uses: <owner>/<repo>/.github/actions/example@<sha>  # <version>\n\
             ```"
        );
    }

    #[test]
    fn a_version_pin_drops_the_sha_and_its_comment() {
        let spec = ActionSpec::default();

        assert_eq!(
            action(
                "example",
                &spec,
                Reference {
                    pin: Pin::Version,
                    ..reference()
                }
            ),
            "```yaml\n\
             - name: \"example\"\n  \
             uses: <owner>/<repo>/.github/actions/example@<version>\n\
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
            action("example", &spec, reference()),
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

        assert!(action("example", &spec, reference()).contains("    thing: \"\"\n"));
    }

    #[test]
    fn a_numeric_default_keeps_its_quotes() {
        // Actions pass every input as a string, so an unquoted 72 would be a
        // lie about the type the action receives. A workflow input is typed,
        // and `a_typed_value_is_written_bare_where_the_action_form_would_quote_it`
        // asserts the opposite for that reason.
        let spec = ActionSpec {
            inputs: vec![input("max-length", Scalar::new("72"), false)],
            ..ActionSpec::default()
        };

        assert!(action("example", &spec, reference()).contains("    max-length: \"72\"\n"));
    }

    #[test]
    fn an_awkward_default_cannot_break_the_snippet() {
        let spec = ActionSpec {
            inputs: vec![input("tricky", Scalar::new("a \"b\": c\nd"), false)],
            ..ActionSpec::default()
        };

        assert!(action("example", &spec, reference()).contains(r#"    tricky: "a \"b\": c\nd""#));
    }

    fn workflow_reference() -> Reference<'static> {
        Reference {
            path: ".github/workflows/example.yml",
            ..reference()
        }
    }

    fn workflow_input(name: &str, kind: &str, default: Scalar, required: bool) -> WorkflowInput {
        WorkflowInput {
            name: name.to_owned(),
            description: Scalar::null(),
            default,
            required: Scalar::new(if required { "true" } else { "false" }),
            r#type: Scalar::new(kind),
        }
    }

    fn secret(name: &str, required: bool) -> Secret {
        Secret {
            name: name.to_owned(),
            description: Scalar::null(),
            required: Scalar::new(if required { "true" } else { "false" }),
        }
    }

    #[test]
    fn a_workflow_is_called_as_a_job_with_every_block() {
        let mut spec = WorkflowSpec {
            name: Scalar::new("Reusable"),
            inputs: vec![
                workflow_input("flag", "boolean", Scalar::new("false"), false),
                workflow_input("who", "string", Scalar::null(), true),
                workflow_input("retries", "number", Scalar::new("3"), false),
            ],
            secrets: vec![secret("token", false), secret("registry-password", true)],
            outputs: vec![Output {
                name: "digest".to_owned(),
                description: Scalar::null(),
            }],
            permissions: vec![Permission {
                scope: "contents".to_owned(),
                access: "read".to_owned(),
            }],
        };
        spec.sort();

        // Written a line at a time rather than as one escaped literal: at this
        // length the escaping is harder to review than the output.
        assert_eq!(
            workflow("build", &spec, workflow_reference()),
            [
                "```yaml",
                "jobs:",
                "  build:",
                "    uses: <owner>/<repo>/.github/workflows/example.yml@<sha>  # <version>",
                "    permissions:",
                "      contents: read",
                "    with:",
                "      # Required",
                "      who: \"\"",
                "      # Optional, shown with their defaults",
                "      flag: false",
                "      retries: 3",
                "    secrets:",
                "      # Required",
                "      registry-password: \"${{ secrets.REGISTRY_PASSWORD }}\"",
                "      # Optional",
                "      token: \"${{ secrets.TOKEN }}\"",
                "# Outputs, read as needs.build.outputs.<name>: digest",
                "```",
            ]
            .join("\n")
        );
    }

    #[test]
    fn a_workflow_that_declares_nothing_is_just_a_call() {
        assert_eq!(
            workflow("job", &WorkflowSpec::default(), workflow_reference()),
            [
                "```yaml",
                "jobs:",
                "  job:",
                "    uses: <owner>/<repo>/.github/workflows/example.yml@<sha>  # <version>",
                "```",
            ]
            .join("\n")
        );
    }

    #[test]
    fn a_typed_value_is_written_bare_where_the_action_form_would_quote_it() {
        let mut spec = WorkflowSpec {
            inputs: vec![
                workflow_input("flag", "boolean", Scalar::new("true"), false),
                workflow_input("retries", "number", Scalar::new("3"), false),
                workflow_input("label", "string", Scalar::new("3"), false),
            ],
            ..WorkflowSpec::default()
        };
        spec.sort();

        let rendered = workflow("job", &spec, workflow_reference());

        assert!(rendered.contains("\n      flag: true\n"), "got {rendered}");
        assert!(rendered.contains("\n      retries: 3\n"), "got {rendered}");
        // Same characters, different declared type, different rendering.
        assert!(
            rendered.contains("\n      label: \"3\"\n"),
            "got {rendered}"
        );
    }

    #[test]
    fn an_undeclared_type_is_quoted_because_a_string_cannot_be_wrong() {
        let spec = WorkflowSpec {
            inputs: vec![WorkflowInput {
                r#type: Scalar::null(),
                ..workflow_input("thing", "string", Scalar::new("7"), false)
            }],
            ..WorkflowSpec::default()
        };

        assert!(workflow("job", &spec, workflow_reference()).contains("\n      thing: \"7\"\n"));
    }

    #[test]
    fn a_required_input_gets_the_zero_of_its_type() {
        let mut spec = WorkflowSpec {
            inputs: vec![
                workflow_input("who", "string", Scalar::null(), true),
                workflow_input("force", "boolean", Scalar::null(), true),
                workflow_input("count", "number", Scalar::null(), true),
            ],
            ..WorkflowSpec::default()
        };
        spec.sort();

        let rendered = workflow("job", &spec, workflow_reference());

        assert!(rendered.contains("\n      who: \"\"\n"), "got {rendered}");
        assert!(
            rendered.contains("\n      force: false\n"),
            "got {rendered}"
        );
        assert!(rendered.contains("\n      count: 0\n"), "got {rendered}");
    }

    #[test]
    fn a_blanket_permission_is_written_as_a_scalar() {
        let spec = WorkflowSpec {
            permissions: vec![Permission {
                scope: ALL_SCOPES.to_owned(),
                access: "read-all".to_owned(),
            }],
            ..WorkflowSpec::default()
        };

        let rendered = workflow("job", &spec, workflow_reference());

        assert!(
            rendered.contains("\n    permissions: read-all\n"),
            "got {rendered}"
        );
        assert!(!rendered.contains("-:"), "the sentinel leaked: {rendered}");
    }

    #[test]
    fn a_version_pin_shortens_a_workflow_call_too() {
        let rendered = workflow(
            "job",
            &WorkflowSpec::default(),
            Reference {
                pin: Pin::Version,
                ..workflow_reference()
            },
        );

        assert!(rendered.contains("@<version>\n"), "got {rendered}");
        assert!(!rendered.contains("<sha>"), "got {rendered}");
    }
}
