---
name: actdocs-house-style
description: The writing and testing conventions of the actdocs-rs crate. Use when adding or editing Rust code, comments, tests or documentation in this repository, so that new code reads like the code already there.
---

# actdocs-rs house style

This crate is written deliberately. Most review feedback here is about prose,
not logic. Follow these rules before writing any Rust or Markdown.

## Comments explain why

A comment states the decision and what the alternative would have cost. It
never restates the code.

```rust
// GitHub only honours a literal `true`, so `yes`, `True` and `1` are all
// deliberately not required. Matching loosely here would document an input
// as required that Actions would happily accept without.
```

Bad: `// returns true if the value is "true"`.

Doc comments on public items follow the same rule. A one-line summary of what
the item is, then, where a choice was made, a paragraph on why that choice and
not the obvious other one.

## Prose

- Wrap at roughly 79 columns, in comments, doc comments and Markdown alike.
- British spelling: `behaviour`, `recognised`, `summarised`, `serialises`.
- Terse and unmarketed. No "simply", "just", "powerful", "seamless".
- Backticks for paths, flags, keys and identifiers.

## Tests

Test names are full sentences in snake_case describing the behaviour, not the
function under test.

```rust
fn a_missing_document_is_scaffolded_and_filled_in()
fn an_unrecognised_permission_scalar_grants_nothing()
fn only_a_literal_true_counts_as_true()
```

Never `fn test_parse_hooks()` or `fn hooks_works()`.

Tests live in a `#[cfg(test)] mod tests` at the foot of the file they cover,
with small fixture helpers above them (`fn action() -> Entry`, `fn options(root)
-> Options`). Assert on whole rendered strings with `assert_eq!` and a `"\`
continuation literal where the expected value is multi-line; use
`assert!(x.contains(..), "got {x}")` when only part matters.

## Types

- Absent and present-but-empty are different values. Use `Scalar`, not
  `String` or `Option<String>`, for anything read out of source YAML, and let
  `scalar.rs` own every rule for rendering one into Markdown.
- Rendering is centralised: reach for `cell_summary`, `cell_code`, `cell_text`,
  `section` and `escape_pipes` rather than formatting a cell by hand. A literal
  `|` corrupts a whole table, so it is escaped in exactly one place.
- Keep `#[derive(Debug, Clone, PartialEq, Eq)]` on model and plan types.
  `Target` holds a `Vec<Plan>` and the derives are load-bearing; dropping one
  from `Plan` breaks `Target` with four unhelpful errors.

## Errors and exit codes

Three outcomes, and they are not interchangeable:

| Outcome | How | Exit |
| :--- | :--- | :--- |
| Nothing to document | `Ok(None)` | 0 |
| Markers missing or unterminated | `report.unwritable` | 1 |
| Unreadable, unparseable or unwritable file | `Err` with `.with_context(...)` | 2 |

A document handed over speculatively by a hook runner that has nothing to
document is `Ok(None)`. A document asked for by name that is not there is an
`Err`. Rewriting a file is never an error.

## Validation

Run all three, in this order, and do not claim they passed without seeing it:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

`mise run check` runs the same set as CI. `mise` warns `missing: uv@0.12.2`;
that is the docs toolchain and is harmless here.

Lints are strict: `unsafe_code = "forbid"`, clippy `all` denied and `pedantic`
warned. Fix a pedantic warning rather than allowing it, unless the allow is
already established in `Cargo.toml`.

## Scope

Do exactly what was asked. Do not rename, restructure or "tidy" anything
adjacent. Do not fix unrelated failing tests — mention them instead.
