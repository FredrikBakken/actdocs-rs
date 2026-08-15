# Configuration

Settings come from four layers. A flag beats an environment variable, which
beats a file, which beats a built-in default — the ordering follows how
deliberately and how narrowly a value was stated.

## Files

The first of these that exists is the one used, outright:

1. `.actdocs.toml`
2. `actdocs.toml`
3. `config/actdocs.toml`
4. `.config/actdocs.toml`

The others are named on stderr and ignored. They are never merged, because
merged settings live in no single place a reader can open. `--config FILE`
replaces the search entirely, and a file named that way must exist.

```toml
# .actdocs.toml
docs-dir-target = "docs"
index-target = "README.md"
workflow-docs = "docs-dir"
repo-slug = "acme/tools"
pin = "sha"
```

Keys match the flags they mirror, minus the dashes. An unrecognised key is an
error rather than a silent no-op.

## Environment

Only the three values a CI system derives have variables:
`ACTDOCS_REPO_SLUG`, `ACTDOCS_REF_SHA` and `ACTDOCS_REF_VERSION`. The rest are
policy, and an exported shell variable that quietly changed every generated
document would be a bad way to find that out.

## Defaults

`--repo-slug`, `--ref-sha` and `--ref-version` default to the obvious
placeholders `<owner>/<repo>`, `<sha>` and `<version>`. Reaching into the local
clone for them would make output differ between a fork, a working copy and CI.
