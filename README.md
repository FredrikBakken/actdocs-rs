# actdocs-rs

Generates documentation for GitHub Actions and reusable workflows.

Reads an `action.yml` or a reusable workflow, renders its interface as Markdown
tables, and writes them into the regions of a document marked out by HTML
comments. Everything outside those markers is hand-written and survives every
regeneration.

📖 **[Documentation](https://fredrikbakken.github.io/actdocs-rs/)**

## Use as a hook

This repository is itself a hook repository, so a project consumes it by
reference rather than installing anything:

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/FredrikBakken/actdocs-rs
    rev: v0.1.0
    hooks:
      - id: actdocs
```

| Hook | Behaviour |
| :--- | :--- |
| `actdocs` | Rewrites documents in place. For local commits |
| `actdocs-check` | Fails if anything is out of date, and writes nothing. For CI |

Both are built from source on first use, and both accept every flag through
`args`. On its own, `actdocs` updates only the document beside each source;
mirroring into a documentation tree and rebuilding a repository index are
opt-in. See [Use as a hook] for the rest.

Releases here are immutable, so a published tag is locked to its commit and can
never be moved, deleted or reused. Pinning `rev` to a tag therefore carries the
same guarantee as pinning a commit SHA, without the unreadability.

## Use directly

```sh
actdocs-rs sync .github/actions/*/action.yml .github/workflows/*.yml
```

Targets are never discovered — the caller decides what to document. See the
[command line reference] for every flag, and [configuration] for stating them
in a file instead.

## Development

```sh
mise run check      # fmt, clippy and tests, as CI runs them
mise run docs:serve # preview the documentation site
```

## License

Apache-2.0. See [`LICENSE`](LICENSE) for the terms and [`NOTICE`](NOTICE) for
attribution.
