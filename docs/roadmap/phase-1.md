# Phase 1 — Matrix Copy/Paste & Session Undo/Redo

**Spec:** `03_matrixUpdate.md` · **Status:** ✅ Implemented (no commits) · **Engine changes:** none

Deliver a complete clipboard (Copy/Cut/Paste between patterns) for the Matrix View and a full session-scoped Undo/Redo system covering every project edit. This phase touches only `seqterm-core`, `seqterm-history`, `seqterm-command`, and `seqterm-ui`.

---

## Goals

- Internal clipboard (not the OS clipboard), alive for the whole session.
- Copy / Cut / Paste with **Replace**, **Merge**, and **Insert** semantics.
- Single, multiple, rectangular, and full-pattern selection in the Matrix.
- Command-pattern Undo/Redo with descriptions, a 1000-step cap, and diff-based storage (no full-project copy per action where avoidable).
- Status-bar shows the next Undo/Redo action; disabled when none.

---

## Current state

- `seqterm-history` already provides `EditCommand` (apply/revert/description/as_any), `History` (push/record/undo/redo/begin_group/end_group), and a universal `ProjectSnapshot` plus typed commands (`SetNote`, `SetClipSource`, `SwapClips`, `SetPatternLength`, …). `App::record_edit` wraps arbitrary mutations.
- Matrix View renders an `A`–`P` × N grid of `Option<Clip>`; clips reference named patterns. Cursor + single-cell ops exist; there is no rectangular selection or clipboard.
- Undo/redo is wired (`Ctrl+Z`/`Ctrl+Y`) but coverage is partial and there is no status-bar description of the pending action.

---

## Work breakdown

### 1. Selection model (Matrix)
- [x] `MatrixState.selection_anchor` → rectangular region (`App::matrix_region`).
- [x] Keyboard: `Shift`+move extends (vim uppercase + arrow modifier both handled); `Ctrl+A` select all; `Esc` clears.
- [x] Mouse: shift-click range + left-drag rectangle (`matrix_cell_at` hit-test).
- [x] Visual feedback: selected region tinted (muted indigo) distinct from cursor/grab/drop. (Copied-region overlay not retained — copy is instantaneous; status toast reports it.)

### 2. Internal clipboard
- [x] `MatrixClipboard { height, width, cells: Vec<Vec<Option<ClipboardCell>>>, source_label }`; `ClipboardCell { clip, pattern }` deep-copies the referenced pattern.
- [x] Lives on `App.matrix_clipboard` (session); cleared on new/open project.
- [x] Deep copy → pastes are independent (new unique pattern keys).

### 3. Copy / Cut / Paste
- [x] `AppCommand::{MatrixCopy, MatrixCut, MatrixPaste(PasteMode)}`, `PasteMode::{Replace,Merge,Insert}` in `seqterm-command`.
- [x] **Replace** = fresh copy (new pattern); **Merge** = fill empty steps (`merge_pattern`); **Insert** = prepend/shift steps (`insert_pattern`).
- [x] Shortcuts: `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+Shift+V` (merge), `Ctrl+Alt+V` (insert) — Matrix grid only.
- [x] Paste anchored at cursor; clamps to grid bounds; rebuilds audio slots.

### 4. Undo/Redo completeness (Command Pattern)
- [x] Cut/Paste wrapped in `record_edit` (one undo step each) on top of the existing per-edit command coverage. Undo/Redo handlers also support `Ctrl+Shift+Z` = Redo.
- [x] `History`: configurable cap (`AppSettings.max_undo_steps`, default **1000**) via `History::set_cap`; oldest dropped. (Started with grouped `ProjectSnapshot`; diff-minimization deferred per the Risks note.)
- [x] `Undo → Redo → Undo → Redo` stability — unit-tested in `seqterm-history`.
- [x] Stacks cleared on project new/open.

### 5. Status bar / TUI
- [x] Transport bar shows `↶ <undo>` / `↷ <redo>` (dim `—` when none) via `History::peek_{undo,redo}_description`.
- [x] Copy/cut/paste toasts ("Copied N clip(s) [A1..B3]", "Pasted N clip(s) (merge)").

---

## Data-model changes

- `seqterm-ui`: `MatrixSelection`, `MatrixClipboard` (transient, not persisted).
- `seqterm-history`: optional `PasteCommand`, `CutCommand` (or reuse `ProjectSnapshot` grouping); `History::set_limit(usize)`.
- No `seqterm-core` schema change required (clipboard is in-memory). A configurable `max_undo_steps` lives in settings.

---

## Affected crates / files

- `crates/seqterm-ui/src/app.rs`, `views/matrix.rs`, `lib.rs` (commands, key/mouse handlers, status bar).
- `crates/seqterm-command/src/lib.rs` (new `AppCommand`s).
- `crates/seqterm-history/src/lib.rs` (limit + any new commands).
- `crates/seqterm-settings` (`max_undo_steps`).

---

## Tests (334 workspace tests green)

- [x] `merge_pattern` / `insert_pattern` semantics (`seqterm-ui::matrix_clipboard_tests`).
- [x] History cap eviction (1000), peek descriptions, `Undo→Redo→Undo→Redo` stability (`seqterm-history`).
- [~] Full App-level copy/paste integration (the pure paste-merge logic is unit-tested; the App-driven flow is exercised manually — a headless App harness is a follow-up).

## Risks

- Borrow/locking around `App.project` during multi-cell grouped edits — mitigate with the existing `record_edit` snapshot pattern.
- Diff-based history complexity vs `ProjectSnapshot` simplicity — start with grouped snapshots, optimize hot paths only if profiling shows it.

## Exit criteria

Copy/Cut/Paste (3 modes) work between any patterns via keyboard and mouse with visual feedback; all listed actions are undoable with descriptions and a 1000-step bound; tests green; no audio-engine changes.
