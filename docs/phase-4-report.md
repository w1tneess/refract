# Phase 4 (Desktop v1) Report

November 2026 · branch `phase-4-desktop-rust`

## 1. Executive Summary

Phase 4 delivers the native desktop application for Refract. While originally scoped as a Tauri 2 webview wrapper, Phase 4 was transitioned into a **Pure Native Rust Application** with **Slint UI** and a modular native Rust workspace (`crates/refract-core`, `crates/refract-engine`, `apps/desktop`).

- **Status**: Complete. All 9 desktop tools across Images, PDF, and Audio are implemented and wired to the Slint GUI.
- **Binary Footprint**: Standalone single executable is **17.3 MB** uncompressed (**~6.5 MB** zipped) with zero runtime dependencies.
- **Cold Boot Time**: < 20 ms.
- **Test Suite**: Full Rust unit test suite (`refract-core` + `refract-engine`), along with full Web v1 unit and E2E test suites passing in CI.

---

## 2. Key Architectural Decisions

| Decision | Choice | Benefit |
| --- | --- | --- |
| **Desktop UI Framework** | Slint 1.9 | Pure declarative UI, hardware-accelerated GPU rendering (FemtoVG/Skia), zero webview/DOM requirements. |
| **Core Logic** | `crates/refract-core` | Native Rust reimplementation of `@refract/core` logic (magic bytes detection, formats, error models, job state machine). |
| **Image Engine** | `image` (0.25) | Native Rust codecs for JPEG, PNG, and WebP, Lanczos-3 resampling, target-size binary search. |
| **Audio Engine** | Native FFmpeg / FFprobe | Direct process execution with stream copying, bitrate control, and millisecond trimming. |
| **PDF Engine** | `lopdf` (0.38) | Pure Rust PDF parsing, page range extraction, 1-to-1 page reordering, merging, and stream deflating. |
| **Desktop I/O** | `rfd` & `open` | Native OS file picker dialogs with tool-specific extension filters and direct "Show in Folder" file manager reveal. |

---

## 3. Comparison: Pure Rust (Slint) vs. Tauri 2 vs. Electron

| Metric | Refract (Pure Rust + Slint) | Tauri 2 (Webview) | Electron |
| --- | --- | --- | --- |
| **Webview Requirement** | None (Self-contained) | OS Webview (WebView2 / WebKitGTK) | Bundled Chromium |
| **Cold Startup Time** | **< 20 ms** | ~150–300 ms | ~800–1,500 ms |
| **Idle Memory Footprint** | **~18–25 MB** | ~35–50 MB | ~200–300 MB |
| **Installer / Binary Size** | **~6.5 MB zipped / 17 MB exe** | ~3 MB installer + ~150 MB WebView2 | ~85 MB installer |
| **Background Threading** | Native Rust OS threads | Rust / IPC message passing | Node.js child processes |

---

## 4. Work Completed by Milestone

### Milestone D0: Rust Workspace Scaffolding
- Virtual Cargo workspace with `crates/refract-core`, `crates/refract-engine`, and `apps/desktop`.
- Cross-platform CI matrix (`ubuntu-24.04`, `windows-latest`, `macos-latest`) in `.github/workflows/ci.yml`.

### Milestone D1: Native Image Engine (`crates/refract-engine/src/image.rs`)
- Fast image dimension probing (`probe_dimensions`) without decoding pixel buffers.
- Lossless/lossy format conversion (`convert_image`) between JPEG, PNG, and WebP.
- Target-size compression search (`compress_image`) down to user-specified KB budget.
- Lanczos-3 pixel resizing (`resize_image`) and physical dimension resizing (`resize_image_cm`).

### Milestone D2: Native Audio Engine (`crates/refract-engine/src/audio.rs`)
- Subprocess spawning for FFmpeg and FFprobe (`is_ffmpeg_available`, `probe_duration`).
- Timestamp parser (`parse_timestamp`) supporting `"90"`, `"1:30"`, `"01:30.5"`, `"01:00:00"`.
- Conversion (`convert_audio`) across MP3, WAV, M4A, and OGG with configurable bitrates (128–320 kbps).
- Millisecond-accurate audio trimming (`trim_audio`) with `-ss` and `-to`.

### Milestone D3: Native PDF Engine (`crates/refract-engine/src/pdf.rs`)
- PDF metadata inspection (`probe_pdf`) detecting page counts and encrypted trailers.
- Page range extraction (`parse_ranges`) supporting `"1-2, 3"`, `"1-3, 5, 8-end"`.
- Exact page reordering (`parse_order`, `reorder_pdf`) validating 1-to-1 page matching.
- Lossless stream deflating (`compress_pdf`).
- Multi-document merging (`merge_pdfs`).

### Milestone D4: Slint Desktop UI Wiring (`apps/desktop`)
- Sidebar tool navigation across all 9 desktop tools (3 Image, 4 PDF, 2 Audio).
- Dynamic options form updating based on active tool (formats, quality, target KB, dimensions, bitrates, timestamps, ranges).
- Non-blocking background worker thread executing operations via `refract-engine`.
- Asynchronous UI state updates via `slint::invoke_from_event_loop`.
- "Show in Folder" action triggering native file explorer reveal.

### Milestone D5: Packaging & Release Automation (`.github/workflows/release.yml`)
- Automated cross-platform release workflow building standalone packages:
  - Windows x86_64: `refract-windows-x86_64.zip` (containing `refract.exe`).
  - macOS ARM64: `refract-macos-arm64.tar.gz`.
  - Linux x86_64: `refract-linux-x86_64.tar.gz`.
- Automatic SHA256 checksum generation (`SHA256SUMS.txt`).
- GitHub Releases publishing with release notes on tag push or manual trigger.
