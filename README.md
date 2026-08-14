# actdocs-rs

Generates documentation for GitHub Actions and reusable workflows.

Reads an `action.yml` or a reusable workflow, renders its interface as Markdown
tables, and writes them into the regions of a document marked out by HTML
comments. Everything outside those markers is hand-written and survives every
regeneration.

## Usage

```sh
actdocs-rs sync .github/actions/*/action.yml .github/workflows/*.yml
```

Targets are never discovered. The caller decides what to document - in this
repository, the `files` pattern of the `actdocs` hook in `prek.toml`. The one
exception is the repository index, which by nature has to list every action
rather than only the ones that just changed.

| Flag | Meaning |
| :--- | :--- |
| `--check` | Report whether anything would change, and write nothing |
| `--root` | Repository root that generated paths resolve against |
| `--repo-slug` | `owner/repo` stamped into usage snippets (`ACTION_REPO_SLUG`) |
| `--ref-sha` | Commit SHA stamped into usage snippets (`ACTION_REF_SHA`) |
| `--ref-version` | Version stamped as a trailing comment (`ACTION_REF_VERSION`) |

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
| `<!-- index start -->` / `<!-- index end -->` | The repository index, in `README.md` |

A document with no markers is reported rather than overwritten. A document that
does not exist yet is scaffolded once.

## Development

```sh
mise run check
```

## License

Apache-2.0. See [`LICENSE`](LICENSE) for the terms and [`NOTICE`](NOTICE) for
attribution.
