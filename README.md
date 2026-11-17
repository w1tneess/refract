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

## Supported Tools & Formats (Web v1)

Refract Web v1 runs 12 dedicated file conversion and transformation tools across three primary domains:

### Images
- **Convert image**: Convert between HEIC, JPEG, PNG, and WebP with EXIF orientation baking and removal.
- **Compress image**: Target-size compression (in KB) using binary quality search across JPEG, PNG, and WebP.
- **Resize image**: Lanczos-3 resampling by pixel dimensions or physical size (cm + DPI), with aspect ratio preservation.

### PDF
- **Merge PDF**: Combine multiple PDFs into a single document in custom drop order.
- **Split PDF**: Split documents by page numbers or ranges (e.g. `1-3, 5, 8-end`) into a downloadable ZIP.
- **Reorder PDF**: Reorder pages with strict sequence validation.
- **Images to PDF**: Convert image files (JPEG, PNG, WebP) into multi-page PDF documents.
- **PDF to images**: Render PDF pages into full-resolution PNG or JPEG images bundled in a ZIP.
- **Compress PDF**: Lossless linearization and stream compression powered by QPDF WebAssembly.
- **Password Support**: Built-in support for encrypted PDFs (user password unlock and owner-only bypass).

### Audio
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

Refract Web is a **purely client-side static site** — no backend, no server, no database. All conversion happens in the browser via WebAssembly. You can host it on any static hosting platform (Vercel, Netlify, GitHub Pages, Cloudflare Pages, etc.).

#### Quick deploy (Vercel)

```bash
# Install Vercel CLI
npm i -g vercel

# Deploy to production
cd apps/web
vercel --prod
```

The `vercel.json` at the repo root configures:
- **SPA fallback** — all routes serve `index.html`
- **WASM MIME type** — `.wasm` files are served as `application/wasm` (required for WebAssembly to compile)
- **Long-term caching** for hashed JS/CSS/WASM assets

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

Whatever host you choose, **`.wasm` files must be served with `Content-Type: application/wasm`**. Without this, WebAssembly compilation will fail and image/audio/PDF tools will not work. Both `vercel.json` and `netlify.toml` include this header.
