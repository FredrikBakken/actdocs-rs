# Use as a hook

This repository is itself a hook repository, so a project consumes it by
reference rather than installing anything. [pre-commit] and [prek] read the
same definitions, so pick whichever your project already uses.

=== "`.pre-commit-config.yaml`"

  ```yaml
  repos:
    - repo: https://github.com/FredrikBakken/actdocs-rs
      rev: v0.1.0
      hooks:
        - id: actdocs
          args: ["--docs-dir-target", "docs", "--index-target", "README.md"]
  ```

=== "`prek.toml`"

  ```toml
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

Pin `rev` to the latest [release]. Releases are immutable, so a tag is locked
to its commit and can never be moved, deleted or reused — pinning to one
carries the same guarantee as pinning a commit SHA, without the unreadability.

## The two hooks

| Id | Behaviour |
| :--- | :--- |
| `actdocs` | Rewrites documents in place. For local commits |
| `actdocs-check` | Fails if anything is out of date, and writes nothing. For CI |

`--check` lives in `actdocs-check`'s `entry` rather than its `args`, so setting
`args` for something else cannot silently turn the check back into a rewrite.

Both are built from source on first use, so there is no separate installation
step and nothing to keep in step with the pinned `rev`.

## What they match

```text
^\.github/(actions/.+/action\.ya?ml|workflows/[^/]+\.ya?ml)$
```

Which is where GitHub requires those files to live, and what the index
discovers. Case-sensitive on purpose: GitHub does not treat `CI.YML` as a
workflow, and neither does this.

Routing is by file name rather than contents, so widening `files` beyond that
will write a document beside any YAML the hook is handed.
