# AGENTS.md — Refract Project Rules & Context

Refract is an open-source (GPL-3.0-or-later), local-first file utility workbench with two independent local processing applications:
- **Refract Web** (`apps/web`, `packages/core`, `packages/engine-wasm`): React 19 + TypeScript + Vite web application running WebAssembly codecs inside dedicated Web Workers.
- **Refract Desktop** (`apps/desktop`, `crates/refract-core`, `crates/refract-engine`): Pure native Rust desktop application with a Slint 1.9 UI (no Electron, no Tauri, no webview).

## 1. Repository Map

| Path | Responsibility |
| --- | --- |
| `packages/core` | `@refract/core` — DOM-free TypeScript core: magic-byte detection, header probing, route planner, limits & memory budgets, 9-state job machine, output verification, tool specs. |
| `packages/engine-wasm` | `@refract/engine-wasm` — Dedicated Web Worker runtime and WASM codecs (`libheif-js`, `@jsquash/*`, `pdf-lib`, `pdfjs-dist`, `@neslinesli93/qpdf-wasm`, lazy-loaded `@ffmpeg/ffmpeg`). |
| `apps/web` | `@refract/web` — React 19 + Vite UI, batch queue, Command Palette, Optical Loupe, ZIP exports, tool chaining, and Playwright E2E suite. |
| `crates/refract-core` | Native Rust core types, magic-byte detection, error & job models, and tool metadata. |
| `crates/refract-engine` | Native Rust image (`image`), PDF (`lopdf`), PDF rasterizer (`mutool`/`pdftoppm`/`pdftocairo`/`gs`), and audio (`ffmpeg`/`ffprobe`) processing modules. |
| `apps/desktop` | Native Slint 1.9 desktop application (`refract-desktop`). |
| `fixtures/` | Shared test fixtures (valid, rotated EXIF, truncated, empty, disguised extensions, encrypted PDFs, Unicode filenames, audio samples). |
| `docs/` | Phase reports, audit reports, and research notes. |
| `specs/` | Canonical Tool Manifest (`tool-manifest.json`), JSON schema (`tool-manifest.schema.json`), generating TypeScript, Rust, and `docs/platform-matrix.md`. |

## 2. Product Domains & Preserved Tool IDs

Refract covers three domains and **11 canonical tool IDs** (preserve all IDs verbatim):
- **Images**: `convert-image`, `compress-image`, `resize-image`
- **PDF**: `merge-pdf`, `split-pdf`, `reorder-pdf`, `images-to-pdf`, `pdf-to-images`, `compress-pdf`
- **Audio**: `convert-audio`, `trim-audio`

Platform-specific differences must be explicit and honest:
- **HEIC decoding**: Supported on Web (`libheif-js` WASM); unsupported on Desktop native builds (must return a clear error).
- **Desktop external dependencies**: Audio tools require `ffmpeg` and `ffprobe` on `PATH`; `pdf-to-images` uses `mutool` / `pdftoppm` / `pdftocairo` / `gs` on `PATH` when available and falls back to embedded image extraction otherwise.

## 3. Non-Negotiable Rules

1. **Inspect before editing**: Read the relevant source files and tests before proposing or implementing changes.
2. **Evolve, do not rebuild**: Preserve all 11 existing tools, conversion algorithms, drag-and-drop, clipboard paste, auto-routing, batching, retry/cancel, password unlock, ZIP export, tool chaining, Command Palette, shortcuts, theme persistence, previews, and Optical Loupe.
3. **Strict local-first privacy**: Never upload user files to any server. Zero analytics, telemetry, ads, accounts, remote web fonts, or backend databases.
4. **Independent platform engines**: Keep `@refract/engine-wasm` and `crates/refract-engine` independent. Share product contracts, specs, limits, error semantics, and fixtures — never force WASM and native Rust into a single runtime, and never replace native Desktop with Electron/Tauri/webviews.
5. **Justify dependencies**: Do not add npm or Cargo dependencies without explaining why existing tools or standard library code are insufficient.
6. **Work in isolated, reviewable phases**: Follow the approved phase order (Phase 0 -> Phase 1 -> Phase 2 -> Phase 3 -> Phase 4 -> Phase 5 -> Phase 6). Do not begin visual redesign until Phase 2 passes its gates. Commit each completed phase separately.
7. **Honest verification**: Never claim tests or builds pass unless actually executed in the current session. Never count skipped tests as passed. Never fabricate benchmarks, features, or capabilities.
8. **Preserve tests and add regressions**: Keep existing unit, E2E, and Rust tests intact, and add regression tests for every verified bug fixed.

## 4. Design Direction (Precision Workbench — Provisional)

Target identity: a calm, technical, tactile, local-first utility workbench — distinct from generic SaaS dashboards and not a direct Apple UI clone.

Provisional palette (must pass full WCAG 2.2 AA contrast verification before locking `DESIGN.md` in Phase 3):
- **Canvas**: `#F4F3EF` (Light) / `#15191B` (Dark)
- **Surface**: `#FFFFFF` (Light) / `#202629` (Dark)
- **Raised surface**: `#ECEDE9` (Light) / `#292F32` (Dark)
- **Accent**: `#315F70` (Light; `#FFFFFF` foreground, 7.00:1) / `#80B9C4` (Dark; `#15191B` dark ink foreground, 8.14:1 — never white text on `#80B9C4`)

Required visual constraints:
- **WCAG 2.2 AA**: >= 4.5:1 for normal text, >= 3:1 for large text and interactive control borders (`control-border` must be distinct from decorative hairlines).
- **System font stack only** (no remote font requests), 4px spacing scale, restrained radii (8–14px), mostly solid layered surfaces with hairline borders, short interruptible motion respecting `prefers-reduced-motion`, consistent line icons (never emoji).
- **Banned**: Excessive glassmorphism/blur, purple AI gradients, neon accents, floating orbs, decorative fake charts/waveforms, oversized card clutter, uppercase tracked labels, marketing filler copy.

## 5. Verification Commands

Run the relevant subset after every phase and the full suite before release:

```bash
# Web & TypeScript
pnpm typecheck
pnpm test
pnpm --filter @refract/web build
pnpm --filter @refract/web e2e

# Native Rust & Desktop
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For full QA checklists, edge-case testing, and visual viewport audits, follow `.agents/skills/refract-qa/SKILL.md`. For detailed structural invariants, follow `.agents/rules/architecture.md`.
