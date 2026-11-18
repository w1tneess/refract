# Codebase Audit & Remediation Report

Date: 2026-11-18

This document records the full-repository audit of Refract (`@refract/core`, `@refract/engine-wasm`, `@refract/web`, `crates/refract-core`, `crates/refract-engine`, `apps/desktop`, deployment configs, and documentation) and every fix applied.

---

## 1. Critical Functional & Data-Correctness Fixes

| Area | File(s) | Flaw Found | Fix Applied |
| --- | --- | --- | --- |
| **Desktop Audio** | `apps/desktop/src/main.rs` | `map_format_str` omitted `"mp3"`, causing the default `"Convert audio"` → MP3 target to fall through to `Format::Jpeg` and invoke `ffmpeg` with a `.jpg` output path. | Added `"mp3" => Format::Mp3` in `map_format_str`. |
| **Desktop PDF Merge** | `crates/refract-engine/src/pdf.rs`, `apps/desktop/src/main.rs` | `refract_engine::pdf::merge_pdfs` only copied raw `/Page` dictionaries without renumbering objects, copying resources/streams, or updating `/Pages` `/Kids` & `/Count` (returning only the first PDF's pages). In addition, `apps/desktop` called `compress_pdf` individually instead of `merge_pdfs`. | Implemented full `lopdf` multi-document merge with object ID renumbering, inherited page attribute propagation (`/MediaBox`, `/CropBox`, `/Resources`, `/Rotate`), unified `/Pages` root, and multi-file `"Merge PDF"` job grouping in `apps/desktop`. |
| **Desktop Image Codecs** | `crates/refract-engine/src/image.rs`, `apps/desktop/src/main.rs` | (1) EXIF orientation was never applied on decode, leaving rotated photos sideways; (2) converting RGBA images to JPEG crashed with `Rgba8` unsupported; (3) `resize_image` always encoded as JPEG even for PNG/WebP; (4) `compress_image` stopped at quality 10 without scaling down when oversized. | Applied `ImageDecoder::orientation()` + `apply_orientation()` on decode & probe; added white-background RGBA flattening before JPEG encoding; added `resize_image_with_format` preserving PNG/WebP/JPEG; added scale-down fallback matching `@refract/core`'s `searchTargetSize`. |
| **Web Images-to-PDF** | `packages/engine-wasm/src/codecs/pdf.ts` | `pdf.fromImages` ignored the `orientations` option passed by `@refract/core`, embedding rotated JPEGs sideways with raw EXIF still present. | Decoded JPEGs with `orientation !== 1` through `decode()` (baking upright pixels) and re-encoded EXIF-free upright JPEGs before embedding. |
| **Core Encrypted PDFs** | `packages/core/src/jobs/run.ts` | When `engine.inspect` couldn't read page counts on an encrypted PDF before unlock (`pages: undefined`), `runJob` never re-inspected the unlocked PDF, leaving `p.pages` undefined and bypassing `maxPages` limits. | Re-inspected unlocked PDFs whenever `got.pages === undefined` before checking limits and planning steps. |
| **Desktop PDF Split** | `crates/refract-engine/src/pdf.rs` | `split_pdf` cloned the entire PDF per group without pruning unreferenced pages/resources, bloating every split file with the entire source document. | Called `prune_objects()` and `renumber_objects()` on each split output; added support for `8-`, `8-end`, and empty range (split every page) to match `@refract/core`. |
| **Web & Core Output Naming** | `packages/core/src/jobs/run.ts`, `packages/core/src/tools/tools.ts`, `packages/core/src/detect/detect.ts` | (1) Mismatched input filenames (e.g. `really-png.jpg`) kept the wrong extension on `resize-image` (`to: 'same'`) and `trim-audio`; (2) duplicate split ranges (`1-2, 1-2`) created colliding ZIP entry names; (3) `formatFromName("png")` matched extensionless filenames. | Reconciled single-output extension against detected output format before `Saved`; deduplicated `split-pdf` ZIP entry names via `outputName`; required `dot > 0` in `formatFromName`. |

---

## 2. Safety, Security, Reliability & Resource-Leak Fixes

| Area | File(s) | Flaw Found | Fix Applied |
| --- | --- | --- | --- |
| **Rust Magic-Byte Detection** | `crates/refract-core/src/detect.rs` | (1) `ftyp` compatible-brand scan looped over the entire file instead of stopping at the `ftyp` box size; (2) MP3 sync check matched ADTS AAC frames; (3) missing `M4B `, `iso2`, `dash` brands. | Bounded compatible-brand scan to `ftyp` box size; required non-zero MPEG layer bits (`((b[1] >> 1) & 3) != 0`); added missing M4A brands. |
| **Core Header Probing & Verification** | `packages/core/src/detect/heif.ts`, `packages/core/src/detect/image.ts`, `packages/core/src/verify/verify.ts` | Missing bounds checks in `probeHeif` (`pitm`/`ipma`/`irot`), unguarded JPEG segment length `< 2` (potential infinite loop on malformed JPEG) and missing `0xff` fill/standalone marker handling in `jpegOrientation`, unchecked zero dimensions in `VP8 `, and `verifyOutputs` running `checkInspect` before checking `format === 'unknown'`. | Added strict bounds checks, `len < 2` guards, TIFF byte-order validation, zero-dimension checks, and reordered `verifyOutputs` unknown-format validation ahead of dimension/page checks. |
| **Resize Memory Budget** | `packages/core/src/tools/tools.ts` | `resize-image` memory estimation multiplied raw `width * height` without converting `cm` + `DPI` to pixels or resolving `0` aspect-ratio dimensions, allowing multi-gigabyte resize jobs past `checkMemory`. | Extracted shared `resolveResizeDimensions` helper used by both `memory()` and `plan()`. |
| **State Machine Error Handling** | `packages/core/src/jobs/run.ts` | Unexpected errors while in `Queued` or `NeedsPassword` attempted illegal transitions directly to `Failed`, throwing `Illegal job transition`. | Transitioned safely through `Probing` → `Rejected` when errors occur prior to `Running`. |
| **Web Worker Transfer & Cleanup** | `packages/engine-wasm/src/worker.ts`, `src/index.ts`, `src/codecs/pdf.ts`, `src/codecs/audio.ts` | (1) Transferring duplicate or subarray `ArrayBuffer`s could throw `DataCloneError` or detach shared WASM memory; (2) `pdf.toImages` used `cleanup()` instead of `destroy()` and rendered transparent paper; (3) `audio.ts` cached rejected `ffmpegPromise` and leaked listeners/temp files on error. | Added `detachForTransfer()` normalizing standalone unique buffers; transferred copied inputs on `inspect`/`unlock`; painted white page background & called `doc.cleanup()` + `loadingTask.destroy()` in `finally`; reset `ffmpegPromise` on failure and moved FFmpeg listener/file cleanup to `finally`. |
| **React StrictMode & Blob URL Leaks** | `apps/web/src/useJobs.ts`, `apps/web/src/App.tsx` | (1) `retry` and `reRunAll` executed side effects (`createJob`, `abort`, `start`) inside `setJobs` updaters, spawning duplicate jobs under React 19 `StrictMode`; (2) `URL.createObjectURL` URLs and `files`/`pending` refs were never revoked or cleaned up. | Refactored `retry`/`reRunAll` to read from `jobsRef` outside state updaters; revoked Blob URLs on remove/clear/retry/re-run/unmount; cleaned up refs and resolved pending password prompts on cancel/remove; avoided broken HEIC `<img>` object URLs. |
| **Rust Audio Validation** | `crates/refract-engine/src/audio.rs`, `apps/desktop/src/main.rs` | `parse_timestamp` accepted negative values, `NaN`, and `inf`; `convert_audio` passed `-b:a` even for WAV and ignored target codec; `trim_audio` didn't validate `end > start` and desktop passed `Some("")` for blank End time (`-to 0.000`). | Validated finite non-negative timestamps; set explicit codec args per format (omitting `-b:a` for WAV); validated `end > start`; passed `None` when desktop End field is blank. |
| **Desktop File Safety & Error Visibility** | `apps/desktop/src/main.rs`, `apps/desktop/ui/app.slint` | Output files could overwrite existing files; `format_size(0)` printed `1 KB`; failed job errors were never shown in Slint UI; `opt_image_quality` had no UI control; clippy warnings in `detect.rs` and `main.rs`. | Added `unique_output_path` deduplication; fixed `format_size(0)`; rendered `job.error` in failed rows; added Quality input for JPEG conversion; refactored options into `JobOptions` struct (zero clippy warnings). |

---

## 3. Web UX, Accessibility & Privacy Fixes

- **Zero External Network Requests & COEP Compatibility (`apps/web/index.html`)**: Removed unused Google Fonts `<link>` tags (`IBM Plex Sans` / `IBM Plex Mono`), fulfilling the strict local-first privacy guarantee and preventing `Cross-Origin-Embedder-Policy: require-corp` blocks.
- **Dark-Mode FOUC Fix (`apps/web/index.html`)**: Updated the pre-hydration theme script to treat saved `'system'` preference identically to unset (`!t || t === 'system'`).
- **Text Selection Contrast Fix (`apps/web/src/styles.css`)**: Changed `::selection` foreground from `var(--accent-text)` (invisible white-on-light-blue in light mode, black-on-dark-blue in dark mode) to `var(--text)`.
- **"Use in another tool" Guard (`apps/web/src/JobCard.tsx`)**: Filtered `compatibleTools` by `(TOOLS[id].minInputs ?? 1) <= outCount` so single-output PDF jobs don't offer `merge-pdf` as the one-click default and immediately fail.
- **"Try a sample file" on Merge PDF (`apps/web/src/pages/ToolWorkspacePage.tsx`, `src/sampleFiles.ts`)**: Generates `minInputs` sample files (2 PDFs for `merge-pdf`) so the sample button succeeds on every tool.
- **Clear Finished Visibility (`apps/web/src/pages/ToolWorkspacePage.tsx`)**: Shows "Clear finished" whenever any terminal job exists (including failed, rejected, or canceled jobs), not only when `savedCount > 0`.
- **Keyboard & Modal Accessibility (`apps/web/src/App.tsx`, `JobCard.tsx`, `OpticalLoupeModal.tsx`)**: Guarded `1`/`2`/`3`/`?` shortcuts against modifier keys (`Ctrl`/`Cmd`/`Alt`), `contenteditable`, and open modals; added `Escape` + `role="dialog"` to the image lightbox; synchronized `transformOrigin` during zoomed Optical Loupe drags and added Left/Right arrow scrubbing.
- **Dependency & Dev Config Hygiene (`apps/web/package.json`, `apps/web/vite.config.ts`)**: Moved runtime-imported `pdf-lib` from `devDependencies` to `dependencies`; excluded all WASM packages from Vite `optimizeDeps`.

---

## 4. Hosting & Documentation Fixes

- **`vercel.json`**: Replaced the invalid combination of `"routes"` + `"headers"` (rejected by Vercel) with `"rewrites"` + `"headers"`, and configured monorepo `buildCommand` / `outputDirectory`.
- **`netlify.toml`**: Removed invalid `conditions = {Path = ["!assets/*"]}` syntax and unsupported `{js,mjs,css}` brace expansion in header paths.
- **Repository Hygiene**: Removed duplicate root `REA-findings.md` (canonical file lives at `docs/REA-findings.md`).
- **Documentation Alignment**: Updated `README.md`, `DESIGN.md`, `apps/web/src/config/toolPresentation.ts`, `docs/phase-3-report.md`, `docs/phase-4-report.md`, and `.github/workflows/release.yml` to reflect exact tool counts (11 Web tools, 9 Desktop tools at the time of that audit; expanded to 11 Desktop tools in Desktop v1.1 — see [docs/platform-matrix.md](platform-matrix.md)), supported formats, bitrates (128–320 kbps), QPDF flags, Slint version (1.9), and design tokens.

---

## 5. Verification Summary

| Check | Command | Result |
| --- | --- | --- |
| **Lockfile integrity** | `pnpm install --frozen-lockfile` | Passed |
| **TypeScript typecheck** | `pnpm typecheck` | Passed (0 errors across all workspaces) |
| **Core unit tests** | `pnpm test` | 68 passed (3 test files) |
| **Web production build** | `pnpm --filter @refract/web build` | Passed |
| **Web E2E (Chromium & Firefox)** | `pnpm --filter @refract/web exec playwright test e2e/convert.spec.ts e2e/images.spec.ts e2e/pdf.spec.ts e2e/audio.spec.ts` | 72 passed, 2 skipped (optional `iphone.heic`), 0 failed |
| **Rust unit tests** | `cargo test --workspace` | 24 passed (`refract-core`: 9, `refract-engine`: 15) |
| **Rust linter** | `cargo clippy --workspace --all-targets -- -D warnings` | Passed (0 warnings) |

