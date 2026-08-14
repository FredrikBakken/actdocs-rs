//! Turning YAML into a [`Document`].
//!
//! Nothing here knows about Markdown, and nothing downstream knows about YAML.

use anyhow::{Result, anyhow};
use saphyr::{LoadableYamlNode, Yaml};

use crate::model::{
    ActionInput, ActionSpec, Output, Permission, Secret, WorkflowInput, WorkflowSpec,
};
use crate::scalar::Scalar;

/// `permissions: read-all` and its counterpart apply to every scope at once.
const READ_ALL: &str = "read-all";
const WRITE_ALL: &str = "write-all";
/// The scope shown for the blanket forms, which name no scope of their own.
const ALL_SCOPES: &str = "-";

/// A parsed source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document {
    Action(ActionSpec),
    Workflow(WorkflowSpec),
}

/// Parse an `action.yml` or a reusable workflow, if that is what this is.
///
/// `Ok(None)` is an ordinary outcome, not a failure: the hook is handed every
/// workflow the commit touched, and an ordinary CI workflow simply has nothing
/// to document. Reserving `Err` for unreadable YAML is what lets the caller
/// skip the former silently while still reporting the latter.
///
/// The kind is decided structurally rather than by pattern-matching the raw
/// text: a top-level `runs:` makes it an action, and `on.workflow_call:` makes
/// it a reusable workflow. A file with both is an action, since that is the key
/// that actually determines how GitHub executes it.
pub fn try_parse(source: &str) -> Result<Option<Document>> {
    let documents =
        Yaml::load_from_str(source).map_err(|error| anyhow!("invalid YAML: {error}"))?;
    let Some(root) = documents.first() else {
        return Ok(None);
    };

    if lookup(root, "runs").is_some() {
        Ok(Some(Document::Action(action(root))))
    } else if workflow_call(root).is_some() {
        Ok(Some(Document::Workflow(workflow(root))))
    } else {
        Ok(None)
    }
}

/// Parse a file that is expected to be an action or a reusable workflow.
pub fn parse(source: &str) -> Result<Document> {
    try_parse(source)?.ok_or_else(|| {
        anyhow!(
            "not an action or a reusable workflow: no top-level `runs:` and no `on.workflow_call:`"
        )
    })
}

fn action(root: &Yaml<'_>) -> ActionSpec {
    let mut spec = ActionSpec {
        name: scalar(lookup(root, "name")),
        description: scalar(lookup(root, "description")),
        inputs: entries(lookup(root, "inputs"))
            .into_iter()
            .map(|(name, entry)| ActionInput {
                name,
                description: field(entry, "description"),
                default: field(entry, "default"),
                required: field(entry, "required"),
            })
            .collect(),
        outputs: outputs(lookup(root, "outputs")),
    };
    spec.sort();
    spec
}

fn workflow(root: &Yaml<'_>) -> WorkflowSpec {
    let call = workflow_call(root);
    let mut spec = WorkflowSpec {
        name: scalar(lookup(root, "name")),
        inputs: entries(call.and_then(|call| lookup(call, "inputs")))
            .into_iter()
            .map(|(name, entry)| WorkflowInput {
                name,
                description: field(entry, "description"),
                default: field(entry, "default"),
                required: field(entry, "required"),
                r#type: field(entry, "type"),
            })
            .collect(),
        secrets: entries(call.and_then(|call| lookup(call, "secrets")))
            .into_iter()
            .map(|(name, entry)| Secret {
                name,
                description: field(entry, "description"),
                required: field(entry, "required"),
            })
            .collect(),
        outputs: outputs(call.and_then(|call| lookup(call, "outputs"))),
        permissions: permissions(lookup(root, "permissions")),
    };
    spec.sort();
    spec
}

fn workflow_call<'a, 'input>(root: &'a Yaml<'input>) -> Option<&'a Yaml<'input>> {
    lookup(lookup(root, "on")?, "workflow_call")
}

fn outputs(node: Option<&Yaml<'_>>) -> Vec<Output> {
    entries(node)
        .into_iter()
        .map(|(name, entry)| Output {
            name,
            description: field(entry, "description"),
        })
        .collect()
}

/// Read the top-level `permissions`.
///
/// Job-level permissions are deliberately ignored: they govern one job rather
/// than the contract a caller has to satisfy, so they are not part of the
/// workflow's public interface.
fn permissions(node: Option<&Yaml<'_>>) -> Vec<Permission> {
    let Some(node) = node else {
        return Vec::new();
    };

    if let Some(access) = node.as_str() {
        return if access == READ_ALL || access == WRITE_ALL {
            vec![Permission {
                scope: ALL_SCOPES.to_owned(),
                access: access.to_owned(),
            }]
        } else {
            Vec::new()
        };
    }

    let Some(mapping) = node.as_mapping() else {
        return Vec::new();
    };

    mapping
        .iter()
        .filter_map(|(scope, access)| {
            // A scope with no access, such as a bare `contents:`, grants
            // nothing and has nothing to document.
            Some(Permission {
                scope: scope.as_str()?.to_owned(),
                access: access.as_str()?.to_owned(),
            })
        })
        .collect()
}

/// The named children of a mapping, in document order.
///
/// An entry may be null — `empty:` with nothing under it is valid YAML and
/// appears in the wild — which yields a name with no body rather than being
/// skipped, so the entry still shows up in the generated table.
fn entries<'a, 'input>(
    parent: Option<&'a Yaml<'input>>,
) -> Vec<(String, Option<&'a Yaml<'input>>)> {
    let Some(mapping) = parent.and_then(Yaml::as_mapping) else {
        return Vec::new();
    };

    mapping
        .iter()
        .filter_map(|(key, value)| {
            let name = key.as_str()?.to_owned();
            let body = if value.is_null() { None } else { Some(value) };
            Some((name, body))
        })
        .collect()
}

fn field(entry: Option<&Yaml<'_>>, key: &str) -> Scalar {
    scalar(entry.and_then(|entry| lookup(entry, key)))
}

/// Look up a key in a mapping.
///
/// `Yaml::as_mapping_get` would do, were it not for `on:`. YAML 1.1 resolves a
/// bare `on` to the boolean true, YAML 1.2 keeps it a string, and workflows are
/// written assuming whichever the reader does. Accepting both spellings means
/// the trigger is found either way instead of the workflow silently parsing as
/// having no inputs.
fn lookup<'a, 'input>(node: &'a Yaml<'input>, key: &str) -> Option<&'a Yaml<'input>> {
    let mapping = node.as_mapping()?;
    mapping.iter().find_map(|(candidate, value)| {
        let matched =
            candidate.as_str() == Some(key) || (key == "on" && candidate.as_bool() == Some(true));
        matched.then_some(value)
    })
}

/// Flatten a YAML scalar into the string the renderers work with.
///
/// Every field is documented as text regardless of how it was written, so
/// `default: 5` and `default: "5"` are indistinguishable downstream — which is
/// correct, because GitHub passes both to the action as the string `5`.
fn scalar(node: Option<&Yaml<'_>>) -> Scalar {
    let Some(node) = node else {
        return Scalar::null();
    };
    if node.is_null() {
        return Scalar::null();
    }
    if let Some(text) = node.as_str() {
        return Scalar::new(text);
    }
    if let Some(flag) = node.as_bool() {
        return Scalar::new(if flag { "true" } else { "false" });
    }
    if let Some(number) = node.as_integer() {
        return Scalar::new(number.to_string());
    }
    if let Some(number) = node.as_floating_point() {
        return Scalar::new(number.to_string());
    }
    // A sequence or mapping where a scalar belongs. Documenting it as absent
    // beats rendering a debug representation into the table.
    Scalar::null()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action_of(source: &str) -> ActionSpec {
        match parse(source).unwrap() {
            Document::Action(spec) => spec,
            Document::Workflow(_) => panic!("expected an action"),
        }
    }

    fn workflow_of(source: &str) -> WorkflowSpec {
        match parse(source).unwrap() {
            Document::Workflow(spec) => spec,
            Document::Action(_) => panic!("expected a workflow"),
        }
    }

    #[test]
    fn detects_an_action() {
        let spec = action_of("name: A\ndescription: B\nruns:\n  using: composite\n");
        assert_eq!(spec.name, Scalar::new("A"));
        assert_eq!(spec.description, Scalar::new("B"));
    }

    #[test]
    fn detects_a_reusable_workflow() {
        let spec = workflow_of("name: A\non:\n  workflow_call:\n    inputs:\n      x:\n");
        assert_eq!(spec.name, Scalar::new("A"));
        assert_eq!(spec.inputs.len(), 1);
    }

    #[test]
    fn an_action_wins_when_a_file_is_both() {
        let source = "on:\n  workflow_call:\n    inputs:\n      x:\nruns:\n  using: composite\n";
        assert!(matches!(parse(source).unwrap(), Document::Action(_)));
    }

    #[test]
    fn rejects_a_file_that_is_neither() {
        let error = parse("name: CI\non:\n  push:\n").unwrap_err().to_string();
        assert!(
            error.contains("not an action or a reusable workflow"),
            "got {error}"
        );
    }

    #[test]
    fn rejects_an_empty_document() {
        assert!(parse("").is_err());
    }

    #[test]
    fn rejects_malformed_yaml() {
        let error = parse("runs:\n  - [unterminated\n").unwrap_err().to_string();
        assert!(error.contains("invalid YAML"), "got {error}");
    }

    #[test]
    fn a_workflow_trigger_is_found_whichever_way_on_resolves() {
        // Quoted, so it is unambiguously the string "on".
        let quoted = workflow_of("\"on\":\n  workflow_call:\n    inputs:\n      x:\n");
        let bare = workflow_of("on:\n  workflow_call:\n    inputs:\n      x:\n");
        assert_eq!(quoted.inputs.len(), 1);
        assert_eq!(bare.inputs.len(), 1);
    }

    #[test]
    fn scalars_of_every_type_become_strings() {
        let spec = action_of(
            "runs:\n  using: composite\ninputs:\n  \
             number: { default: 5 }\n  \
             boolean: { default: true }\n  \
             text: { default: \"x\" }\n  \
             blank: { default: \"\" }\n",
        );
        let default = |name: &str| {
            spec.inputs
                .iter()
                .find(|input| input.name == name)
                .unwrap()
                .default
                .clone()
        };

        assert_eq!(default("number"), Scalar::new("5"));
        assert_eq!(default("boolean"), Scalar::new("true"));
        assert_eq!(default("text"), Scalar::new("x"));
        assert_eq!(default("blank"), Scalar::new(""));
    }

    #[test]
    fn a_float_default_loses_its_trailing_zero() {
        // A known and accepted divergence: the parser resolves the scalar
        // before we see it, so the source spelling `1.50` is not recoverable.
        let spec = action_of("runs:\n  using: composite\ninputs:\n  n: { default: 1.50 }\n");
        assert_eq!(spec.inputs[0].default, Scalar::new("1.5"));
    }

    #[test]
    fn an_explicit_null_is_absent_rather_than_the_text_null() {
        let spec = action_of("runs:\n  using: composite\ninputs:\n  n: { default: ~ }\n");
        assert_eq!(spec.inputs[0].default, Scalar::null());
    }

    #[test]
    fn an_entry_with_no_body_keeps_its_name() {
        let spec = action_of("runs:\n  using: composite\ninputs:\n  empty:\n");
        assert_eq!(spec.inputs[0].name, "empty");
        assert_eq!(spec.inputs[0].description, Scalar::null());
        assert_eq!(spec.inputs[0].default, Scalar::null());
        assert_eq!(spec.inputs[0].required, Scalar::null());
    }

    #[test]
    fn missing_sections_are_empty_rather_than_an_error() {
        let spec = action_of("runs:\n  using: composite\n");
        assert!(spec.inputs.is_empty());
        assert!(spec.outputs.is_empty());
        assert_eq!(spec.description, Scalar::null());
    }

    #[test]
    fn permissions_are_read_as_scope_and_access() {
        let spec = workflow_of(
            "on:\n  workflow_call:\npermissions:\n  pull-requests: write\n  contents: read\n",
        );
        assert_eq!(
            spec.permissions,
            vec![
                Permission {
                    scope: "contents".to_owned(),
                    access: "read".to_owned()
                },
                Permission {
                    scope: "pull-requests".to_owned(),
                    access: "write".to_owned()
                },
            ]
        );
    }

    #[test]
    fn a_blanket_permission_becomes_a_single_row() {
        let spec = workflow_of("on:\n  workflow_call:\npermissions: read-all\n");
        assert_eq!(
            spec.permissions,
            vec![Permission {
                scope: "-".to_owned(),
                access: "read-all".to_owned()
            }]
        );
    }

    #[test]
    fn an_unrecognised_permission_scalar_grants_nothing() {
        let spec = workflow_of("on:\n  workflow_call:\npermissions: nonsense\n");
        assert!(spec.permissions.is_empty());
    }

    #[test]
    fn a_scope_without_access_is_skipped_rather_than_fatal() {
        let spec =
            workflow_of("on:\n  workflow_call:\npermissions:\n  contents:\n  issues: write\n");
        assert_eq!(
            spec.permissions,
            vec![Permission {
                scope: "issues".to_owned(),
                access: "write".to_owned()
            }]
        );
    }

    #[test]
    fn job_level_permissions_are_not_part_of_the_interface() {
        let spec = workflow_of(
            "on:\n  workflow_call:\njobs:\n  run:\n    permissions:\n      contents: write\n",
        );
        assert!(spec.permissions.is_empty());
    }

    #[test]
    fn an_ordinary_workflow_is_skipped_rather_than_rejected() {
        assert_eq!(try_parse("name: CI\non:\n  push:\n").unwrap(), None);
    }

    #[test]
    fn unreadable_yaml_is_still_an_error() {
        assert!(try_parse("runs:\n  - [unterminated\n").is_err());
    }
}
