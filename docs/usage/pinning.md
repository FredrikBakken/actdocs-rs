# Pinning

Usage snippets name a reference, and there are two honest ways to write one.

## `--pin sha` (default)

```yaml
uses: owner/repo/.github/actions/example@0a1b2c3…  # v1.2.0
```

Resolves to exactly one commit however the tag moves afterwards, with the
version alongside so the line stays readable. This is correct everywhere, which
is why it is the default.

## `--pin version`

```yaml
uses: owner/repo/.github/actions/example@v1.2.0
```

Shorter and legible, and equal in integrity to a SHA **only** where the
publishing repository has enabled [immutable releases]. GitHub then locks a
release tag to its commit and forbids reusing the name, even after the release
is deleted.

!!! warning "It is a claim about the publisher, not a preference"

```text
Choosing `version` for a repository without immutable releases produces
snippets that look pinned and are not. Nothing here can detect the
difference: asking GitHub would put a network call in the middle of a
generator whose whole point is reproducible output.
```

Set it once, in a configuration file, rather than on every invocation:

```toml
# .actdocs.toml
pin = "version"
```

[immutable releases]: https://docs.github.com/en/repositories/releasing-projects-on-github
