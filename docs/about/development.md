# Development

```sh
mise install        # the pinned Rust toolchain, prek and uv
mise run check      # fmt, clippy and tests, exactly as CI runs them
```

## Hooks

```sh
prek run --all-files
```

`mise run check` verifies formatting; the `cargo-fmt` hook fixes it. If `check`
fails on formatting, run `cargo fmt --all`.

## Documentation

```sh
mise run docs:serve   # http://localhost:8000
mise run docs:build   # writes site/, as the publishing workflow does
```

The site is built with [Zensical] and published to GitHub Pages by
`.github/workflows/docs.yml` on every push to `main`. Pull requests build it
without publishing, so a broken link fails the pull request rather than the
deployment.

## Tests

The suite is deliberately behavioural: test names read as sentences about what
the tool guarantees, and the golden tests pin the exact bytes of generated
documents. A change that alters output should change a golden test on purpose,
never incidentally.
