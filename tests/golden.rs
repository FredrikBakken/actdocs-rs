//! End-to-end fixtures: YAML in, Markdown out.
//!
//! These are the tests that decide whether the port is faithful, so they assert
//! whole documents rather than fragments.

use actdocs_rs::parse::parse;
use actdocs_rs::render::table;

const ACTION: &str = "\
name: Valid Action
description: This is a test Custom Action for actdocs.

inputs:
  full-number:
    default: 5
    required: false
    description: \"The full number value.\"
  full-string:
    default: \"Default value\"
    required: true
    description: \"The full string value.\"
  full-boolean:
    default: true
    required: false
    description: \"The full boolean value.\"
  description-only:
    description: \"The description without default and required.\"
  empty:

outputs:
  with-description:
    description: \"The output value with description.\"
    value: ${{ inputs.description-only }}
  only-value:
    value: \"The output value without description.\"

runs:
  using: composite
";

const WORKFLOW: &str = "\
name: Lint YAML
on:
  workflow_call:
    inputs:
      full-number:
        default: 5
        required: false
        type: number
        description: \"The full number value.\"
      full-string:
        default: \"\"
        required: true
        type: string
        description: \"The full string value.\"
      full-boolean:
        default: true
        required: false
        type: boolean
        description: \"The full boolean value.\"
      default-and-type:
        default: \"foo\"
        type: string
      required-and-description:
        required: true
        description: \"The required and description value.\"
      empty:
    secrets:
      not-required-secret:
        description: \"The not required secret value.\"
        required: false
      required-secret:
        description: \"The required secret value.\"
        required: true
      alternative-required-secret:
        description: \"The alternative required secret value.\"
        required: true
      without-required-secret:
        description: \"The not required secret value.\"
      empty:
    outputs:
      with-description:
        value: \"foo\"
        description: \"The description value.\"
      only-value:
        value: \"bar\"

permissions:
  pull-requests: write
  contents: read

jobs:
  run:
    runs-on: ubuntu-latest
";

#[test]
fn renders_a_complete_action() {
    let expected = "\
## Description

This is a test Custom Action for actdocs.

## Inputs

| Name | Description | Default | Required |
| :--- | :---------- | :------ | :------: |
| full-string | The full string value. | `Default value` | yes |
| description-only | The description without default and required. | n/a | no |
| empty |  | n/a | no |
| full-boolean | The full boolean value. | `true` | no |
| full-number | The full number value. | `5` | no |

## Outputs

| Name | Description |
| :--- | :---------- |
| only-value |  |
| with-description | The output value with description. |";

    assert_eq!(table::document(&parse(ACTION).unwrap(), false), expected);
}

#[test]
fn renders_a_complete_workflow() {
    let expected = "\
## Inputs

| Name | Description | Type | Default | Required |
| :--- | :---------- | :--- | :------ | :------: |
| full-string | The full string value. | `string` | `` | yes |
| required-and-description | The required and description value. | n/a | n/a | yes |
| default-and-type |  | `string` | `foo` | no |
| empty |  | n/a | n/a | no |
| full-boolean | The full boolean value. | `boolean` | `true` | no |
| full-number | The full number value. | `number` | `5` | no |

## Secrets

| Name | Description | Required |
| :--- | :---------- | :------: |
| alternative-required-secret | The alternative required secret value. | yes |
| required-secret | The required secret value. | yes |
| empty |  | no |
| not-required-secret | The not required secret value. | no |
| without-required-secret | The not required secret value. | no |

## Outputs

| Name | Description |
| :--- | :---------- |
| only-value |  |
| with-description | The description value. |

## Permissions

| Scope | Access |
| :--- | :---- |
| contents | read |
| pull-requests | write |";

    assert_eq!(table::document(&parse(WORKFLOW).unwrap(), false), expected);
}

#[test]
fn rendering_is_stable_across_runs() {
    // Guards against map iteration order leaking into the output, which is the
    // failure mode that made the original tool's ordering nondeterministic.
    let first = table::document(&parse(WORKFLOW).unwrap(), false);
    for _ in 0..16 {
        assert_eq!(table::document(&parse(WORKFLOW).unwrap(), false), first);
    }
}

#[test]
fn omit_drops_the_sections_a_workflow_lacks() {
    let source = "on:\n  workflow_call:\n    inputs:\n      only:\n        type: string\n";
    let rendered = table::document(&parse(source).unwrap(), true);

    assert!(rendered.starts_with("## Inputs"));
    assert!(!rendered.contains("## Secrets"));
    assert!(!rendered.contains("## Permissions"));
    assert!(!rendered.contains("N/A"));
}

use actdocs_rs::render::usage::{self, Pin, Reference};

const VALIDATE_PR_TITLE: &str = "\
name: \"Validate PR Title\"
description: \"Validates that a pull-request title follows the Conventional Commits standard.\"

inputs:
  pr-title:
    description: \"The pull-request title to validate.\"
    required: true
  types:
    description: \"A comma-separated list of allowed types.\"
    required: false
    default: \"feat,fix,docs,style,refactor,test,chore\"
  max-length:
    description: \"The maximum length of the title.\"
    required: false
    default: 72

runs:
  using: \"composite\"
  steps:
    - name: \"Validate PR Title\"
      shell: bash
      run: \"${{ github.action_path }}/validate-pr-title.sh\"
";

#[test]
fn reproduces_the_committed_tables() {
    let expected = "\
## Description

Validates that a pull-request title follows the Conventional Commits standard.

## Inputs

| Name | Description | Default | Required |
| :--- | :---------- | :------ | :------: |
| pr-title | The pull-request title to validate. | n/a | yes |
| max-length | The maximum length of the title. | `72` | no |
| types | A comma-separated list of allowed types. | `feat,fix,docs,style,refactor,test,chore` | no |";

    let document = parse(VALIDATE_PR_TITLE).unwrap();
    assert_eq!(table::document(&document, true), expected);
}

#[test]
fn reproduces_the_committed_usage_snippet() {
    let expected = "\
```yaml
- name: \"validate-pr-title\"
  uses: <owner>/<repo>/.github/actions/validate-pr-title@<sha>  # <version>
  with:
    # Required
    pr-title: \"\"
    # Optional, shown with their defaults
    max-length: \"72\"
    types: \"feat,fix,docs,style,refactor,test,chore\"
```";

    let actdocs_rs::Document::Action(spec) = parse(VALIDATE_PR_TITLE).unwrap() else {
        panic!("expected an action");
    };

    assert_eq!(
        usage::action(
            "validate-pr-title",
            &spec,
            Reference {
                repo_slug: "<owner>/<repo>",
                path: ".github/actions/validate-pr-title",
                sha: "<sha>",
                version: "<version>",
                pin: Pin::Sha,
            },
        ),
        expected
    );
}
