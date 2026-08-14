//! The parsed shape of an action or a reusable workflow.
//!
//! These types are the boundary between parsing and rendering: everything the
//! renderers need is here, and nothing about YAML survives past this point.

use serde::Serialize;

use crate::scalar::Scalar;

/// A composite, Docker or JavaScript action, as declared by `action.yml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ActionSpec {
    pub name: Scalar,
    pub description: Scalar,
    pub inputs: Vec<ActionInput>,
    pub outputs: Vec<Output>,
}

impl ActionSpec {
    /// Order every section deterministically. See [`sort_entries`].
    pub fn sort(&mut self) {
        sort_entries(&mut self.inputs);
        sort_entries(&mut self.outputs);
    }
}

/// A reusable workflow, as declared by `on.workflow_call`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WorkflowSpec {
    /// The workflow's `name`.
    ///
    /// A workflow has no `description` key, so this is the only human-written
    /// summary available and it is what the repository index lists.
    pub name: Scalar,
    pub inputs: Vec<WorkflowInput>,
    pub secrets: Vec<Secret>,
    pub outputs: Vec<Output>,
    pub permissions: Vec<Permission>,
}

impl WorkflowSpec {
    /// Order every section deterministically. See [`sort_entries`].
    pub fn sort(&mut self) {
        sort_entries(&mut self.inputs);
        sort_entries(&mut self.secrets);
        sort_entries(&mut self.outputs);
        self.permissions.sort_by(|a, b| a.scope.cmp(&b.scope));
    }
}

/// An entry under an action's `inputs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionInput {
    pub name: String,
    pub description: Scalar,
    pub default: Scalar,
    pub required: Scalar,
}

/// An entry under a workflow's `on.workflow_call.inputs`.
///
/// Unlike an action input, this one is typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkflowInput {
    pub name: String,
    pub description: Scalar,
    pub default: Scalar,
    pub required: Scalar,
    pub r#type: Scalar,
}

/// An entry under a workflow's `on.workflow_call.secrets`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Secret {
    pub name: String,
    pub description: Scalar,
    pub required: Scalar,
}

/// An entry under `outputs`, for either an action or a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Output {
    pub name: String,
    pub description: Scalar,
}

/// One scope of a workflow's top-level `permissions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Permission {
    pub scope: String,
    pub access: String,
}

/// How an entry participates in section ordering.
trait Ordered {
    fn order_name(&self) -> &str;

    /// Whether the entry sorts into the required group. Sections whose entries
    /// have no notion of being required leave this alone.
    fn order_required(&self) -> bool {
        false
    }
}

impl Ordered for ActionInput {
    fn order_name(&self) -> &str {
        &self.name
    }

    fn order_required(&self) -> bool {
        self.required.is_true()
    }
}

impl Ordered for WorkflowInput {
    fn order_name(&self) -> &str {
        &self.name
    }

    fn order_required(&self) -> bool {
        self.required.is_true()
    }
}

impl Ordered for Secret {
    fn order_name(&self) -> &str {
        &self.name
    }

    fn order_required(&self) -> bool {
        self.required.is_true()
    }
}

impl Ordered for Output {
    fn order_name(&self) -> &str {
        &self.name
    }
}

/// Sort required entries first, then by name within each group.
///
/// Ordering is unconditional rather than a flag. Source order is a YAML mapping
/// and therefore an accident of authoring, and generated files that reorder
/// themselves between runs produce noisy diffs. Required-first puts the entries
/// a caller cannot omit at the top of the table, where they are read first.
fn sort_entries<T: Ordered>(entries: &mut [T]) {
    entries.sort_by(|a, b| {
        b.order_required()
            .cmp(&a.order_required())
            .then_with(|| a.order_name().cmp(b.order_name()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action_input(name: &str, required: bool) -> ActionInput {
        ActionInput {
            name: name.to_owned(),
            description: Scalar::null(),
            default: Scalar::null(),
            required: Scalar::new(if required { "true" } else { "false" }),
        }
    }

    fn output(name: &str) -> Output {
        Output {
            name: name.to_owned(),
            description: Scalar::null(),
        }
    }

    fn names<T: Ordered>(entries: &[T]) -> Vec<&str> {
        entries.iter().map(Ordered::order_name).collect()
    }

    #[test]
    fn inputs_sort_required_first_then_by_name() {
        let mut spec = ActionSpec {
            inputs: vec![
                action_input("full-number", false),
                action_input("required-and-description", true),
                action_input("empty", false),
                action_input("full-string", true),
                action_input("default-and-type", false),
                action_input("full-boolean", false),
            ],
            ..ActionSpec::default()
        };

        spec.sort();

        assert_eq!(
            names(&spec.inputs),
            [
                "full-string",
                "required-and-description",
                "default-and-type",
                "empty",
                "full-boolean",
                "full-number",
            ]
        );
    }

    #[test]
    fn an_absent_required_sorts_as_optional() {
        let mut spec = ActionSpec {
            inputs: vec![
                ActionInput {
                    required: Scalar::null(),
                    ..action_input("absent", false)
                },
                action_input("zzz-required", true),
            ],
            ..ActionSpec::default()
        };

        spec.sort();

        assert_eq!(names(&spec.inputs), ["zzz-required", "absent"]);
    }

    #[test]
    fn only_a_literal_true_sorts_as_required() {
        let mut spec = ActionSpec {
            inputs: vec![
                ActionInput {
                    required: Scalar::new("yes"),
                    ..action_input("zzz-yes", false)
                },
                action_input("aaa-true", true),
            ],
            ..ActionSpec::default()
        };

        spec.sort();

        assert_eq!(names(&spec.inputs), ["aaa-true", "zzz-yes"]);
    }

    #[test]
    fn outputs_sort_by_name_alone() {
        let mut spec = ActionSpec {
            outputs: vec![output("second"), output("first")],
            ..ActionSpec::default()
        };

        spec.sort();

        assert_eq!(names(&spec.outputs), ["first", "second"]);
    }

    #[test]
    fn sorting_is_idempotent() {
        let mut once = ActionSpec {
            inputs: vec![
                action_input("b", false),
                action_input("a", true),
                action_input("c", false),
            ],
            ..ActionSpec::default()
        };
        once.sort();

        let mut twice = once.clone();
        twice.sort();

        assert_eq!(once, twice);
    }

    #[test]
    fn workflow_sorts_every_section() {
        let mut spec = WorkflowSpec {
            name: Scalar::new("CI"),
            inputs: vec![
                WorkflowInput {
                    name: "optional".to_owned(),
                    description: Scalar::null(),
                    default: Scalar::null(),
                    required: Scalar::new("false"),
                    r#type: Scalar::new("string"),
                },
                WorkflowInput {
                    name: "needed".to_owned(),
                    description: Scalar::null(),
                    default: Scalar::null(),
                    required: Scalar::new("true"),
                    r#type: Scalar::new("boolean"),
                },
            ],
            secrets: vec![
                Secret {
                    name: "optional-secret".to_owned(),
                    description: Scalar::null(),
                    required: Scalar::new("false"),
                },
                Secret {
                    name: "needed-secret".to_owned(),
                    description: Scalar::null(),
                    required: Scalar::new("true"),
                },
            ],
            outputs: vec![output("second"), output("first")],
            permissions: vec![
                Permission {
                    scope: "pull-requests".to_owned(),
                    access: "write".to_owned(),
                },
                Permission {
                    scope: "contents".to_owned(),
                    access: "read".to_owned(),
                },
            ],
        };

        spec.sort();

        assert_eq!(names(&spec.inputs), ["needed", "optional"]);
        assert_eq!(names(&spec.secrets), ["needed-secret", "optional-secret"]);
        assert_eq!(names(&spec.outputs), ["first", "second"]);
        assert_eq!(
            spec.permissions
                .iter()
                .map(|permission| permission.scope.as_str())
                .collect::<Vec<_>>(),
            ["contents", "pull-requests"]
        );
    }

    #[test]
    fn serialises_absent_values_as_null() {
        let spec = ActionSpec {
            name: Scalar::new("Pre-commit"),
            description: Scalar::new("Run hooks."),
            inputs: vec![ActionInput {
                name: "all-files".to_owned(),
                description: Scalar::null(),
                default: Scalar::new("false"),
                required: Scalar::null(),
            }],
            outputs: Vec::new(),
        };

        let json = serde_json::to_string_pretty(&spec).unwrap();

        assert_eq!(
            json,
            r#"{
  "name": "Pre-commit",
  "description": "Run hooks.",
  "inputs": [
    {
      "name": "all-files",
      "description": null,
      "default": "false",
      "required": null
    }
  ],
  "outputs": []
}"#
        );
    }

    #[test]
    fn workflow_input_type_serialises_without_the_raw_prefix() {
        let input = WorkflowInput {
            name: "flag".to_owned(),
            description: Scalar::null(),
            default: Scalar::null(),
            required: Scalar::null(),
            r#type: Scalar::new("boolean"),
        };

        let json = serde_json::to_string(&input).unwrap();

        assert!(json.contains(r#""type":"boolean""#), "got {json}");
    }
}
