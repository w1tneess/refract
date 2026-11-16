# Phase 1 (Spike) report

Nov 16, 2026 · branch `phase-1-spike`

**Recommendation: continue to phase 3 (Web v1) after boards.** HEIC → JPG works end to end in a Web Worker through the real core pipeline in Chromium and Firefox, the WASM cost is small, and nothing found so far blocks the plan. One caveat: the phase-1 gate is not fully met until a real iPhone HEIC passes (see Open risks 1).

Started on Nov 16, before the brief's Park phase ends (2 Dec), at your request.

## Decisions taken

| Decision | Choice |
| --- | --- |
| UI framework | React 19.3.1 |
| Licence | GPL-3.0-or-later |
| HEIC decoder | `libheif-js@1.23.5` (LGPL-3.0; libheif 1.23, WASM bundle) |
| JPEG encoder | `@jsquash/jpeg@1.6.0` (Apache-2.0; mozjpeg), pinned, so output doesn't depend on the browser |
| Verify decoder | The browser's own `createImageBitmap` in a worker, independent of the encoder |

All direct dependencies are pinned exactly and at least two weeks old; `minimumReleaseAge: 20160` in `pnpm-workspace.yaml` enforces the same rule on the whole tree. TypeScript 7.0.2 (the native port) type-checks all three packages.

## Market check (Nov 16)

1. **OpenFile** (github.com/aniruddhaadak80/OpenFile): MIT, 18 stars, v1.2.0 on 29 Oct 2026. Web + Tauri 2 desktop (Windows/macOS/Linux) on one build, local-only, 300+ tools including HEIC → JPG. This is the closest match to Refract.
2. Its desktop app only wraps the web (WASM) build: no native FFmpeg, LibreOffice or libvips, so no Office → PDF and no large-file path. Its README says nothing about output verification.
3. **VERT** (AGPL-3.0, Svelte, WASM) is popular and local-first but web-only; video goes to a server daemon. No official desktop app.
4. **ConvertX** (AGPL-3.0) is a self-hosted *server* converter (FFmpeg, LibreOffice, Pandoc), not a local app.
5. **Verdict:** the "web + desktop on one engine" gap is now partly filled by a very young project. What remains for Refract is a desktop app with **native** engines plus the reliability layer (verify-or-fail, atomic writes, typed errors). You chose to continue on that basis.

## What works

- **Pipeline:** Queued → Probing → Running → Verifying → Saved, plus Rejected, Failed and Canceled. Canceled is separate, isn't counted as a failure, and has no error.
- **Detect** by magic bytes (`ftypheic`/`ftypheix`/`ftypmif1`, `FF D8 FF`). A `.jpg` that is really HEIC converts and shows a notice.
- **Probe** in `core`, before any engine work, by walking the HEIF boxes (`pitm`, `ipco`, `ipma`, `ispe`, `irot`, `imir`). The file is only read into memory after the size limit passes.
- **Limits and memory:** `presets/limits.ts` (`{maxMB, maxFiles, maxPages?, maxPixels?}`, web and desktop values). The memory estimate is w × h × 4 × `memFactor` (3 for HEIC → JPG), checked against a per-job budget before decoding. A selection over 200 files shows a toast.
- **Route planner** reads the edge table: least quality loss, then fewest hops, at most 2 hops, with lossless intermediate steps. A route is stored as steps `{id, op, dependsOn, status, error}`. On a two-hop route the root error is shown and later steps get `UpstreamFailed` (unit tested; only one real edge exists today).
- **Orientation:** HEIC orientation lives in `irot`/`imir`, which libheif applies. EXIF orientation inside a HEIC is ignored, which prevents double rotation. mozjpeg writes JFIF only, so the output has no EXIF, and Verify checks for that. The rotated fixture is checked by pixel colour in a corner, not only by size.
- **Verify:** type is JPEG, the file ends with EOI, there's no EXIF, and the browser decodes it to the expected (rotated) size. Any failed check → Failed.
- **Watchdog:** heartbeats at each worker step; a timeout and a silence window, both scaled by megapixels; the clock pauses while `document.visibilityState === 'hidden'`. Cancel terminates the worker.
- **Concurrency** is capped at 2 jobs; one fresh worker per job.
- **Naming:** `{name} (converted).jpg`, adding ` (2)`, ` (3)` for clashes within the session (case-insensitive). Bengali and emoji names survive.
- **Errors** follow "what, why, what to do". The error code and raw engine log sit behind a "Details" toggle.
- **UI:** drop zone (also a keyboard-focusable file picker), one card per file with state, before/after size, cancel and download. Dark mode follows the system setting.

## Tests

| Layer | Count | Result (local, Nov 16) |
| --- | --- | --- |
| Unit (Vitest, `core`) | 36 | 36 passed |
| E2E (Playwright, Chromium 1254 + Firefox 157) | 9 specs × 2 browsers + timings | 18 passed, 2 skipped (iPhone fixture missing) |
| CI (`.github/workflows/ci.yml`) | typecheck, unit, e2e on every push | **Written but never run**: nothing has been pushed |

## Measured timings

Machine: Intel i5-12400F (12 threads), 16 GB RAM, Windows 11, production build (`vite preview`), headless Playwright. Median of 3 runs, in ms. Each job spawns a fresh worker, so every run pays worker start + WASM instantiation. Raw data: `apps/web/test-results/timings-*.json` (`pnpm --filter @refract/web exec playwright test timings`).

| Fixture | Browser | Worker start | WASM load | Decode | Encode | Verify | Total |
| --- | --- | --- | --- | --- | --- | --- | --- |
| plain.heic (400×300) | Chromium | 8 | 65 | 19 | 46 | 9 | 163 |
| plain.heic | Firefox | 17 | 52 | 3 | 28 | 11 | 116 |
| rotated.heic (300×400) | Chromium | 13 | 64 | 33 | 37 | 11 | 173 |
| rotated.heic | Firefox | 15 | 47 | 3 | 27 | 12 | 107 |
| large.heic (6000×4000, 24 MP) | Chromium | 7 | 45 | 730 | 2,828 | 183 | 3,777 |
| large.heic | Firefox | 13 | 45 | 756 | 3,396 | 177 | 4,421 |
| iphone.heic | — | not measured: fixture missing | | | | | |

Download cost: the libheif bundle is 2.0 MB (about 700 KB gzipped) because the WASM is embedded as base64; mozjpeg's WASM is 252 KB (60 KB gzipped). Both load only when a job starts.

## Open risks

1. **No real iPhone HEIC tested.** Real iPhone files are grid images (tiled HEVC) with EXIF and depth/aux images. The synthetic fixtures don't cover that. This is the most important gap; add `fixtures/iphone.heic` (see `fixtures/README.md`).
2. **Encode dominates and blocks heartbeats.** mozjpeg takes about 2.8–3.4 s for 24 MP here, all in one synchronous call. The silence window is 10 s + 1 s per megapixel; a slow phone on a 48 MP photo could still trip it. It needs phone testing.
3. **The memory budget is a guess** (1 GiB per job, 2 jobs at once). It isn't calibrated on a phone or a low-RAM laptop.
4. **Bundle size:** using the separate `libheif.wasm` (1.46 MB, compresses better than base64) instead of the bundle would cut the download. Worth doing in phase 3.
5. **`mif1` is also used by AVIF.** Detection follows the spec for phase 1; once AVIF is in scope, check compatible brands too.
6. **Safari not tested** (WebKit wasn't in scope).
7. **Name clashes with files already in the Downloads folder** can't be seen from the web; the browser renames those itself. The desktop app's Rust write helper must check the disk.
8. **CI unproven:** the workflow hasn't run on GitHub yet.

## Deliberately not built (phase 3+)

Options UI and remembered options, "Reset to defaults", Retry, ZIP / "Save all as ZIP", "Use in another tool", the separate stuck-at-100 % phase (Verify does have its own shorter timeout), the target-size search, other formats, `packages/ui` and `engine-native`, deployment.

## Deviations from the brief to confirm

- `Edge.run(input, opts, ctx)` takes a context `{signal, heartbeat, report}` instead of a bare `signal`, so the watchdog can hear from the engine.
- `memFactor` for images multiplies decoded RGBA (w × h × 4), not file size, as the brief's step 4 suggests.
- `core` gets time from an injected clock rather than timer globals, so it stays free of DOM and Node APIs and its tests are deterministic.
