# Use as a hook

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/FredrikBakken/actdocs-rs
    rev: v0.1.0
    hooks:
      - id: actdocs
        args: ["--docs-dir-target", "docs", "--index-target", "README.md"]
```

Two ids are published:

| Id | Behaviour |
| :--- | :--- |
| `actdocs` | Writes. Modified files are reported by the hook runner as usual |
| `actdocs-check` | Writes nothing, and fails if anything would change |

Neither writes into a documentation tree nor touches an index unless asked to,
so `- id: actdocs` on its own updates only the document beside each source.
