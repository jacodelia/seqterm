# Releasing

The whole workspace shares **one** version, defined in a single place.

## Single source of truth

Root `Cargo.toml`:

```toml
[workspace.package]
version = "1.1.0"
```

Every crate inherits it:

```toml
[package]
name = "seqterm-…"
version.workspace = true   # ← do NOT hardcode a version here
```

So a release is a one-line edit. The About screen / `--version` read the same
value through `env!("CARGO_PKG_VERSION")`, so they follow automatically.

## Cut a release

1. Pick the new version (semver): bug-fix → patch, new features → minor,
   breaking → major.
2. Bump the one line in root `Cargo.toml` (`[workspace.package].version`).
3. `cargo build` — refreshes `Cargo.lock` so every crate resolves to the new
   version. Confirm with:
   ```
   cargo metadata --no-deps --format-version 1 \
     | python3 -c "import json,sys;print(set(p['version'] for p in json.load(sys.stdin)['packages']))"
   ```
   Expect a single `{'<new version>'}`.
4. `cargo test` — keep it green.
5. Commit `Cargo.toml` + `Cargo.lock` (`chore(release): vX.Y.Z`).
6. Tag and push:
   ```
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push && git push --tags
   ```

## Rules / guard

- **Never** put a literal `version = "x.y.z"` in a crate's `[package]`; always
  `version.workspace = true`. A quick audit:
  ```
  grep -rE '^version = "' crates/*/Cargo.toml   # must print nothing
  ```
- Internal crates depend on each other by `{ path = … }` with no version pin, so
  there is nothing else to bump.
- `edition` is intentionally per-crate (some are 2021, some 2024) — not part of
  the release bump.
