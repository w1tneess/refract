---
name: refract-qa
description: Repeatable quality assurance, regression testing, cross-platform parity, and visual audit checklist for Refract (Web and Native Desktop). Use before committing any phase or performing final QA.
---

# Refract QA & Verification Skill

Use this skill to verify changes across Refract Web (`packages/`, `apps/web/`) and Refract Desktop (`crates/`, `apps/desktop/`). Follow every rule honestly — never mark skipped tests as passed, and never claim a check succeeded unless you executed it in the current session.

## 1. Automated Build, Lint & Test Gates

### Web & TypeScript
Run from the repository root:
```bash
pnpm typecheck
pnpm test
pnpm --filter @refract/web build
pnpm --filter @refract/web e2e
```

### Native Rust & Desktop
Run from the repository root:
```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### Manifest & Generated-File Drift (Phase 1+)
Once `specs/tool-manifest.json` and the generator script exist, verify zero drift:
```bash
pnpm manifest:check
```
Confirm generated TypeScript, Rust, and platform-matrix files match the manifest byte-for-byte.

## 2. Functional & Edge-Case Verification Matrix

Test affected tools (and all 11 tools during final QA) against `fixtures/` and synthetic edge cases:

| Category | Test Cases / Fixtures | Expected Result |
| --- | --- | --- |
| **Valid inputs** | `plain.jpg`, `plain.png`, `plain.webp`, `plain.heic` (Web), `plain.pdf`, `5pages.pdf`, `sound.mp3`, `sound.wav`, `sound.m4a`, `sound.ogg` | Output passes re-probe verification (format, dimensions, pages, duration, size). |
| **EXIF orientation** | `rotated.jpg`, `rotated.heic` (Web) | Orientation baked upright into pixels; EXIF stripped; dimensions match upright view. |
| **Corrupt / truncated** | `truncated.jpg`, `truncated.heic` | Clean `Rejected` or `Failed` error with user-readable reason + technical details; no crash or hang. |
| **Empty files** | `empty.heic`, 0-byte PDF/audio | Rejected upfront in `Probing` with clear empty-file error. |
| **Disguised extensions** | `really-png.jpg`, `really-heic.jpg` | Magic-byte detection overrides extension; notice shown; output extension matches real format. |
| **Unicode filenames** | `ছবি 😀.heic` | Safe output naming and download/save without encoding errors. |
| **Encrypted PDFs** | `locked.pdf` (user password), `owner-only.pdf` (empty password bypass) | Owner-only unlocks transparently; locked prompts `NeedsPassword`, handles wrong password retry and cancel/skip cleanly across all supported PDF tools. |
| **Multi-file & duplicate names** | `merge-pdf` (2+ files), `images-to-pdf`, duplicate filenames in same batch | Preserves drop/queue order; deduplicates output paths without overwriting existing files; enforces `minInputs` (e.g., `merge-pdf` < 2 fails cleanly). |
| **Invalid options** | `resize-image` (0x0), `split-pdf` (out-of-range), `reorder-pdf` (missing/duplicate pages), `trim-audio` (`end <= start` or `start >= duration`) | Rejected cleanly with actionable message; no engine crash. |
| **Cancellation** | Cancel queued job, cancel active job, cancel while `NeedsPassword` | Job transitions to `Canceled`; worker/subprocess terminates; no orphaned temp files or leaked Blob URLs. |
| **Retry & Re-run** | Retry failed/canceled job; switch tool then retry earlier job | Preserves original job's files and snapshotted options; produces clean output. |
| **Platform boundaries** | HEIC on Desktop; missing `ffmpeg` or PDF rasterizer on Desktop; `pdf-to-images` JPEG + PNG across rasterizers | Explicit, accurate status/error; unaffected tools remain usable. |
| **Resource cleanup** | Clear/remove jobs, unmount, cancel mid-write | No leftover temp sibling files on Desktop; Blob URLs revoked on Web. |

## 3. Visual, Responsive & Accessibility Audit

Inspect both **Light** and **Dark** themes. Capture visual evidence (screenshots) for UI phases.

### Web Viewports
Serve the built app (`pnpm --filter @refract/web preview` or Playwright) and inspect:
- **Mobile**: `390x844` — single-column layout, <= 5 bottom nav destinations, >= 44px interactive targets, safe-area support, no horizontal overflow, no hover-only actions.
- **Tablet**: `768x1024` — collapsible navigation, clean settings layout, no cramped columns.
- **Desktop**: `1440x900` — topbar, sidebar, workspace, inspector, previews/loupe/timeline/thumbnails.

### Desktop Window Sizes
Launch `cargo run -p refract-desktop` and verify:
- **Minimum**: `820x580` — sidebar collapses / inspector moves to drawer cleanly; zero clipped controls or overlapping text.
- **Default**: `1080x740` — balanced workbench layout, readable queue rows, clear status bar.
- **Large**: `1440x900` — clean spacing without awkward stretching.

### UI & Accessibility Checklist
- **Contrast (WCAG 2.2 AA)**: >= 4.5:1 for normal text (including text on accent fills and soft tints), >= 3:1 for large text and interactive control borders (`input`, `select`, `button`, `checkbox`).
- **Keyboard & Focus**: Tab order logical, `:focus-visible` outlines clear, dialogs trap focus and close on `Escape`, global shortcuts ignored while typing in inputs.
- **States**: Empty states, loading/progress states, `NeedsPassword` modal, error states (with technical details toggle), notices, and reduced-motion (`prefers-reduced-motion: reduce`).
- **Design Integrity**: No emoji icons, no remote font requests (verify Network tab shows 0 external requests), no decorative fake progress/waveforms, no excessive glassmorphism or purple/neon gradients.

## 4. Required QA Report Format

Every QA pass must conclude with an honest structured report containing:
1. **Checks executed** (exact commands run).
2. **Passed checks** (with exact counts).
3. **Failed checks** (with error details and fixes applied, if any).
4. **Skipped checks** (explicitly separated; never counted as passed, with reason).
5. **Remaining limitations / known gaps** (e.g., optional `iphone.heic` fixture, unavailable local rasterizers).
6. **Visual evidence** (screenshot paths or window inspection notes for UI phases).
7. **Changed files** (linked paths).
