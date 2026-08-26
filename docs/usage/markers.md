# Markers

Generated content lives between HTML comments. Everything outside them is
yours and is never touched.

| Pair | Contents |
| :--- | :--- |
| `<!-- actdocs start -->` / `<!-- actdocs end -->` | The generated tables |
| `<!-- usage start -->` / `<!-- usage end -->` | A copy-pasteable call — a step for an action, a job for a workflow |
| `<!-- index start -->` / `<!-- index end -->` | The repository index, with `--index-target` |
| `<!-- hooks start -->` / `<!-- hooks end -->` | The hooks the repository publishes, with `--hooks-target` |

A document that does not exist is scaffolded with the markers it needs. A
document that exists but has none is reported and left alone rather than
overwritten, because guessing where the tables belong would destroy prose.

A document written under `--docs-dir-target` is scaffolded with a line linking
back to the manifest it was generated from, as an absolute URL into the
repository built from `--repo-slug`: the published site does not contain the
`.github` tree, so a relative path would point at nothing. It names `HEAD`
rather than a pinned reference, and so follows the default branch. Like
anything outside the markers, the line is written once and is then yours.

The index and the hooks table are the exceptions to scaffolding: `--index-target`
and `--hooks-target` name documents that must already exist, with their markers
already in them. A hooks table also needs a `.pre-commit-hooks.yaml` at the
root, and says so rather than writing an empty region if there is none.
