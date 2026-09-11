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
docs-dir-layout = "flat"
index-target = "README.md"
hooks-target = "README.md"
workflow-docs = "docs-dir"
repo-slug = "acme/tools"
pin = "sha"
```

Keys match the flags they mirror, minus the dashes. An unrecognised key is an
error rather than a silent no-op.

## Layout

`docs-dir-layout` decides the shape of each document under `docs-dir-target`.
Only the mirror is affected; the document beside the source keeps GitHub's
layout.

| Value | Writes |
| :--- | :--- |
| `"flat"` | `docs/actions/<name>.md` |
| `"directory"` | `docs/actions/<name>/README.md` |
| `"directory-index"` | `docs/actions/<name>/index.md` |

`flat` is the default, because it is what every already-generated tree has on
disk. A directory is what you want once a page needs siblings — a screenshot, a
diagram, a sub-page. `README.md` is what GitHub renders when someone browses the
tree; `index.md` is what a static site generator looks for.

The entry file is named as part of the layout rather than beside it, because it
is a property of having a directory at all. `flat` has no file name to choose —
the document *is* `<name>.md` — so there is no way to write the combination and
nothing has to validate against one.

As a table, the same setting can say more. Rules are resolved from the most
specific statement to the least, the way the layers above are:

```toml
[docs-dir-layout]
default = "flat"                     # everything, unless something below applies
workflows = "directory"              # every workflow

[docs-dir-layout.action]
setup-toolchain = "directory-index"  # one action, by directory name

[docs-dir-layout.workflow]
release = "flat"                     # one workflow, by file name without the extension
```

A rule naming one target beats a rule about its kind, which beats `default`.
Most repositories want one shape everywhere and should write the one-word form.

### Changing a layout

Switching layouts leaves the previously generated files where they are. Each is
named on stderr and the run exits `1`, but none is deleted: everything outside
the markers is hand-written, and a generator that throws away prose it did not
produce is one nobody trusts twice. Move what is worth keeping into the new
document, delete the old file, and the run is clean again.

## Environment

Only the three values a CI system derives have variables:
`ACTDOCS_REPO_SLUG`, `ACTDOCS_REF_SHA` and `ACTDOCS_REF_VERSION`. The rest are
policy, and an exported shell variable that quietly changed every generated
document would be a bad way to find that out.

## Defaults

`--repo-slug`, `--ref-sha` and `--ref-version` default to the obvious
placeholders `<owner>/<repo>`, `<sha>` and `<version>`. Reaching into the local
clone for them would make output differ between a fork, a working copy and CI.

`repo-slug` is also what makes the source link on a mirrored document resolve,
so a repository publishing its documentation should state it rather than leave
the placeholder.
