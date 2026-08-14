# actdocs-rs

Generates documentation for GitHub Actions and reusable workflows.

Reads an `action.yml` or a reusable workflow, renders its interface as Markdown
tables, and writes them into the regions of a document marked out by HTML
comments. Everything outside those markers is hand-written and survives every
regeneration.

## Usage

```sh
actdocs-rs sync \
  --docs-dir-target docs \
  --index-target README.md \
  .github/actions/*/action.yml .github/workflows/*.yml
```

Targets are never discovered. The caller decides what to document - in this
repository, the `files` pattern of the `actdocs` hook in `prek.toml`.

Every run writes the document beside each source. The other two outputs are
opt-in, so that a run only ever touches what it was pointed at:

| Flag | Meaning |
| :--- | :--- |
| `--docs-dir-target DIR` | Also mirror each document to `DIR/actions/<name>.md` or `DIR/workflows/<name>.md` |
| `--index-target FILE` | Rebuild the repository index in `FILE`, listing every action and workflow rather than only the targets given |
| `--check` | Report whether anything would change, and write nothing |
| `--root` | Repository root that generated paths resolve against |
| `--repo-slug` | `owner/repo` stamped into usage snippets (`ACTION_REPO_SLUG`) |
| `--ref-sha` | Commit SHA stamped into usage snippets (`ACTION_REF_SHA`) |
| `--ref-version` | Version stamped as a trailing comment (`ACTION_REF_VERSION`) |

`--index-target` names a document that must already exist, with the index
markers in it; unlike the per-source documents, an index is never scaffolded.

`actdocs-rs generate FILE [--format json]` prints a single file's documentation
to stdout. It exists for debugging; `sync` is the supported entry point.

### Exit codes

| Code | Meaning |
| :--- | :--- |
| 0 | Nothing to do, or files were rewritten successfully |
| 1 | `--check` found a difference, or a document is missing its markers |
| 2 | A file could not be read, parsed or written |

Rewriting a file is deliberately not an error. Hook runners detect modified
files themselves, and conflating the two would make `--check` useless in CI,
where "would change" is the signal worth having.

### Markers

| Pair | Contents |
| :--- | :--- |
| `<!-- actdocs start -->` / `<!-- actdocs end -->` | The generated tables |
| `<!-- usage start -->` / `<!-- usage end -->` | A copy-pasteable step, actions only |
| `<!-- index start -->` / `<!-- index end -->` | The repository index, with `--index-target` |

A document with no markers is reported rather than overwritten. A document that
does not exist yet is scaffolded once.

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
        args: ["--docs-dir-target", "docs", "--index-target", "README.md"]
```

```toml
# prek.toml
[[repos]]
repo = "https://github.com/FredrikBakken/actdocs-rs"
rev = "v0.1.0"
hooks = [
    { id = "actdocs", args = [
        "--docs-dir-target", "docs",
        "--index-target", "README.md",
    ] },
]
```

| Hook | Behaviour |
| :--- | :--- |
| `actdocs` | Rewrites documents in place. For local commits |
| `actdocs-check` | Fails if anything is out of date, and writes nothing. For CI |

Both accept the flags above through `args`, and both are built from source on
first use - no separate installation step, and nothing to keep in step with the
pinned `rev`.

Neither writes into a documentation tree nor touches an index unless asked to,
so `{ id = "actdocs" }` on its own updates only the document beside each
source. `--index-target` is the one flag that names a document which must
already exist, with the index markers already in it: unlike the per-source
documents, an index is never scaffolded.

By default the hooks match `.github/actions/*/action.yml` and
`.github/workflows/*.yml`, which is where GitHub requires those files to live.
Routing is by file name, so widening `files` beyond that will generate
documents beside any YAML it is handed.

Releases here are immutable, which means a published tag is locked to its
commit and can never be moved, deleted or reused. Pinning `rev` to a tag
therefore carries the same guarantee as pinning a commit SHA, without the
unreadability.

## Development

```sh
mise run check
```

## License

Apache-2.0. See [`LICENSE`](LICENSE) for the terms and [`NOTICE`](NOTICE) for
attribution.
