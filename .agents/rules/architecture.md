# Architecture Rules — Refract

These rules govern architectural boundaries and evolution across Web (`packages/`, `apps/web/`) and Desktop (`crates/`, `apps/desktop/`).

## 1. Dual-Engine Separation, Single Product Contract

- **Engines stay platform-native**:
  - Web uses `@refract/engine-wasm` inside dedicated Web Workers (with lazy-loaded FFmpeg WASM).
  - Desktop uses `crates/refract-engine` (native `image`, `lopdf`, and system `ffmpeg`/`ffprobe` + optional PDF rasterizers) inside native OS threads.
- **Product contract is shared** (implemented in Phase 1):
  - `specs/tool-manifest.json` (validated by `specs/tool-manifest.schema.json`) is the single source of truth for tool IDs, domains, accepted/output formats, input multiplicity, option schemas, defaults, min/max/step constraints, `showIf` visibility rules, output naming templates, resource limits, capabilities, and platform support.
  - TypeScript and Rust definitions are generated deterministically from the manifest; hand-editing generated files is prohibited.
  - CI must verify zero drift between `specs/tool-manifest.json` and generated TypeScript/Rust/documentation outputs.
- **Preserve all 11 public tool IDs**: `convert-image`, `compress-image`, `resize-image`, `merge-pdf`, `split-pdf`, `reorder-pdf`, `images-to-pdf`, `pdf-to-images`, `compress-pdf`, `convert-audio`, `trim-audio`.

## 2. Canonical Job Lifecycle & Error Semantics

Both `@refract/core` and `crates/refract-core` + `apps/desktop` must share the same 9-state lifecycle:

```text
Queued -> Probing -> Running -> Verifying -> Saved
             |          |          |
             v          v          v
       NeedsPassword  Failed     Failed
             |
             v (back to Probing on unlock, or Canceled on skip)
          Probing

Any non-terminal state -> Canceled
Probing validation failure -> Rejected (distinct from engine Failed)
```

Invariants:
- `Rejected` represents pre-run input/option/limit rejection (empty file, unsupported format, limit exceeded, invalid range/order). `Failed` represents an execution or output-verification failure. `Canceled` is neither `Rejected` nor `Failed`.
- Every job snapshots its typed tool options and destination settings at **enqueue time**; switching tools or editing form fields must never mutate queued or retryable jobs.
- Retry preserves the job's original inputs and configuration without duplicating unnecessary in-memory byte buffers.

## 3. Real Cancellation

- Cancellation must never be cosmetic UI state.
- **Web**: Canceling aborts the `AbortController`, resolves any pending password prompt as skipped, releases queued concurrency slots, and terminates the active Web Worker to reclaim WASM memory.
- **Desktop**: Each job owns a cancellation token (`Arc<AtomicBool>` or equivalent). Canceling a queued job prevents execution; canceling an active job stops Rust processing loops at safe checkpoints and terminates spawned child processes (`ffmpeg`, `ffprobe`, `mutool`, `pdftoppm`, `pdftocairo`, `gs`), cleaning up any temporary files.

## 4. File Safety, Probing, Limits & Output Verification

- **Upfront probing**: Always detect file type by magic bytes (never trust file extensions alone), check emptiness, check encryption, and enforce operation-specific limits (file count, byte size, pixel count, page count) before starting work.
- **Output verification**: Every generated output must be re-probed (format, non-empty bytes, and expected dimensions / page count / duration / max bytes where applicable) before transitioning a job to `Saved`.
- **Desktop atomic persistence**:
  - Never overwrite an existing user file; compute unique destination paths.
  - Write outputs to a temporary sibling file in the target directory, verify the generated output, and atomically rename/persist to the final destination. Clean up temp files on failure or cancellation.
  - Prefer path-based processing over full-file `std::fs::read()` copies whenever the underlying engine allows it.
- **Web memory & concurrency**:
  - Enforce operation-specific file selection caps from the selected tool's manifest limits rather than a single global constant.
  - Adapt concurrency to device/viewport constraints (e.g., 1 heavy job on constrained/mobile devices, 2 on desktop) and warn before exceeding estimated memory budgets.
  - Revoke Blob URLs cleanly on removal, retry, re-run, clear, and unmount; avoid retaining duplicate byte copies beyond what is needed for active display/download/retry.

## 5. Desktop Modularization & Declarative UI Boundary

- Keep `apps/desktop/ui/` purely declarative; all state transitions, persistence, validation, and processing live in Rust.
- Target Rust module structure in `apps/desktop/src/`:
  - `main.rs` — thin bootstrap & wiring
  - `app/` — application state and UI event dispatch
  - `jobs/` — queue, controller, state transitions, cancellation tokens
  - `tools/` — manifest-driven typed options and engine dispatcher
  - `settings/` — load/save persistence
  - `platform/` — native file dialogs, folder reveal, external dependency probing (`ffmpeg`, `ffprobe`, PDF rasterizers)
- Target Slint UI structure in `apps/desktop/ui/`:
  - `app.slint`
  - `theme/palette.slint`
  - `components/` (`sidebar`, `toolbar`, `file_queue`, `job_row`, `options_panel`, `password_dialog`, `status_bar`)
- Replace generic `opt_string_*` / `opt_int_*` / `opt_combo_*` UI slots with manifest-driven typed options.
- Collapse navigation / move inspector into a drawer at narrow window widths (`820x580`) instead of squeezing columns.

## 6. External Dependency Transparency & Documentation Parity

- Clearly distinguish three states in both UI and documentation:
  1. Implemented and built-in on platform.
  2. Available only when an external binary is present on `PATH` (`ffmpeg`/`ffprobe`, PDF rasterizers).
  3. Unsupported on platform (e.g., HEIC decoding on Desktop).
- Probe external dependencies on startup in Desktop and surface actionable status before the user starts a job; disable only the affected features when a dependency is missing.
- Keep `README.md`, `DESIGN.md`, and `docs/` synchronized with generated platform support documentation so tool counts and dependency statements never drift.
