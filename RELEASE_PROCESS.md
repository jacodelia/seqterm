# SeqTerm Release Process

This document describes how to cut a SeqTerm release.

---

## Versioning

SeqTerm uses **Semantic Versioning** (`MAJOR.MINOR.PATCH`):

| Bump | When |
|------|------|
| `MAJOR` | Incompatible project file format changes that require manual migration; major architecture rewrites |
| `MINOR` | New features, new FX processors, new views, new export formats — backward-compatible with existing project files |
| `PATCH` | Bug fixes, performance improvements, documentation updates — no new features |

The version is set in `Cargo.toml` at the workspace root:
```toml
[workspace.package]
version = "0.1.0"
```

All crates inherit this version via `version.workspace = true`.

---

## Release Checklist

### 1. Prepare

- [ ] All P0 and P1 TODO items for the milestone are marked `[x]`
- [ ] `cargo test --workspace` passes on Linux, macOS, and Windows
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] Update `CHANGELOG.md` (see below)
- [ ] Bump version in workspace `Cargo.toml`
- [ ] Run `cargo build --release` locally and smoke-test

### 2. Create the release tag

```bash
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

This triggers the GitHub Actions release workflow (`.github/workflows/release.yml`), which builds:
- `seqterm_0.2.0_amd64.deb` + `seqterm-0.2.0-1.x86_64.rpm` + `.tar.gz` (Linux x86\_64)
- `seqterm_0.2.0_arm64.deb` (Linux ARM64 cross-compiled)
- `seqterm_0.2.0_armhf.deb` (Linux ARMv7 / Raspberry Pi 32-bit)
- `seqterm-0.2.0-windows-x86_64.msi` (Windows)
- `seqterm-0.2.0-windows-arm64.msi` (Windows ARM64)
- `SeqTerm-0.2.0-macos-universal.dmg` (macOS Universal)

### 3. After the CI release run

- [ ] Download and verify each artifact on a real machine (or VM)
- [ ] Update the GitHub Release description with highlights from `CHANGELOG.md`
- [ ] Announce on relevant channels

---

## Changelog Format

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

```markdown
## [0.2.0] — 2026-06-15

### Added
- Expander FX processor (downward/upward expansion)
- Pan FX processor with linear and constant-power laws
- Drum channel mode: MIDI ch 10 routing, 16-pad GM drum map
- Note CC11 (expression) and CC93 (chorus send) per-step
- InstrumentBackend trait in seqterm-ports
- Arranger: track types, colors, visibility toggle, clip split/glue
- Mixer: phase invert, mono, record arm, channel color, drum indicator

### Fixed
- Bank 128 (GM percussion) was incorrectly clamped to 127 in SF2 preset enumeration
- SMF-imported "bpm" automation lanes now apply during playback

### Changed
- FxKind enum expanded from 9 to 29 variants to cover all audio-engine processors
- ArrangerState now tracks selected_col, snap grid, and multi-select set

## [0.1.0] — 2026-05-01

Initial release.
```

---

## Schema Migrations

If a release changes the project file format:

1. Bump `Project::CURRENT_VERSION` in `seqterm-core/src/project.rs`
2. Add a migration function in `seqterm-persistence/src/migration.rs`
3. Test migration from `v(N-1)` to `vN` with a real project file
4. Note the version bump in `CHANGELOG.md` under **Changed**

Rules:
- Migrations are **forward-only** (old → new). We never add downgrade migrations.
- New fields must have `#[serde(default)]` so old files still deserialize.
- Removed fields should be marked with `#[serde(skip)]` for one release before removal.

---

## Patch Releases

For bug fixes between minor releases:

```bash
git checkout -b fix/critical-audio-glitch v0.2.0
# make the fix, commit
git tag -a v0.2.1 -m "Patch: fix critical audio glitch"
git push origin v0.2.1
```

Patch releases use the same CI workflow and artifact set.

---

## Pre-releases (alpha / beta / rc)

Use suffixes in the version string: `0.3.0-alpha.1`, `0.3.0-beta.2`, `0.3.0-rc.1`.

Mark the GitHub Release as "Pre-release" to exclude it from the "latest" tag.
