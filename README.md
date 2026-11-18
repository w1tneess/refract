# Refract

An open-source, local-first file converter: a web app (WASM) and a pure native Rust desktop app (Slint UI).
Files are converted entirely on your own machine. No uploads, no accounts, no ads, no telemetry.

**Status**: Phase 3 (Web v1) & Phase 4 (Desktop v1) complete. See [docs/phase-3-report.md](docs/phase-3-report.md) & [docs/phase-4-report.md](docs/phase-4-report.md).

**Licence**: GPL-3.0-or-later (see [LICENSE](LICENSE)).

---

## UI Design (Web v1)

Refract Web uses an **Apple-inspired interface** — calm, exact, and quiet:

- **System materials** — translucent surfaces with `backdrop-filter: blur`, hairline borders, and soft neutral shadows. Sidebar, topbar, and panels read as frosted glass layers that content scrolls underneath.
- **SF Pro typography** — system font with optical sizing. Hierarchy from weight and size (400/500 only), tight tracking on large display text, sentence case everywhere.
- **Critically damped motion** — springs with no overshoot by default; press feedback on pointer-down (scale 0.97); all animation interruptible and gated behind `prefers-reduced-motion`.
- **Single accent** — Apple blue (`#0071e3`) used only for the primary action and live-job indicator. Everything else is graphite neutral.
- **One signature** — a single amber dot marks a job that is still developing; nothing else competes for attention.

## Supported Tools & Formats

All 11 canonical tools, option contracts, limits, and platform differences are defined in [specs/tool-manifest.json](specs/tool-manifest.json) and summarized in the generated [Platform Support & Capabilities Matrix](docs/platform-matrix.md).

### Web App (11 tools via WebAssembly)

Refract Web runs 11 dedicated file conversion and transformation tools across three domains:

#### Images
- **Convert image**: Convert between HEIC, JPEG, PNG, and WebP with EXIF orientation baking and metadata removal.
- **Compress image**: Target-size compression (in KB) using binary quality search with automatic scale-down fallback, outputting JPEG or WebP (accepts HEIC, JPEG, PNG, WebP).
- **Resize image**: Lanczos-3 resampling by pixel dimensions or physical size (cm + DPI), with aspect ratio preservation.

#### PDF
- **Merge PDFs**: Combine 2+ PDFs into a single document in drop order.
- **Split PDF**: Split documents into every page or by custom ranges (e.g. `1-3, 5, 8-` or `8-end`) into a downloadable ZIP.
- **Reorder pages**: Reorder pages with strict 1-to-1 sequence validation.
- **Images to PDF**: Compile HEIC, JPEG, PNG, and WebP images (with EXIF orientation baked upright) into A4, Letter, or image-sized PDF pages.
- **PDF to images**: Render PDF pages onto white backgrounds into full-resolution PNG or JPEG images bundled in a ZIP.
- **Compress PDF**: Lossless Flate stream re-compression and compressed object streams powered by QPDF WebAssembly.
- **Password Support**: Built-in support for encrypted PDFs (user password unlock and transparent owner-only bypass).

#### Audio
- **Convert audio**: Transcode between MP3, WAV, M4A, and OGG with configurable bitrates (128–320 kbps).
- **Trim audio**: Trim audio clips with exact start and end timestamps (in seconds).
- **Lazy Load**: Heavy FFmpeg WASM binaries (~32 MB) are loaded on demand only when an audio job is scheduled.

### Native Desktop App (11 tools via Rust + Slint UI)

Refract Desktop (`apps/desktop`) runs as a standalone native executable with no webview, supporting all 11 tools with Light/Dark theme persistence, custom output folders, batch queue management, and password-protected PDF unlocking:
- **Images**: Convert image (JPEG/PNG/WebP with EXIF orientation baking and RGBA-to-white flattening), Compress image (target KB with scale-down fallback), Resize image (Lanczos-3 resampling preserving format).
- **PDF**: Merge PDF, Split PDF (range extraction with object pruning), Reorder PDF, Images to PDF (configurable DPI & quality), PDF to images (external rasterizer chain `mutool` / `pdftoppm` / `pdftocairo` / `ghostscript` with embedded-image fallback), Compress PDF (via `lopdf`, with password unlock).
- **Audio**: Convert audio and Trim audio (with `mm:ss` / `hh:mm:ss` / seconds timestamps) via system `ffmpeg` / `ffprobe`.

---

## Workspace Structure

| Path | Description |
| --- | --- |
| `specs/` | Canonical Tool Manifest (`tool-manifest.json`) and JSON Schema (`tool-manifest.schema.json`), generating TypeScript, Rust, and `docs/platform-matrix.md`. |
| `packages/core` | `@refract/core` — DOM-free TypeScript pipeline: magic-byte detection, HEIF/JPEG/PNG/WebP header probing, route planner, limits & memory budgets, job state machine, output verification. |
| `packages/engine-wasm` | `@refract/engine-wasm` — Dedicated Web Worker codecs (`libheif-js`, `@jsquash/*`, `pdf-lib`, `pdfjs-dist`, `qpdf-wasm`, `@ffmpeg/ffmpeg`). |
| `apps/web` | `@refract/web` — React 19 + Vite web UI with Command Palette, Optical Loupe comparison modal, batch queue, and Playwright E2E suite. |
| `crates/refract-core` | Native Rust core types, magic-byte detection, error & job models. |
| `crates/refract-engine` | Native Rust image (`image`), PDF (`lopdf`), and audio (`ffmpeg`/`ffprobe`) processing engine. |
| `apps/desktop` | Native Slint 1.9 desktop application. |

---

## Key Architecture Principles

1. **Local-Only Processing**: All web decoding and encoding runs inside dedicated Web Workers via `@refract/engine-wasm`, and the desktop app runs natively via `refract-engine`. Zero external network requests (no web fonts, no analytics, no uploads).
2. **DOM-Free Core (`@refract/core` & `refract-core`)**: Business logic, magic-byte detection, format routing, memory heuristics, and job state machines are platform-agnostic and unit-tested independently.
3. **Verified Outputs**: Output files are probed and validated before marking jobs as complete (verifying headers, EXIF stripping, dimensions, page counts, and decodability).
4. **Resilient UX**:
   - Multi-file batch queue with concurrency limiting (2 concurrent jobs on Web).
   - "Use in another tool" pipeline to chain compatible operations without re-uploading.
   - "Save all as ZIP" for batch downloads with automatic Blob URL lifecycle management.
   - Per-tool remembered settings with "Reset to defaults".
   - Clear, user-friendly error diagnostics with technical details available on demand.

For the full codebase audit and fixes changelog, see [docs/audit-report.md](docs/audit-report.md).

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

# Validate canonical tool manifest and check for generated-file drift
pnpm manifest:check

# Run type checks across all workspaces
pnpm typecheck

# Run unit tests
pnpm test

# Start the web app in dev mode (localhost)
pnpm --filter @refract/web dev

# Build the production bundle (static output in apps/web/dist)
pnpm --filter @refract/web build

# Preview the production build locally
pnpm --filter @refract/web preview

# Run Playwright E2E tests (Chromium & Firefox)
pnpm --filter @refract/web e2e

# Run Rust desktop tests & checks
pnpm desktop:check
pnpm desktop:test

# Start the desktop app in dev mode
pnpm desktop:dev
```

### Hosting the Web App (Deploy to the Web)

Refract Web is a **purely client-side static site** — no backend, no server, no database. All conversion happens in the browser via WebAssembly. You can host it on any static hosting platform (Cloudflare Workers Static Assets, Vercel, Netlify, GitHub Pages, etc.).

> **WASM size optimization**: During `vite build`, oversized WASM bundles (`ffmpeg-core.wasm`, ~31 MiB) are automatically gzip-compressed to `.wasm.gz` (~9.8 MiB) so every file stays well under Cloudflare's 25 MiB per-asset limit and downloads 3× faster. The audio worker transparently decompresses `.wasm.gz` in the browser via `DecompressionStream`.

#### Cloudflare Workers (Static Assets)

`apps/web/cloudflare.config.ts` and `apps/web/public/_headers` configure Refract Web as an assets-only Cloudflare Worker with SPA fallback (`single-page-application`), COOP/COEP cross-origin isolation headers, `application/wasm` MIME type, and immutable caching for hashed assets:

```bash
# Build the production bundle
pnpm --filter @refract/web build

# Deploy to Cloudflare Workers
pnpm --filter @refract/web deploy:cloudflare
```

#### Quick deploy (Vercel)

```bash
# Install Vercel CLI
npm i -g vercel

# Deploy to production from the repo root
vercel --prod
```

The `vercel.json` at the repo root configures:
- **Monorepo build** — builds `@refract/web` and serves `apps/web/dist`
- **SPA rewrite** — non-asset routes rewrite to `/index.html`
- **WASM MIME type** — `.wasm` files are served as `application/wasm` (required for WebAssembly to compile)
- **Long-term caching** for hashed JS/MJS/CSS/WASM assets and COOP/COEP isolation headers

#### Netlify

The `netlify.toml` at the repo root configures the build command, publish directory (`dist`), WASM headers, and SPA redirects. Connect your repo and deploy — or run:

```bash
netlify deploy --prod --dir=apps/web/dist
```

#### GitHub Pages (subpath hosting)

GitHub Pages serves user/organization sites under a path prefix (e.g. `https://user.github.io/repo/`). Set the base path so all asset URLs resolve correctly:

```bash
# Create a production env file
echo "VITE_BASE=/repo-name/" > apps/web/.env.production

# Build and deploy the dist folder
pnpm --filter @refract/web build
```

Then push `apps/web/dist` to your `gh-pages` branch or use your preferred Pages deployment method.

#### Important: WASM MIME type

Whatever host you choose, **`.wasm` files must be served with `Content-Type: application/wasm`**. Without this, WebAssembly compilation will fail and image/audio/PDF tools will not work. `apps/web/public/_headers` (Cloudflare), `vercel.json`, and `netlify.toml` all include this header.

### Desktop External Dependencies

| Feature | Dependency | Notes |
| --- | --- | --- |
| Audio convert/trim | **FFmpeg** (`ffmpeg` + `ffprobe` in PATH) | Required for audio tools |
| PDF → images (render) | `mutool` / `pdftoppm` / `pdftocairo` / `ghostscript` | First available in PATH is used; falls back to embedded image extraction |
| HEIC decode | *(web app only)* | Native build shows a clear error explaining HEIC is handled in Refract Web |
