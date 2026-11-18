# Phase 3 (Web v1) Report

November 2026 · branch `phase-3-web-v1`

## 1. Executive Summary

Phase 3 (Web v1) delivers a full, zero-server in-browser file converter across **Images**, **PDF**, and **Audio** on Refract's shared pipeline. All files remain on the user's device and are processed inside dedicated Web Workers.

- **Status**: Complete. All 11 tools across 3 domains are implemented and verified.
- **Test Suite**: 64 unit tests (`@refract/core`) and 74 Playwright E2E tests (`@refract/web`) passing across Chromium and Firefox (72 passed, 2 skipped pending manual real-phone fixtures).
- **Core Principles Maintained**:
  - Pinned dependencies ≥ 2 weeks old, all compatible with GPL-3.0.
  - `@refract/core` remains strictly DOM-free and platform-agnostic.
  - Output verification step for all lossy/critical operations.
  - Typed, actionable errors ("what, why, what to do") with raw details preserved.

---

## 2. Implemented Tools & Capabilities

### Images
| Tool | Formats | Engine / Codecs | Key Behaviors |
| --- | --- | --- | --- |
| **Convert image** | HEIC, WebP, PNG, JPEG ↔ each other | `libheif-js`, `@jsquash/jpeg`, `@jsquash/png`, `@jsquash/webp` | Preserves dimensions, handles transparency, removes EXIF tags while baking in correct visual orientation. |
| **Compress image** | HEIC, JPEG, WebP, PNG → JPEG, WebP | `@jsquash/*` | Binary search for target file size (KB) with automatic scale-down fallback (quality 10–95). |
| **Resize image** | HEIC, JPEG, WebP, PNG → JPEG, WebP, PNG | `@jsquash/resize` (Lanczos-3) | Resize by px or cm (with customizable DPI, default 300). Maintains aspect ratio if one dimension is left 0. |

- **Orientation Guarantee**: JPEG EXIF orientation (tags 1–8) is baked directly into the pixel buffer upon decode and stripped from the encoded output, eliminating browser orientation inconsistencies. For HEIC, container `irot`/`imir` boxes govern orientation without double-rotation.
- **Damaged & Truncated Files**: Corrupt files fail early with typed diagnostics (`CorruptData`) and offer a one-click Retry button.

### PDF
| Tool | Engine | Key Behaviors |
| --- | --- | --- |
| **Merge PDF** | `pdf-lib` | Combines 2+ documents in drop order, copying all pages with metadata cleanup. |
| **Split PDF** | `pdf-lib` | Extracts page ranges (`1-3, 5, 7-end`) into individual PDF files bundled into a ZIP. |
| **Reorder PDF** | `pdf-lib` | Reorders pages based on exact user list (`3, 1, 2`), strictly enforcing 1-to-1 page matching. |
| **Images to PDF** | `pdf-lib` | Embeds JPEG, PNG, WebP images as single-page PDF leaves sized to original image dimensions. |
| **PDF to images** | `pdfjs-dist` + OffscreenCanvas | Renders each PDF page at full resolution to PNG or JPEG, packaged into a ZIP archive. |
| **Compress PDF** | `@neslinesli93/qpdf-wasm` | Lossless Flate stream re-compression and compressed object streams via QPDF. |

- **Password Handling (`NeedsPassword`)**: Owner-only protected PDFs open transparently without user friction. User-locked PDFs trigger the inline password flow, supporting a "Use for all locked files" batch option and distinguishing incorrect passwords from corrupt files.

### Audio
| Tool | Formats | Engine | Key Behaviors |
| --- | --- | --- | --- |
| **Convert audio** | MP3, WAV, M4A, OGG ↔ each other | `@ffmpeg/ffmpeg` + `@ffmpeg/core` | Format conversion with configurable bitrates (128–320 kbps). |
| **Trim audio** | MP3, WAV, M4A, OGG | `@ffmpeg/ffmpeg` | Exact start and end trimming in seconds via `-ss` and `-to`. |

- **Lazy FFmpeg Instantiation**: FFmpeg WASM core (~32 MB) is loaded only when an audio job is first dispatched. Timing instrumentation measures `wasmLoadMs` distinctly during first load and reuses the worker instance for subsequent operations.

---

## 3. Core UX & Workflow Features

1. **Batch Queue & Limiter**:
   - Concurrency limit: 2 concurrent worker jobs on web to prevent UI freeze and memory thrashing.
   - Batch status indicator (`waiting`, `running`, `finished`, `partial`).
2. **Multi-Output Packaging**:
   - Operations that produce multiple files (Split PDF, PDF to Images) package results into a single `.zip` on the fly via `client-zip`.
   - "Save all as ZIP" button appears whenever 2 or more jobs in the queue complete.
3. **"Use in another tool" Pipeline**:
   - Completed jobs display a "Use in another tool" action with intelligent format compatibility filtering (e.g. converted HEIC → JPEG output can be sent directly to "Compress image" or "Resize image").
   - Automatically switches hash route, populates input files, and initiates the target job.
4. **Remembered Options**:
   - Options are persisted per tool in `localStorage`.
   - "Reset to defaults" restores default parameters for any tool with one click.
5. **Robust Watchdog & Watchdog Timeout**:
   - Watchdog tracks heartbeats across worker phases (`Probing`, `Running`, `Verifying`).
   - Includes a finishing timeout (15 s) preventing hangs during file serialization and transfer.

---

## 4. Verification & Test Summary

| Package | Framework | Test Count | Result |
| --- | --- | --- | --- |
| `@refract/core` | Vitest | 64 | 64 passed (magic byte detection, image header probing, tool option resolutions, batch logic) |
| `@refract/web` (E2E) | Playwright (Chromium & Firefox) | 74 | 72 passed, 2 skipped (optional real iPhone photos). Complete coverage of Images, PDF, Audio, and UI flows. |

---

## 5. Bundle Sizes & Performance

Production build output (`vite build`):

```
dist/index.html                                         0.48 kB │ gzip:      0.31 kB
dist/assets/index-BV9RwdmK.css                          2.22 kB │ gzip:      0.90 kB
dist/assets/index-y30bWCMA.js                         263.23 kB │ gzip:     84.18 kB

# On-Demand Codecs (Lazy loaded by Web Worker):
dist/assets/mozjpeg_dec-muSO2n8T.wasm                 166.47 kB │ gzip:     64.57 kB
dist/assets/mozjpeg_enc-DO-zoExo.wasm                 251.52 kB │ gzip:     60.44 kB
dist/assets/squoosh_png_bg-DAY7U9NW.wasm              181.08 kB │ gzip:     85.73 kB
dist/assets/webp_enc_simd-CFvKQ_80.wasm               345.58 kB │ gzip:    128.61 kB
dist/assets/squoosh_resize_bg-YY9xfwUg.wasm            34.54 kB │ gzip:     17.16 kB
dist/assets/libheif-bundle-BYjQD5aJ.js              2,039.90 kB │ gzip:    688.10 kB
dist/assets/pdf.worker.min-CIqJB0yW.mjs             1,271.91 kB │ gzip:    420.15 kB
dist/assets/qpdf-C3Giu3T4.wasm                      1,334.28 kB │ gzip:    453.06 kB
dist/assets/ffmpeg-core-CgUfceKH.wasm              32,232.41 kB │ gzip: 10,370.97 kB
```

- **Initial Load**: Only 84 kB gzipped JS is loaded upfront. No WASM module is fetched until a relevant job is queued.
- **Isolated Workers**: Engine codecs run in worker threads, keeping the main UI responsive even under 24 MP image encoding.

---

## 6. Known Risks & Trade-offs

1. **FFmpeg WASM Download Size**:
   - `@ffmpeg/core.wasm` is ~32 MB uncompressed (~10 MB gzip). This is acceptable for desktop-class browsers and is loaded strictly on-demand, but may be heavy on slow mobile networks.
2. **QPDF Instance Lifecycle**:
   - Emscripten's filesystem and global state require a fresh `QPDF()` instantiation per job to avoid state corruption across multiple operations.
3. **Lossless-Only PDF Compression**:
   - In-browser PDF compression uses QPDF, which performs stream deflating, font/object deduplication, and linearization. It does not downsample embedded high-res images (which requires Ghostscript or Canvas re-rasterization). Full image-recompression for PDFs is slated for the native desktop engine.
4. **Memory Ceilings**:
   - Image and audio decoding buffers can approach 500 MB+ for very large files. The 2-worker concurrency cap prevents out-of-memory crashes on systems with limited RAM.
