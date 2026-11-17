# Refract

An open-source, local-first file converter: a web app (WASM) and a pure native Rust desktop app (Slint UI).
Files are converted entirely on your own machine. No uploads, no accounts, no ads, no telemetry.

**Status**: Phase 3 (Web v1) & Phase 4 (Desktop v1) complete. See [docs/phase-3-report.md](docs/phase-3-report.md) & [docs/phase-4-report.md](docs/phase-4-report.md).

**Licence**: GPL-3.0-or-later (see [LICENSE](LICENSE)).

---

## Supported Tools & Formats (Web v1)

Refract Web v1 runs 12 dedicated file conversion and transformation tools across three primary domains:

### 🖼️ Images
- **Convert image**: Convert between HEIC, JPEG, PNG, and WebP with EXIF orientation baking and removal.
- **Compress image**: Target-size compression (in KB) using binary quality search across JPEG, PNG, and WebP.
- **Resize image**: Lanczos-3 resampling by pixel dimensions or physical size (cm + DPI), with aspect ratio preservation.

### 📄 PDF
- **Merge PDF**: Combine multiple PDFs into a single document in custom drop order.
- **Split PDF**: Split documents by page numbers or ranges (e.g. `1-3, 5, 8-end`) into a downloadable ZIP.
- **Reorder PDF**: Reorder pages with strict sequence validation.
- **Images to PDF**: Convert image files (JPEG, PNG, WebP) into multi-page PDF documents.
- **PDF to images**: Render PDF pages into full-resolution PNG or JPEG images bundled in a ZIP.
- **Compress PDF**: Lossless linearization and stream compression powered by QPDF WebAssembly.
- **Password Support**: Built-in support for encrypted PDFs (user password unlock and owner-only bypass).

### 🎵 Audio
- **Convert audio**: Transcode between MP3, WAV, M4A, and OGG with customizable bitrates (64–320 kbps).
- **Trim audio**: Fast audio trimming with exact timestamp boundaries (`mm:ss` or seconds).
- **Lazy Load**: Heavy FFmpeg WASM binaries are loaded on demand only when an audio job is scheduled.

---

## Key Architecture Principles

1. **Local-Only Web Workers**: All file parsing, decoding, and encoding happens in Web Workers via `@refract/engine-wasm`. The main thread never blocks, and files never leave the browser.
2. **DOM-Free Core (`@refract/core`)**: Business logic, magic byte detection, format routing, memory heuristics, and job state machines are 100% DOM-free and reusable by the upcoming desktop app.
3. **Verified Outputs**: Output files are probed and validated before marking jobs as complete (e.g. verifying valid headers, dimensions, and decodability).
4. **Resilient UX**:
   - Multi-file batch queue with concurrency limiting (2 concurrent jobs).
   - "Use in another tool" pipeline to chain operations without re-uploading.
   - "Save all as ZIP" for batch downloads.
   - Per-tool remembered settings with "Reset to defaults".
   - Clear, user-friendly error diagnostics with technical details available on demand.

---

## Getting Started

### Prerequisites
- Node.js ≥ 20
- pnpm ≥ 9
- Rust ≥ 1.80 (for native desktop app)

### Development & Testing

```bash
# Install dependencies
pnpm install

# Run type checks across all workspaces
pnpm typecheck

# Run unit tests
pnpm test

# Start the web app in dev mode
pnpm --filter @refract/web dev

# Build the production bundle
pnpm --filter @refract/web build

# Run Playwright E2E tests (Chromium & Firefox)
pnpm --filter @refract/web e2e

# Run Rust desktop tests & checks
pnpm desktop:check
pnpm desktop:test

# Start the desktop app in dev mode
pnpm desktop:dev
```
