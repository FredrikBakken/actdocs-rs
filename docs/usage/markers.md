# Markers

Generated content lives between HTML comments. Everything outside them is
yours and is never touched.

| Pair | Contents |
| :--- | :--- |
| `<!-- actdocs start -->` / `<!-- actdocs end -->` | The generated tables |
| `<!-- usage start -->` / `<!-- usage end -->` | A copy-pasteable call — a step for an action, a job for a workflow |
| `<!-- index start -->` / `<!-- index end -->` | The repository index, with `--index-target` |

A document that does not exist is scaffolded with the markers it needs. A
document that exists but has none is reported and left alone rather than
overwritten, because guessing where the tables belong would destroy prose.

A document written under `--docs-dir-target` is scaffolded with a line linking
back to the manifest it was generated from, as an absolute URL into the
repository built from `--repo-slug`: the published site does not contain the
`.github` tree, so a relative path would point at nothing. It names `HEAD`
rather than a pinned reference, and so follows the default branch. Like
anything outside the markers, the line is written once and is then yours.

The index is the one exception to scaffolding: `--index-target` names a
document that must already exist, with the index markers already in it.
