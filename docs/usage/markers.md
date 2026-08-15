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

The index is the one exception to scaffolding: `--index-target` names a
document that must already exist, with the index markers already in it.
