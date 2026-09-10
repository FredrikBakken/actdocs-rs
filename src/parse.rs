//! Turning YAML into a [`Document`], or a manifest into the hooks it declares.
//!
//! Nothing here knows about Markdown, and nothing downstream knows about YAML.

use anyhow::{Result, anyhow};
use saphyr::{LoadableYamlNode, Yaml};

use crate::model::{
    ALL_SCOPES, ActionInput, ActionSpec, Hook, Output, Permission, Secret, WorkflowInput,
    WorkflowSpec,
};
use crate::scalar::Scalar;

/// `permissions: read-all` and its counterpart apply to every scope at once.
const READ_ALL: &str = "read-all";
const WRITE_ALL: &str = "write-all";

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

/// Parse a `.pre-commit-hooks.yaml`.
///
/// The one source here that is a sequence rather than a mapping, which is why
/// nothing below is reused: [`entries`] reads named children, and these have no
/// names until you look inside them.
///
/// Unlike [`try_parse`], a document that is not a manifest is an error rather
/// than a shrug. Nothing hands this file over speculatively — it is read only
/// because a hooks table was asked for by name.
pub fn hooks(source: &str) -> Result<Vec<Hook>> {
    let documents =
        Yaml::load_from_str(source).map_err(|error| anyhow!("invalid YAML: {error}"))?;
    let Some(root) = documents.first() else {
        return Ok(Vec::new());
    };
    let Some(sequence) = root.as_sequence() else {
        return Err(anyhow!(
            "not a hooks manifest: the document is not a sequence of hooks"
        ));
    };

    Ok(sequence
        .iter()
        .filter_map(|entry| {
            Some(Hook {
                id: lookup(entry, "id")?.as_str()?.to_owned(),
                name: scalar(lookup(entry, "name")),
                description: scalar(lookup(entry, "description")),
            })
        })
        .collect())
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
        permissions: permissions(root),
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

/// Every scope a caller has to grant, across the whole file.
///
/// A job's `permissions` replaces the top-level block for that job rather than
/// adding to it, and a caller's own block is the ceiling every called job runs
/// under. Neither one alone is therefore the contract: the caller has to grant
/// the union, at the strongest access any job asks for.
///
/// Reading only the top-level block was a quieter kind of wrong than reading
/// none. It documented `contents: read` for a workflow whose publish job
/// cannot run without `contents: write`, and a caller who granted exactly what
/// was documented found out at release time.
fn permissions(root: &Yaml<'_>) -> Vec<Permission> {
    let jobs = lookup(root, "jobs")
        .and_then(Yaml::as_mapping)
        .into_iter()
        .flat_map(|jobs| jobs.iter().map(|(_, job)| job))
        .filter_map(|job| lookup(job, "permissions"));

    let mut merged: Vec<Permission> = Vec::new();
    for node in lookup(root, "permissions").into_iter().chain(jobs) {
        for permission in block(node) {
            grant(&mut merged, permission);
        }
    }

    // A blanket grant already covers every named scope, so listing both would
    // be noise at best and a contradiction at worst. It is kept alone.
    if let Some(blanket) = merged
        .iter()
        .filter(|permission| permission.scope == ALL_SCOPES)
        .max_by_key(|permission| rank(&permission.access))
    {
        return vec![blanket.clone()];
    }

    merged
}

/// Record one grant, keeping the strongest access already held for its scope.
fn grant(merged: &mut Vec<Permission>, permission: Permission) {
    let Some(held) = merged
        .iter_mut()
        .find(|held| held.scope == permission.scope)
    else {
        merged.push(permission);
        return;
    };

    if rank(&permission.access) > rank(&held.access) {
        held.access = permission.access;
    }
}

/// How much a grant allows, for choosing between two of them.
///
/// `none` is ranked rather than discarded: a job that states it is saying
/// something, and what it should do is lose to every other grant for the same
/// scope, which is what ranking it lowest achieves.
fn rank(access: &str) -> u8 {
    match access {
        "write" | WRITE_ALL => 2,
        "read" | READ_ALL => 1,
        _ => 0,
    }
}

/// The grants one `permissions` block makes, in whichever form it was written.
fn block(node: &Yaml<'_>) -> Vec<Permission> {
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
    fn a_job_scope_is_part_of_the_interface_too() {
        let spec = workflow_of(
            "on:\n  workflow_call:\njobs:\n  run:\n    permissions:\n      contents: write\n",
        );
        assert_eq!(
            spec.permissions,
            vec![Permission {
                scope: "contents".to_owned(),
                access: "write".to_owned()
            }]
        );
    }

    #[test]
    fn a_job_that_needs_more_than_the_top_level_says_so() {
        let spec = workflow_of(
            "on:\n  workflow_call:\npermissions:\n  contents: read\njobs:\n  plan:\n    permissions:\n      contents: read\n  publish:\n    permissions:\n      contents: write\n",
        );
        assert_eq!(
            spec.permissions,
            vec![Permission {
                scope: "contents".to_owned(),
                access: "write".to_owned()
            }]
        );
    }

    #[test]
    fn a_weaker_job_grant_does_not_lower_a_stronger_one() {
        let spec = workflow_of(
            "on:\n  workflow_call:\npermissions:\n  contents: write\njobs:\n  run:\n    permissions:\n      contents: none\n",
        );
        assert_eq!(spec.permissions[0].access, "write");
    }

    #[test]
    fn scopes_only_a_job_names_are_listed_beside_the_rest() {
        let spec = workflow_of(
            "on:\n  workflow_call:\npermissions:\n  contents: read\njobs:\n  run:\n    permissions:\n      id-token: write\n      packages: write\n",
        );
        assert_eq!(
            spec.permissions
                .iter()
                .map(|permission| permission.scope.as_str())
                .collect::<Vec<_>>(),
            ["contents", "id-token", "packages"]
        );
    }

    #[test]
    fn a_blanket_grant_swallows_the_scopes_it_already_covers() {
        let spec = workflow_of(
            "on:\n  workflow_call:\npermissions: write-all\njobs:\n  run:\n    permissions:\n      contents: read\n",
        );
        assert_eq!(
            spec.permissions,
            vec![Permission {
                scope: "-".to_owned(),
                access: "write-all".to_owned()
            }]
        );
    }

    #[test]
    fn a_workflow_granting_nothing_anywhere_has_no_permissions() {
        let spec =
            workflow_of("on:\n  workflow_call:\njobs:\n  run:\n    runs-on: ubuntu-latest\n");
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

    #[test]
    fn hooks_are_read_in_the_order_they_are_declared() {
        let manifest = hooks("- id: b\n  name: B\n- id: a\n  name: A\n").unwrap();

        assert_eq!(
            manifest
                .iter()
                .map(|hook| hook.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
    }

    #[test]
    fn a_folded_description_arrives_as_one_value() {
        let manifest = hooks("- id: x\n  description: >-\n    one\n    two\n").unwrap();

        assert_eq!(manifest[0].description, Scalar::new("one two"));
    }

    #[test]
    fn a_hook_without_an_id_is_skipped() {
        assert_eq!(hooks("- name: Nameless\n- id: x\n").unwrap().len(), 1);
    }

    #[test]
    fn the_optional_fields_stay_absent() {
        let manifest = hooks("- id: x\n").unwrap();

        assert_eq!(manifest[0].name, Scalar::null());
        assert_eq!(manifest[0].description, Scalar::null());
    }

    #[test]
    fn an_empty_manifest_declares_no_hooks() {
        assert!(hooks("").unwrap().is_empty());
    }

    #[test]
    fn a_manifest_that_is_not_a_sequence_is_rejected() {
        let error = hooks("id: x\n").unwrap_err().to_string();
        assert!(error.contains("not a hooks manifest"), "got {error}");
    }
}
