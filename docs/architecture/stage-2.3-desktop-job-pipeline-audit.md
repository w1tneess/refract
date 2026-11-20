# Stage 2.3 — Desktop Job Pipeline Audit & Implementation Plan

- **Repository**: `w1tneess/refract`
- **Branch**: `phase-2-desktop-core`
- **Audited Commit**: `b2ab43e6da7aeb4395d588ce7d7e220cb9190fb6`
- **Scope**: Read-only audit of the native Rust/Slint Desktop execution pipeline (`apps/desktop`, `crates/refract-core`, `crates/refract-engine`, `specs/tool-manifest.json`) and incremental Stage 2.3 implementation plan.

---

## 1. Current Architecture & Execution Flow

The Desktop application currently lives in a single 1,529-line binary crate (`apps/desktop/src/main.rs`) paired with a single Slint UI definition (`apps/desktop/ui/app.slint`). Stages 2.1 and 2.2 built manifest-backed limits, naming, 9-state job transitions, cancellation tokens, `SecretPassword`, cancellable subprocesses, and safe audio/PDF/image engine APIs in `refract-core` and `refract-engine`, **but `apps/desktop/src/main.rs` has not yet been migrated to use any of them**.

### 1.1 End-to-End Lifecycle Trace

| Stage | Current Implementation (File & Functions) | Existing Guarantees | Missing Guarantees & Races |
| --- | --- | --- | --- |
| **1. User input & job creation** | `setup_pick_files` (`apps/desktop/src/main.rs:437-503`), `app.slint:634-658`. Opens `rfd::FileDialog` with extension filter + `*`. Appends a `JobRecord` (`main.rs:12-24`) per file with `JobStatus::Waiting`. | Preserves per-file queue rows and order (`setup_move_job`, `main.rs:1109-1139`). | Swallows `fs::metadata` errors as `size_bytes = 0` (`main.rs:476`). No existence, regular-file, empty-file, magic-byte, or manifest limit checks. Captures **no options or output destination** at enqueue time. |
| **2. Tool defaults & UI state** | `apply_tool_selection` (`main.rs:168-237`), `app.slint:217-226, 456-562`. Seven generic Slint properties (`opt_string_1..2`, `opt_int_1..2`, `opt_float_1`, `opt_combo_1..2`) shared across all 11 tools. | Sets initial UI values when switching tools; loads tool titles/descriptions/extensions from `ToolId::ALL`. | Hardcoded defaults and SpinBox bounds drift from `specs/tool-manifest.json`. Switching tools overwrites the shared generic slots for previously queued waiting jobs. |
| **3. Option capture** | `capture_options` (`main.rs:684-720`), called only inside `setup_start_processing` (`main.rs:746`) and `setup_retry_job` (`main.rs:1022`). Not called at all in `setup_password_handlers` (`main.rs:612-681`). | Clamps negative `i32` UI values to `0` when casting to `u32`. | **No enqueue-time snapshot.** Mixed-tool queues feed the *currently selected* tool's generic slots to jobs belonging to *other* tools. Retries recapture live mutated UI options. Password unlock drops options entirely and hardcodes `pdf-to-images` to `150` DPI + `"png"` (`main.rs:1452`). |
| **4. Validation & limits** | Scattered inline checks inside `setup_start_processing` (`main.rs:819`), `process_job` (`main.rs:1304, 1334, 1356, 1380`), and engine decoders after full file reads. | Engine rejects empty/HEIC/truncated/invalid-range inputs once bytes reach the engine. | `refract_core::limits` is **unused**. All 9 image/PDF tools call `std::fs::read(&job.input_path)` unconditionally (`main.rs:827, 892, 1169..1431`) before checking size, format, pixels, pages, or memory budget. Audio tools do not check `ffprobe` or file existence/format before spawning FFmpeg. |
| **5. Execution & dispatch** | `setup_start_processing` (`main.rs:722-998`), `process_job` (`main.rs:1162-1408`), `process_job_with_password` (`main.rs:1410-1469`). Spawns a raw `std::thread::spawn` per click, guarded by `is_busy: Arc<AtomicBool>`. | Groups waiting `merge-pdf` and `images-to-pdf` rows into batch runs during "Process all" while keeping per-file rows in the UI. | **Race**: `on_password_entered` (`main.rs:612-681`) never checks `is_busy.swap(true)` and never toggles `window.set_is_running`, allowing concurrent worker threads to race on `jobs_store`, `is_busy`, and output paths. `remove_job`/`move_job` (`main.rs:1094-1139`) can mutate the queue mid-run. Uses ad-hoc 5-state `JobStatus` instead of `refract_core::job::JobState`. |
| **6. Cancellation & retry** | `setup_retry_job` (`main.rs:1000-1092`). No cancel callback or `CancelToken` exists anywhere in `apps/desktop`. | Single-file retry button resets status to `Working` and re-runs `process_job`. | **Zero cancellation** across all 11 tools (all call non-cancellable engine wrappers). Retry has no error retryability policy, recaptures live mutated UI options, breaks `merge-pdf` (`main.rs:1351-1354`), and collapses `images-to-pdf` to a single file (`main.rs:1215-1235`). |
| **7. Output persistence** | `determine_output_path` (`main.rs:1141-1160`), `uniquify_path` (`main.rs:74-93`), direct `std::fs::write` (`main.rs:839, 903, 1184..1457`), except `audio::convert_audio`/`trim_audio` which use engine-side `TempPath::persist_noclobber` (`audio.rs:368-441`). | Audio engine never overwrites an existing destination and cleans up owned temp files. | **Data-loss defects on 9/11 tools**: `uniquify_path` falls back to returning the *existing* path after 9,999 collisions (`main.rs:92`), has a TOCTOU window before `std::fs::write` (`O_CREAT|O_TRUNC`), writes directly without temp staging, and leaves orphaned partial files on multi-output failure (`split-pdf`, `pdf-to-images`). Filenames also drift from `refract_core::naming`. |
| **8. Verification & completion** | None. Worker sets `j.status = JobStatus::Done` immediately after `std::fs::write` or `audio::*` returns `Ok(())`. | None beyond OS write return status. | **No post-write verification** (`JobState::Verifying` and `ErrorCode::VerificationFailed` unused). In `pdf-to-images` (`main.rs:1257, 1453`) and `split-pdf` (`main.rs:1322`), an empty output list would still transition to `Done` with a synthetic/parent path. |

### 1.2 Detailed Audit of the Six Required Areas

#### A. Typed Job Options & Manifest Drift
- Desktop passes an untyped `CapturedOptions` bag (`main.rs:685-695`) populated from 7 reused Slint properties.
- Verified defaults & semantics drift against `specs/tool-manifest.json`:
  - `resize-image`: Desktop defaults `width = 1920` (`main.rs:208`) vs manifest `1200` (`specs/tool-manifest.json:315`); Slint caps `SpinBox` at `10000` (`app.slint:483-485`) vs manifest `20000`; manifest options `unit` (`px`/`cm`), `dpi` (`72..=1200`), and `to` (`same`/`jpeg`/`png`/`webp`) are not wired (`main.rs:1204` always passes `None`).
  - `compress-image`: Slint `SpinBox` range is `10..=10000` (`app.slint:476`) vs manifest `5..=20000` (`specs/tool-manifest.json:242-243`); `main.rs:1189` silently clamps `.max(1)`.
  - `split-pdf`: Desktop defaults to `"1-2, 3"` (`main.rs:222`) and rejects empty ranges with `ErrorCode::DamagedFile` (`main.rs:1304-1309`), conflicting with manifest default `""` ("Every page", `specs/tool-manifest.json:456`) and `pdf::parse_ranges("")` (`pdf.rs:232`).
  - `reorder-pdf`: Desktop defaults to `"1, 2"` (`main.rs:225`) vs manifest `""` (`specs/tool-manifest.json:516`) and rejects empty input with `ErrorCode::DamagedFile` (`main.rs:1334-1339`) instead of `InvalidOption`.
  - `trim-audio`: Desktop defaults `end` to `"30"` (`main.rs:233`) vs manifest `0` (EOF, `specs/tool-manifest.json:868`), and ignores manifest `bitrate` (`main.rs:1400` passes `None`).
  - `convert-audio`: Desktop silently defaults unparseable bitrate to `192` and clamps `64..=320` (`main.rs:1363-1368`) instead of rejecting invalid options via `audio::parse_bitrate_str`.
  - `pdf-to-images`: Desktop clamps `72..=600` (`main.rs:1239`) vs engine `render::validate_render_dpi` (`36..=600`, `render.rs:320`) and manifest (`36..=300`, `specs/tool-manifest.json:669-670`).
  - `images-to-pdf`: Desktop silently defaults `dpi < 72` to `150.0` and clamps `quality` (`main.rs:884-889, 1217-1225`) rather than validating; also uses different output suffixes in batch (`"images"`, `main.rs:900`) vs single retry (`"images-to-pdf"`, `main.rs:1230`).

#### B. Input Validation & Resource Limits
- **Core vs Desktop boundary**:
  - `crates/refract-core` already owns manifest-backed limits (`limits_for_tool`, `check_limits`, `check_memory`, `limits.rs:23-124`), magic-byte detection (`detect.rs:4-86`), and accepted format lists (`ToolId::manifest().desktop_accepted_formats`).
  - Desktop (`apps/desktop`) owns filesystem metadata inspection (`fs::metadata` -> exists, `is_file()`, byte size), bounded header sniffing (reading only the first 1,024 bytes before reading full files), aggregate batch accounting (sum of bytes/pixels/pages across grouped inputs in `merge-pdf` / `images-to-pdf`), and engine dependency checks (`audio::detect_audio_dependencies()`).
- **Current gap**: None of these pre-read checks exist in `apps/desktop`. Every image/PDF tool allocates the entire file into RAM via `std::fs::read` before checking size, format, dimensions, or pages. Audio tools never validate file existence, non-zero length, or audio format before launching FFmpeg/FFprobe, and `trim-audio` chooses output extension from filename extension (`main.rs:1386-1390`) rather than magic-byte detection.

#### C. Output Persistence & No-Clobber Guarantees
- All 9 non-audio tools use `uniquify_path` + `std::fs::write`, causing three concrete safety bugs:
  1. Exhaustion fallback returns the existing file path (`main.rs:92`) and overwrites it.
  2. TOCTOU race between `exists()` and `std::fs::write()` silently truncates a concurrently created file.
  3. Direct writes and unrolled multi-output loops (`split-pdf`, `pdf-to-images`) leave partial/corrupted files on disk on failure, crash, or cancellation.
- Audio tools inherit `audio.rs`'s exclusive `TempPath` + `persist_noclobber`, but Desktop still generates the target path up front via `uniquify_path` rather than resolving collisions atomically at persist time.
- **Shared abstraction verdict**: **Justified.** `crates/refract-engine/Cargo.toml` already depends on `tempfile = "3.16"` (no new dependencies needed). Extracting a small `crates/refract-engine/src/persist.rs` module (sibling exclusive temp allocation, atomic `persist_noclobber` with bounded unique-name retry or strict collision error, and a multi-output `StagedOutputSet` that rolls back all staged/uncommitted files on error or cancellation) unifies all 11 tools—including `audio.rs`—under one proven no-clobber mechanism.

#### D. Output Verification
- **Currently absent across all 11 tools.** Neither `JobState::Verifying` nor `ErrorCode::VerificationFailed` is referenced in `apps/desktop`.
- **Appropriate lightweight verification per output type** (aligned with `packages/core/src/verify/verify.ts`, avoiding redundant full pixel decodes):
  - **All outputs**: file exists on disk (`fs::metadata`), regular file, `len > 0`, magic-byte `refract_core::detect` matches expected `Format`.
  - **Images (`convert-image`, `compress-image`, `resize-image`, `pdf-to-images`)**:
    - JPEG: complete EOI (`jpeg_has_complete_eoi`) and no residual EXIF marker (`read_jpeg_info` / APP1 `Exif\0\0` check, matching Web `verify.ts:45-49`).
    - Header dimension probe (`native::probe_image` / `image::probe_dimensions`): non-zero dimensions; for `resize-image`, exact `(expected_w, expected_h)` match; for `compress-image`, `len <= target_kb * 1024`.
  - **PDF (`merge-pdf`, `split-pdf`, `reorder-pdf`, `images-to-pdf`, `compress-pdf`)**:
    - `pdf::probe_pdf` succeeds, `page_count > 0`, and matches expected count (sum of input pages for `merge-pdf`, input count for `images-to-pdf`, range length for each `split-pdf` output, original page count for `reorder-pdf` and `compress-pdf`).
  - **Audio (`convert-audio`, `trim-audio`)**:
    - Magic-byte format matches expected audio format; when `ffprobe` is available (always required for `trim-audio` and part of audio toolchain), `audio::probe_duration_cancellable` confirms measurable positive duration and within `0.25s` tolerance of expected duration (matching Web `verify.ts:35` `DURATION_TOLERANCE_S = 0.2s` + container framing margin).
  - **Multi-output (`split-pdf`, `pdf-to-images`)**: verify *every* staged output file **before** final no-clobber commit (so verification failure leaves zero final outputs in the user's directory and transitions the job to `Failed` with `ErrorCode::VerificationFailed`, never `Saved`).

#### E. Password & Secret Handling
- `crates/refract-engine/src/pdf.rs:38-75` already provides `SecretPassword` (redacted `Debug`, volatile-zeroed `Vec<u8>` on `Drop`/`clear()`), **but `apps/desktop` does not use it**.
- Current gaps:
  - `setup_password_handlers` (`main.rs:612-681`) passes Slint `SharedString` into a thread closure as plain text.
  - `process_job_with_password` (`main.rs:1410-1469`) only supports `compress-pdf` and `pdf-to-images`. Locked `merge-pdf`, `split-pdf`, and `reorder-pdf` jobs fail or return `"Password not needed for this tool"` (`main.rs:1464-1467`), even though `refract-engine` already implements `split_pdf_with_password`, `reorder_pdf_with_password`, `merge_pdfs_with_inputs` (per-input passwords), and `render_pdf_to_images_with_password_cancellable`.
  - `pdf-to-images` password unlock (`main.rs:1430-1463`) drops user DPI/format options (`150`, `"png"`) and skips embedded-image fallback.
- **Honest limitations**: Wrapping passwords in `SecretPassword` inside a dedicated non-serializable `JobSecretStore` minimizes lifetime, avoids `Debug`/log/error leakage, and clears owned Rust buffers after use, **but cannot guarantee complete erasure of every runtime copy** (Slint's `SharedString` input widget buffer, OS clipboard/IME buffers, allocator pages, and internal `lopdf` decryption buffers are outside our direct control).

#### F. Retry & Cancellation Semantics
- **Cancellation**: Completely unimplemented in `apps/desktop`. None of the cancellable engine APIs (`*_cancellable`) are called; there is no `CancelToken` per job or batch and no Slint cancel callback.
- **Non-interruptible library operations**: As established in Stage 2.2d, `image` crate decode/resize/encode and `lopdf` in-memory operations are not internally interruptible; cancellation is observed at explicit checkpoints (including post-encode/post-op before persistence and before marking `Saved`), whereas external subprocesses (`ffmpeg`, `ffprobe`, `mutool`, `pdftoppm`, `pdftocairo`, `gs`) are actively killed and reaped via `process::run_command_cancellable`.
- **Retryability policy**: Currently missing. Under `refract_core::error::ErrorCode` (`error.rs:5-39`):
  - **Non-retryable rejections (`JobState::Rejected`, never auto-retried)**: `EmptyFile`, `UnsupportedFormat`, `InvalidOption`, `LimitExceeded`, `MissingDependency` (unless user re-triggers manually after fixing environment/input—never automatic).
  - **Non-retryable terminal / user-action states**: `DamagedFile` (corrupt input), `NeedsPassword` / `WrongPassword` (handled via password unlock transition `NeedsPassword -> Probing`, not blind retry), `Saved`.
  - **Retryable failures (`JobState::Failed`, explicit user retry or controlled policy)**: `EngineFailed` (e.g., transient I/O, destination collision, subprocess failure), `VerificationFailed`, and explicit user re-run of `Canceled`.
- **State transition compliance**: Because `JobState::can_transition` (`job.rs:31-56`) defines `Saved`, `Rejected`, `Failed`, and `Canceled` as strictly terminal (no outgoing transitions), retrying a failed/canceled job must **spawn a fresh execution attempt** starting in `JobState::Queued` with a new `CancelToken` and the **exact original immutable `JobRequest`**, keeping every attempt's transitions 100% legal under `JobState::can_transition()` (mirroring `apps/web/src/useJobs.ts:163-177`).

---

## 2. Eleven-Tool Consistency Matrix

Status key: `PASS` = verified in Desktop implementation; `PARTIAL` = implemented but incomplete/defective; `MISSING` = not implemented in Desktop; `N/A` = genuinely not applicable.

| Tool | Typed options | Input validation | Limits | No-clobber output | Verification | Password handling | Retry/cancel |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `convert-image` | **MISSING** (`main.rs:685-720, 1170-1175`: live `CapturedOptions`, clamped quality, no enqueue snapshot) | **PARTIAL** (`main.rs:1169`: full `fs::read` first; engine `image.rs:384-396` checks empty/HEIC/EOF afterward) | **MISSING** (`refract_core::limits` unused in `apps/desktop`) | **MISSING** (`main.rs:74-93, 1184`: `uniquify_path` + `fs::write` TOCTOU & exhaustion overwrite) | **MISSING** (no post-write check in `main.rs:1184-1185`) | **N/A** (`specs/tool-manifest.json:188`) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts, no policy; cancel absent) |
| `compress-image` | **MISSING** (`main.rs:204-206, 1189`: live `CapturedOptions`, `.max(1)` clamp, `app.slint:476` min/max drift) | **PARTIAL** (`main.rs:1188`: full `fs::read` first; engine checks empty/HEIC/corrupt afterward) | **MISSING** (`refract_core::limits` unused) | **MISSING** (`main.rs:74-93, 1198`: `uniquify_path` + `fs::write`) | **MISSING** (does not verify file exists or `<= targetKB` on disk, `main.rs:1198-1199`) | **N/A** (`specs/tool-manifest.json:258`) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts; cancel absent) |
| `resize-image` | **MISSING** (`main.rs:207-210, 1204`: default `1920` vs `1200`, `cm`/`dpi`/`to` unwired, live capture) | **PARTIAL** (`main.rs:1202`: full `fs::read` first; engine validates after read) | **MISSING** (`refract_core::limits` unused) | **MISSING** (`main.rs:74-93, 1212`: `uniquify_path` + `fs::write`) | **MISSING** (does not verify output format/dimensions on disk, `main.rs:1212-1213`) | **N/A** (`specs/tool-manifest.json:364`) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts; cancel absent) |
| `merge-pdf` | **MISSING** (`main.rs:746, 832-838`: no tool opts, but output destination captured live at Run, not enqueue) | **PARTIAL** (`main.rs:819-828`: checks `len < 2` with wrong `DamagedFile` code; full `fs::read` of all PDFs first) | **MISSING** (no file count, total bytes, page sum, or batch memory check) | **MISSING** (`main.rs:74-93, 839`: `uniquify_path` + `fs::write`) | **MISSING** (no post-write PDF/page-count check, `main.rs:839-840`) | **MISSING** (`main.rs:831, 847-856, 1464`: `NeedsPassword` marked `Failed`; `merge_pdfs_with_inputs` unused) | **MISSING** (cancel absent; retry calls `process_job` which always errors at `main.rs:1351-1354`) |
| `split-pdf` | **MISSING** (`main.rs:221-223, 1303-1309`: default `"1-2, 3"` & rejects `""` vs manifest `""`, live capture) | **PARTIAL** (`main.rs:1302`: full `fs::read` first; empty range returns wrong `DamagedFile` code) | **MISSING** (`refract_core::limits` unused) | **MISSING** (`main.rs:1323-1328`: `uniquify_path` + per-part `fs::write`, leaves partial files on failure) | **MISSING** (no per-part PDF verification; empty `parts` returns `parent` dir at `main.rs:1322-1329`) | **MISSING** (`main.rs:1464-1467`: rejected by `process_job_with_password`; `split_pdf_with_password` unused) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts & duplicates outputs; cancel absent) |
| `reorder-pdf` | **MISSING** (`main.rs:224-226, 1333-1339`: default `"1, 2"` vs `""`, wrong `DamagedFile` code, live capture) | **PARTIAL** (`main.rs:1332`: full `fs::read` first; engine validates order after read) | **MISSING** (`refract_core::limits` unused) | **MISSING** (`main.rs:74-93, 1348`: `uniquify_path` + `fs::write`) | **MISSING** (no post-write PDF/page-count check, `main.rs:1348-1349`) | **MISSING** (`main.rs:1464-1467`: rejected by `process_job_with_password`; `reorder_pdf_with_password` unused) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts; cancel absent) |
| `images-to-pdf` | **MISSING** (`main.rs:211-215, 884-889, 1217-1225`: live capture, silent DPI/quality fallback & clamping) | **PARTIAL** (`main.rs:892, 1216`: full `fs::read` of all images first; engine decodes afterward) | **MISSING** (no file count, pixel sum, or batch memory check) | **MISSING** (`main.rs:74-93, 903, 1233`: `uniquify_path` + `fs::write`, inconsistent suffix `"images"` vs `"images-to-pdf"`) | **MISSING** (no post-write PDF/page-count check, `main.rs:903-904, 1233-1234`) | **N/A** (`specs/tool-manifest.json:615`) | **MISSING** (cancel absent; retry runs single-file branch `main.rs:1215-1235` instead of grouped batch) |
| `pdf-to-images` | **MISSING** (`main.rs:216-220, 1238-1243, 1452`: live capture, clamped DPI, unlock hardcodes `150`/`"png"`) | **PARTIAL** (`main.rs:1237`: full `fs::read` first; engine validates after read) | **MISSING** (no PDF size/page/render memory check) | **MISSING** (`main.rs:1258-1284, 1454-1461`: `uniquify_path` + per-page `fs::write`, leaves partial pages) | **MISSING** (no per-page image verification; synthetic `first_written` at `main.rs:1257, 1453`) | **PARTIAL** (`main.rs:1430-1463`: unlocks, but ignores opts, skips fallback, no `SecretPassword`, no `is_busy` guard) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts & duplicates pages; cancel absent) |
| `compress-pdf` | **MISSING** (`main.rs:617-627, 746, 1291`: no tool opts, but output destination read live at Run/Unlock) | **PARTIAL** (`main.rs:1289, 1418`: full `fs::read` first; engine validates after read) | **MISSING** (`refract_core::limits` unused) | **MISSING** (`main.rs:74-93, 1298, 1427`: `uniquify_path` + `fs::write`) | **MISSING** (no post-write PDF/page-count check, `main.rs:1298-1299, 1427-1428`) | **PARTIAL** (`main.rs:646-672, 1417-1429`: unlocks & handles `WrongPassword`, but plain `SharedString`, no `is_busy` guard) | **PARTIAL** (retry at `main.rs:1000-1092`; cancel absent) |
| `convert-audio` | **MISSING** (`main.rs:227-230, 1362-1368`: live capture, silent bitrate fallback/clamp) | **PARTIAL** (`main.rs:1356-1361`: checks `ffmpeg` with wrong `EngineFailed` code; no file existence/size/format check) | **MISSING** (`refract_core::limits` unused) | **PARTIAL** (engine `audio.rs:368-441, 495-518` uses exclusive `TempPath` + `persist_noclobber`; Desktop `main.rs:1369-1375` uses `uniquify_path` with pre-spawn TOCTOU) | **MISSING** (no post-write magic-byte or duration verification, `main.rs:1376-1377`) | **N/A** (`specs/tool-manifest.json:811`) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts; cancel absent despite `convert_audio_cancellable`) |
| `trim-audio` | **MISSING** (`main.rs:231-234, 1398-1400`: default `end = "30"` vs `0`, ignores manifest `bitrate`, live capture) | **PARTIAL** (`main.rs:1380-1390`: checks `ffmpeg` only at Desktop layer with `EngineFailed`, trusts filename ext; engine checks `ffprobe` & duration) | **MISSING** (`refract_core::limits` unused) | **PARTIAL** (engine `audio.rs:368-441, 552-587` uses exclusive `TempPath` + `persist_noclobber`; Desktop `main.rs:1391-1397` uses `uniquify_path` with pre-spawn TOCTOU) | **MISSING** (no post-write magic-byte or trimmed duration verification, `main.rs:1400-1401`) | **N/A** (`specs/tool-manifest.json:897`) | **PARTIAL** (retry at `main.rs:1000-1092` recaptures live opts; cancel absent despite `trim_audio_cancellable`) |

---

## 3. Confirmed Defects & Risks (Prioritized by Severity)

### Critical
1. **Silent user-file overwrite & TOCTOU clobber on 9/11 tools** (`main.rs:74-93, 839, 903, 1184..1457`): `uniquify_path` returns the original existing path if suffix attempts are exhausted, and `std::fs::write` races with concurrent file creation and writes un-staged partial data directly to the final destination.
2. **Unbounded memory reads before limit/format validation** (`main.rs:827, 892, 1169..1431`): All 9 image/PDF tools call `std::fs::read` on unvalidated paths without `fs::metadata` size checks, header probing, or `refract_core::limits::{check_limits, check_memory}`, risking OOM crashes on oversized inputs or large batches.
3. **Concurrent worker race on password unlock** (`main.rs:612-681`): `on_password_entered` ignores `is_busy` and spawns an unsynchronized thread that writes `is_busy` and `jobs_store` concurrently with "Process all" or "Retry" threads.

### High
4. **Live UI option mutation & cross-tool option corruption** (`main.rs:684-720, 746, 1022, 1452`): Because options are not snapshotted at enqueue time, switching tools before clicking "Process all", changing options before "Retry", or unlocking a locked `pdf-to-images` file silently applies wrong or hardcoded options.
5. **Zero cancellation wiring in Desktop** (`main.rs`, `app.slint`): Users cannot cancel active jobs or batches, and no `CancelToken` is passed into Stage 2.2's cancellable engine APIs.
6. **Broken batch retry & missing password support for 3/5 PDF tools** (`main.rs:847-856, 1050, 1215-1235, 1351-1354, 1464-1467`): Retrying `merge-pdf` always errors; retrying `images-to-pdf` drops the batch to 1 file; locked `merge-pdf`, `split-pdf`, and `reorder-pdf` cannot be unlocked in Desktop despite engine support.
7. **Zero output verification & partial multi-output orphans** (`main.rs:1255-1286, 1322-1329`): Corrupted/empty outputs can be marked `Done`, and failures mid-way through `split-pdf` or `pdf-to-images` leave partial files in the user's output folder.

### Medium
8. **State machine & error-code drift** (`main.rs:26-33, 821, 1306, 1336, 1358, 1382`): Desktop uses a custom 5-state `JobStatus` instead of `refract_core::job::JobState` (losing `Probing`, `Verifying`, `Rejected`, `Canceled`), and maps invalid options / missing dependencies to `DamagedFile` or `EngineFailed`.
9. **Manifest default & naming drift** (`main.rs:199-236, 1141-1160`): Hardcoded defaults (`resize-image` `1920`, `split-pdf` `"1-2, 3"`, `trim-audio` `"30"`, missing `trim-audio` bitrate) and ad-hoc filenames (`"{stem} {suffix}.{ext}"` + `(1)`) diverge from `specs/tool-manifest.json` and `refract_core::naming`.
10. **Plaintext password transport & `SecretPassword` unused** (`main.rs:612-651`): Passwords flow as plain `SharedString`/`&str` instead of `SecretPassword` in an isolated secret store.
11. **Queue mutation during active run** (`main.rs:1094-1139`): `remove_job` and `move_job_up`/`down` do not guard against mutating rows belonging to an active batch.

### Low
12. **`trim-audio` extension detection from filename** (`main.rs:1386-1390`): Disguised extensions (e.g., WAV saved as `.mp3`) pick the wrong output extension instead of using `refract_core::detect`.

---

## 4. Proposed Minimal Architecture

Introduce a focused, GUI-decoupled `pipeline` module inside `apps/desktop/src/pipeline/` plus one shared persistence helper in `crates/refract-engine/src/persist.rs`. Keep Slint (`apps/desktop/src/main.rs`) as a thin UI adapter that constructs immutable requests, forwards commands to the pipeline controller, and renders `JobState` snapshots.

### 4.1 Module Layout & Justification

| Component | Location | Responsibility | Why Justified |
| --- | --- | --- | --- |
| **Shared no-clobber staging** | `crates/refract-engine/src/persist.rs` | `StagedFile` (exclusive sibling `TempPath` preserving `.ext`), `persist_noclobber_unique` (atomic `persist_noclobber` with bounded canonical `(2), (3)...` suffix retry on `AlreadyExists`), `StagedOutputBatch` (transactional multi-file commit + RAII rollback of all unfinalized/failed outputs). | `refract-engine` already depends on `tempfile = "3.16"` (zero new deps). Eliminates duplicate temp/no-clobber logic across `audio.rs` and all 9 Desktop file-writing tools. |
| **Typed options & snapshots** | `apps/desktop/src/pipeline/options.rs` | `DesktopToolOptions` enum (one variant per `ToolId`, manifest-backed `default_for(ToolId)`, strict `validate()`), `OutputDestination`, and immutable `JobRequest` snapshot captured at enqueue time. | Eliminates `CapturedOptions` slot collision, mixed-tool corruption, live retry mutation, and password-unlock option loss. |
| **Metadata-first validation & limits** | `apps/desktop/src/pipeline/validate.rs` | `validate_inputs(&JobRequest, &CancelToken) -> Result<ValidatedInputs, RefractError>`: `fs::metadata` existence/regular-file/empty/size check -> bounded 1 KB header sniff (`refract_core::detect`) -> accepted-format check -> lightweight header probe (`native::probe_image`, `pdf::probe_pdf` only after size check, `audio::detect_audio_dependencies` + `probe_duration_cancellable`) -> `refract_core::limits::{check_limits, check_memory}` including aggregate batch file count, bytes, pixels, pages, and estimated RAM. | Prevents OOM reads and enforces identical limits across "Process all", "Retry", and "Unlock" entry points. |
| **Output verification** | `apps/desktop/src/pipeline/verify.rs` | `verify_staged_outputs(&[StagedOutput], &VerificationExpectation, &CancelToken) -> Result<(), RefractError>`: verifies existence, non-zero size, magic-byte format, JPEG EOI + no-EXIF, image dimensions / `target_kb`, PDF page counts, or audio duration (within tolerance) **before** final commit and `JobState::Saved`. | Guarantees corrupt/empty/wrong-format outputs fail with `ErrorCode::VerificationFailed` and never pollute the destination directory. |
| **Isolated secret store** | `apps/desktop/src/pipeline/secrets.rs` | `JobSecretStore` (`HashMap<SecretKey, SecretPassword>` keyed by `(JobId, input_index)`), completely separate from `JobRequest` / `JobRecord` / `Settings` (non-`Serialize`, redacted `Debug`), supporting per-input passwords for `merge-pdf` and automatic `clear()` on job terminal transition or removal. | Integrates existing `pdf::SecretPassword` safely across all 5 PDF tools without leaking into job snapshots, logs, or error strings. |
| **Controller, policy & runner** | `apps/desktop/src/pipeline/runner.rs` | Single serialized job-pipeline controller enforcing `refract_core::job::JobState::can_transition()`, `is_retryable(ErrorCode)`, grouped batch execution units (`merge-pdf`, `images-to-pdf` preserving per-file queue rows), per-job/batch `CancelToken`, fresh execution attempts on retry with immutable `JobRequest`, and serialized password-unlock resumption (`NeedsPassword -> Probing`). | Fixes the `is_busy`/password race, enables real cancellation across all 11 tools, fixes batch retry, and cleanly decouples execution from Slint for fast deterministic unit tests. |

### 4.2 Preserved Invariants
- Canonical `specs/tool-manifest.json` and generated Rust/TS bindings remain the single source of truth (no silent manifest changes).
- Existing 9-state `refract_core::job::JobState` transition matrix (`job.rs:31-56`) is preserved verbatim (retries create fresh `Queued` attempts; terminal states never transition backwards).
- Existing `refract_core::cancel::CancelToken` and independent Web/Desktop engines remain untouched.
- Existing 11 tool IDs, per-file queue rows, and `↑`/`↓` reorder controls are preserved.
- Zero new Cargo dependencies required.

---

## 5. Incremental Implementation Plan

The recommended order matches code dependencies cleanly and keeps every unit small, independently testable, and reviewable:

### Unit 2.3.1 — Typed Options & Immutable Enqueue-Time Snapshots
- **Files**:
  - Add `apps/desktop/src/pipeline/mod.rs` and `apps/desktop/src/pipeline/options.rs`.
  - Update `apps/desktop/src/main.rs` (store immutable `JobRequest` / `DesktopToolOptions` + `OutputDestination` on `JobRecord` at enqueue time in `setup_pick_files`; derive `apply_tool_selection` defaults from `DesktopToolOptions::default_for(tool_id)` / manifest; pass job's own snapshot to `process_job`, `setup_retry_job`, and `process_job_with_password`).
- **Scope**: Strongly typed per-tool option enum for all 11 tools, strict pre-execution validation returning `ErrorCode::InvalidOption`, manifest-aligned defaults (`resize-image` `1200`, `split-pdf` `""`, `trim-audio` `end = "0"`, `bitrate`), immutable enqueue-time snapshots (removing `CapturedOptions` live reads during run/retry/unlock).
- **Dependencies**: None (uses existing `refract_core::tools::ToolId`, `refract_engine::{image, pdf, audio, render}` validators).
- **Tests**: Option mutation after enqueue does not affect queued job; mixed-tool queue preserves each tool's options; invalid/missing options fail with `InvalidOption`; defaults match `specs/tool-manifest.json`.
- **Verification**: `cargo fmt -p refract-desktop -- --check`, `cargo test -p refract-desktop`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **Risks & rollback**: Changing `split-pdf` empty-range behavior and `resize-image` default width aligns with the manifest; rollback is isolated to `options.rs` and `main.rs` call sites.

### Unit 2.3.2 — Metadata-First Input Validation & Manifest Limits
- **Files**:
  - Add `apps/desktop/src/pipeline/validate.rs`.
  - Update `apps/desktop/src/main.rs` (route "Process all", "Retry", and "Unlock" through `validate_inputs` before any full `std::fs::read` or subprocess launch; map `ErrorCode::is_rejection()` cleanly).
- **Scope**: `fs::metadata` checks before reading full bytes, bounded 1 KB magic-byte sniff, desktop accepted-format validation (rejecting HEIC cleanly), lightweight header probing (image dimensions via `native::probe_image`, PDF page count after size check, `audio::detect_audio_dependencies`), `refract_core::limits::{check_limits, check_memory}` for single files and aggregate batches (`merge-pdf`, `images-to-pdf`), magic-byte extension resolution for `trim-audio`.
- **Dependencies**: Unit 2.3.1 (needs `DesktopToolOptions` to compute resize/render memory estimates and validate tool-specific constraints).
- **Tests**: Oversized input rejected via metadata without reading full file (verified with sparse/oversized limit test); empty/missing/directory/disguised/HEIC inputs rejected consistently; aggregate batch file count, byte size, pixel count, page count, and memory budget enforced; missing `ffmpeg`/`ffprobe` returns `MissingDependency`.
- **Verification**: `cargo fmt --check` on touched files, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **Risks & rollback**: Tight memory-estimate formulas must match Web's (`packages/core/src/tools/tools.ts`) scaled to `DESKTOP_MEMORY_BUDGET_MB` (8192 MB) to avoid false rejections; rollback isolated to `validate.rs`.

### Unit 2.3.3 — Shared No-Clobber Output Staging & Persistence
- **Files**:
  - Add `crates/refract-engine/src/persist.rs`, export in `crates/refract-engine/src/lib.rs`.
  - Refactor `crates/refract-engine/src/audio.rs` temp allocation/persistence to reuse `persist.rs` (preserving existing public `persist_audio_temp_noclobber` signature and tests).
  - Update `apps/desktop/src/main.rs` (replace `uniquify_path` + `std::fs::write` across all 9 image/PDF tools and audio destination resolution with `persist.rs` + `refract_core::naming::output_name_for_tool`).
- **Scope**: Exclusive sibling `TempPath` preserving `.ext`, atomic `persist_noclobber` with bounded unique-name collision resolution (`(2), (3)...`) or explicit collision error, transactional multi-output `StagedOutputBatch` (`split-pdf`, `pdf-to-images`) with rollback on failure/cancellation, strict ownership so unrelated files are never deleted.
- **Dependencies**: Units 2.3.1–2.3.2 (validated options & formats determine final extensions).
- **Tests**: Existing destination collision preserves original file; TOCTOU race during persist never overwrites; suffix exhaustion errors without clobbering; failure or cancellation cleans only owned temp files and preserves unrelated files; multi-output partial failure rolls back earlier pages cleanly.
- **Verification**: `cargo fmt -p refract-core -p refract-engine -p refract-desktop -- --check`, `cargo test --workspace`, `cargo test -p refract-engine --test audio_ffmpeg_integration -- --ignored`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **Risks & rollback**: Switching Desktop output names from `"photo converted.png"` to canonical `"photo (converted).png"` updates existing `test_determine_output_path` in `main.rs`; verify explicit product agreement on Desktop naming alignment (see Section 7).

### Unit 2.3.4 — Post-Execution Output Verification
- **Files**:
  - Add `apps/desktop/src/pipeline/verify.rs`.
  - (Optional minimal helper in `crates/refract-engine/src/image.rs` if exposing `jpeg_has_complete_eoi` / EXIF-absence check cleanly avoids duplication.)
  - Update `apps/desktop/src/main.rs` (verify every staged output *before* committing final outputs and marking `Saved`; fail with `ErrorCode::VerificationFailed` and roll back staged files on mismatch).
- **Scope**: Lightweight structural verification per output kind (existence, non-zero size, magic bytes, JPEG EOI + no-EXIF, image dimensions & `compress-image` `<= target_kb`, PDF `probe_pdf` page counts, audio `ffprobe` duration check) for both single and multi-output tools.
- **Dependencies**: Unit 2.3.3 (verifying staged temp files before final `persist_noclobber` guarantees failed verifications leave zero invalid final files).
- **Tests**: Empty/corrupt/wrong-format/wrong-dimension/wrong-page-count/missing-EOI/EXIF-leaking staged outputs fail with `VerificationFailed`, prevent `Saved` state, and leave no final output file; multi-output verification rejects the whole batch if any single output fails.
- **Verification**: `cargo fmt --check` on touched files, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **Risks & rollback**: Audio duration tolerance must allow codec framing padding (`±0.25s`, or `±0.5s` for short MP3 frames) to prevent false verification failures on valid FFmpeg encodes.

### Unit 2.3.5 — Password & Secret Separation Across All 5 PDF Tools
- **Files**:
  - Add `apps/desktop/src/pipeline/secrets.rs`.
  - Update `apps/desktop/src/main.rs` (wire `JobSecretStore` with `pdf::SecretPassword`; unify locked-PDF execution for `compress-pdf`, `pdf-to-images`, `split-pdf`, `reorder-pdf`, and per-input `merge-pdf` via `merge_pdfs_with_inputs` / `render_pdf_to_images_with_password_cancellable` + embedded-image fallback; remove hardcoded `150`/`"png"` unlock path).
- **Scope**: Separate non-serializable secret storage associated by job/input key, redacted `Debug`, best-effort buffer clearing on terminal state/cancel/remove, preservation of immutable options during unlock, per-input unlock for `merge-pdf`.
- **Dependencies**: Units 2.3.1–2.3.4 (unlock must reuse the same immutable `JobRequest`, validation, staging, and verification pipeline—never a separate ad-hoc code path).
- **Tests**: `SecretPassword` and `JobSecretStore` `Debug` output never contains plaintext; secrets are cleared on completion, failure, cancellation, and job removal; all 5 PDF tools (`compress-pdf`, `pdf-to-images`, `split-pdf`, `reorder-pdf`, `merge-pdf` with mixed/distinct passwords) unlock using original immutable options and handle `WrongPassword` cleanly.
- **Verification**: `cargo fmt --check` on touched files, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **Risks & rollback**: Per-input unlock in `merge-pdf` needs the password dialog prompt to identify *which* file is locked (`"Enter password for report.pdf"`); Slint modal already accepts a `prompt: string` (`app.slint:193, 968`), so no redesign is needed.

### Unit 2.3.6 — Unified Job Controller, 9-State Machine, Cancellation & Retry Consistency
- **Files**:
  - Add `apps/desktop/src/pipeline/runner.rs`.
  - Update `apps/desktop/src/main.rs` (replace `JobStatus` with `refract_core::job::JobState`, replace ad-hoc `is_busy` + 3 separate `thread::spawn` entry points with single serialized `JobController`, wire `CancelToken` into all 11 cancellable engine calls, enforce `is_retryable(ErrorCode)` and fresh-attempt retries for both single and grouped batch jobs, guard queue mutations while active).
  - Minimal wiring in `apps/desktop/ui/app.slint` (add `cancel_job(int)` / `cancel_all()` callbacks and show Cancel button while running + Rejected/Canceled badges—strictly functional, no visual redesign).
- **Scope**: Full 9-state `JobState::can_transition()` enforcement, single serialized worker eliminating `on_password_entered` vs `start_processing` races, cooperative cancellation across all 11 tools, retryability classification, fresh attempt creation on retry with immutable `JobRequest`, grouped batch retry for `merge-pdf` and `images-to-pdf`.
- **Dependencies**: Units 2.3.1–2.3.5.
- **Tests**: All state transitions verified against `JobState::can_transition()`; pre-run, mid-run, and finalization cancellation return `Canceled` (never `Saved`) and leave no partial outputs; non-retryable rejections (`InvalidOption`, `EmptyFile`, `LimitExceeded`, `UnsupportedFormat`) transition to `Rejected` and are rejected by retry policy; retry reuses original immutable options and creates a fresh `Queued` attempt; batch retry re-runs all grouped `merge-pdf`/`images-to-pdf` members; concurrent unlock + start commands serialize cleanly without races.
- **Verification**: `cargo fmt --check` (all Rust crates), `cargo check --workspace`, `cargo test --workspace`, `cargo test -p refract-engine --test audio_ffmpeg_integration -- --ignored`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **Risks & rollback**: Largest wiring change in `apps/desktop/src/main.rs`; because Units 2.3.1–2.3.5 already moved pure logic into tested `pipeline::*` modules, `main.rs` becomes a thin, low-risk Slint bridge.

---

## 6. Deterministic Testing Strategy

All pipeline tests run headless in `cargo test` without Slint event-loop windows or timing-dependent sleeps (synchronization via channels/barriers/hooks where needed):

| Required Scenario | Deterministic Test Mechanism | Unit |
| --- | --- | --- |
| Option mutation after enqueue | Enqueue `JobRequest`, mutate simulated UI option state, execute job, assert engine received original snapshot. | 2.3.1 |
| Invalid & missing options | Pass out-of-range quality/DPI/bitrate, `0x0` resize, empty `reorder-pdf`, bad unit/format -> assert `ErrorCode::InvalidOption` before decoding. | 2.3.1 |
| Oversized input rejected before full read | Validate with low test `PlatformLimits` (or sparse/fake metadata reader) and assert `LimitExceeded` before opening/reading full file contents. | 2.3.2 |
| Aggregate batch resource limits | Pass multi-file `merge-pdf` / `images-to-pdf` whose individual files pass limits but aggregate file count, total pages, total pixels, or estimated RAM exceeds Desktop limits -> assert `LimitExceeded`. | 2.3.2 |
| Existing destination collision | Pre-create target file, run persistence -> assert original file bytes untouched and unique suffix resolved (or explicit collision error per policy). | 2.3.3 |
| TOCTOU destination collision | Inject pre-persist hook creating destination file *after* initial path resolution and *before* `persist_noclobber` -> assert original file untouched and no clobber occurs. | 2.3.3 |
| Cancellation during processing | Cancel `CancelToken` via deterministic step/encoder hook (image/PDF) and fake child script (subprocess) -> assert `JobState::Canceled`, never `Saved`. | 2.3.6 |
| Cancellation during finalization | Trigger `token.cancel()` inside post-verify / pre-commit hook -> assert `JobState::Canceled`, staged temp files removed, destination does not exist. | 2.3.3 / 2.3.6 |
| Failure cleanup | Inject engine/write failure after staging partial bytes -> assert owned temp file removed and destination absent. | 2.3.3 |
| Verification failure | Inject corrupted/truncated/wrong-page-count staged output -> assert `ErrorCode::VerificationFailed`, job ends `Failed` (never `Saved`), and final output is never committed. | 2.3.4 |
| Multi-output partial failure | In 3-page `split-pdf` / `pdf-to-images`, fail verification or persist on output #2 -> assert outputs #1–#3 are rolled back and zero partial files remain in destination directory. | 2.3.3 / 2.3.4 |
| Password redaction & clearing | Format `JobSecretStore` and `SecretPassword` via `{:?}` and error messages -> assert secret absent; assert store entry cleared on `Saved`, `Failed`, `Canceled`, and removal. | 2.3.5 |
| Retry with immutable options | Fail a retryable job (`EngineFailed`), mutate live UI options, trigger retry -> assert fresh attempt starts `Queued`, preserves original `JobRequest`, and rejects retry on `Rejected` errors. | 2.3.6 |
| No accidental overwrite/deletion of unrelated files | Place unrelated `.refract-*` and sibling user files in destination dir during failure, collision, and cancellation -> assert exact bytes preserved. | 2.3.3 |

---

## 7. Explicit Unresolved Questions & Assumptions

1. **Destination collision policy for Desktop (`persist.rs`)**:
   - *Assumption*: Desktop should resolve destination collisions atomically using canonical unique suffixes (`"photo (converted) (2).jpg"`, bounded retry up to N attempts) via `TempPath::persist_noclobber` so repeated conversions or retries never overwrite and never fail unnecessarily when a file already exists, while returning an explicit `destination_exists` error if suffix attempts are exhausted (or when a caller explicitly requests strict no-suffix `persist_noclobber`, as in `audio::convert_audio_with_binary`).
   - *Question*: Confirm whether Desktop jobs should auto-suffix on `AlreadyExists` during `persist_noclobber` (recommended for UX parity with existing Desktop `uniquify_path`) or hard-fail on any collision.
   - *Resolved (Unit 2.3.3)*: Auto-suffix. `persist::persist_unique` tries `name`, `name (2)`, ... `name (10000)` with one atomic no-clobber move per attempt (audio too, after FFmpeg finishes) and fails after that without overwriting.
2. **Canonical output filename templates on Desktop**:
   - *Finding*: Desktop currently writes `"{stem} converted.png"`, `"{stem} p0001.png"`, `"{stem}-p1-2.pdf"`, and `uniquify_path` starts at `(1)`. `refract_core::naming` uses manifest templates (`"{name} (converted).png"`, `"{name}_compressed.jpg"`, `"{name}_resized.png"`, `"{name}_merged.pdf"`, `"{name}_reordered.pdf"`, `"{name}_trimmed.wav"`) and suffixes starting at `(2)`.
   - *Question*: Confirm aligning Desktop single-file output names to `refract_core::naming::output_name_for_tool` in Unit 2.3.3 (and keeping individual files rather than ZIP for Desktop `split-pdf` and `pdf-to-images`, using `{name}-p{range}.pdf` and `{name}-p{page:0width}.{ext}`).
   - *Resolved (Unit 2.3.3)*: Yes. Desktop uses manifest names (`"photo (converted).png"`, `"doc_merged.pdf"`) and writes multi-output parts as individual files named by the manifest `entryTemplate` (`"doc-p1-2.pdf"`, `"doc-p1.png"`, no zero padding).
3. **`pdf-to-images` maximum DPI (`300` vs `600`)**:
   - *Finding*: `specs/tool-manifest.json:670` lists `max: 300`, whereas Desktop UI (`app.slint:511`) and `render::validate_render_dpi` (`render.rs:320`) allow `36..=600`.
   - *Assumption*: Preserve `36..=600` in Desktop validation (legitimate native capability, no silent manifest edit in Stage 2.3; reconcile manifest platform notes in Stage 2.5).
4. **Slint UI option controls vs Stage 2.3 scope**:
   - *Assumption*: Stage 2.3 wires typed `DesktopToolOptions` supporting all manifest options (`resize-image` `px`/`cm`/DPI/`to`, `trim-audio` `bitrate`, `images-to-pdf` `pageSize`/DPI/quality) in Rust with manifest defaults, while keeping `apps/desktop/ui/app.slint` changes strictly minimal (only functional Cancel / Rejected / Canceled wiring in Unit 2.3.6) and deferring new option controls / layout polish to Stage 2.4 / Phase 4.
5. **Desktop worker concurrency**:
   - *Assumption*: A single serialized worker thread (concurrency = 1) inside `JobController` is sufficient for Stage 2.3 and eliminates multi-worker contention on shared CPU/disk and memory budgets; bounded parallelism can be added later without changing the `JobRequest` or `JobState` contracts.
