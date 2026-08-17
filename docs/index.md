# actdocs-rs

`actdocs-rs` generates reference documentation for GitHub Actions and reusable
workflows from the manifests themselves, and writes it into marked regions of
Markdown documents that you own.

Everything outside the markers is hand-written and survives every
regeneration, so a generated document can carry as much prose as you like
around a table that is always current.

## What it produces

For an action, a `README.md` beside `action.yml` describing its inputs,
outputs and a copy-pasteable step. For a reusable workflow, the same treatment
plus a copy-pasteable *job*, since calling one means knowing about `secrets`
and `permissions` blocks that a step never needs.

## Quick start

```sh
actdocs sync .github/actions/*/action.yml .github/workflows/*.yml
```

That writes the document beside each source and nothing else. Publishing into
a documentation tree and rebuilding a repository index are both opt-in — see
[the command line reference](usage/cli.md).

## Where to go next

- [Command line](usage/cli.md) — every flag, and what it changes
- [Configuration](usage/configuration.md) — files, environment, precedence
- [Markers](usage/markers.md) — the contract with your documents
- [Use as a hook](usage/hooks.md) — running it on every commit
