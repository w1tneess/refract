# Refract — Research & Project Brief

Nov 14, 2026 · @Advaita Chandra

**Refract** is a file converter: a web app and a desktop app on one shared engine.

## Summary and verdict

**Build it as a portfolio and learning project, not as a business.** Both obvious angles are already crowded: general cloud converters (CloudConvert, Convertio, FreeConvert, iLovePDF) and privacy-first browser converters that never upload files. There are also dozens of browser-only exam-photo resizers for Indian government forms.

Even an open-source, ad-free Windows converter with right-click convert already exists (Tichau File Converter). What is left is a **modern, cross-platform app (web + Windows + macOS + Linux) on one shared engine**, with a better interface than the existing tools. That is a design-and-reach gap, not an unserved need.

- **Worth building for:** a strong GitHub project, real Rust/WebAssembly/desktop experience, a tool you will use yourself.
- **Not worth building for:** income. Competitors give the core features away free, and iLovePDF gives students a year of Premium free.
- **Timing:** no work until Pre-Boards end on 2 December. Small, scoped work only until boards. The main build after boards.

## Competitor research (REA)

All four sell the same scarce resource, their own server compute. They differ only in which limit they put on the free tier. Figures are from public pricing pages and review sites as of November 2026. Sources disagree in places, so treat these numbers as approximate.

**Agents:** free user, paying user, API developer, the company, ad networks, cloud host.

**Resources:** the user's file and trust, server compute time, bandwidth and storage, user attention (sold via ads), money.

**Events (exchanges):**

| Event | User gives | Company gives |
| --- | --- | --- |
| Free conversion | File + attention to ads | Converted file, using its compute |
| Hits the free limit | Money | More quota, bigger files, priority |
| API call | Developer's money | Conversions at scale |

**What each one meters:**

| Tool | Free-tier lever | Paid entry |
| --- | --- | --- |
| [CloudConvert](https://cloudconvert.com/pricing) | Conversion count: 25 per day. Office → PDF costs 2 credits minimum | $8 one-time for 500 minutes, or $8/month for 1,000 |
| [Convertio](https://www.xpay.sh/agent-ready-index/convertio-co/) | Processing time: 10 minutes per file and per day for signed-out visitors. File size: 1 GiB, enforced in its browser code from its own settings (REA, Nov 16). The 100 MB figure on review sites looks out of date | $9.99/month, or $5.99/month billed annually (500 MB) |
| [FreeConvert](https://www.freeconvert.com/pricing) | Processing time: 5 conversion minutes per file; files up to 1 GB | About $10–13/month (sources disagree) |
| [iLovePDF](https://www.ilovepdf.com/pricing) | Tasks and size per task | $9/month, or $5/month billed annually; about ₹200/month annual in India. Free year for students |

**Takeaway:** their pricing exists because each conversion costs them server time. A tool that runs on the user's device has almost no running cost, but it also has nothing to meter. Its money must come from somewhere else (see Monetization).

## Positioning

I tested two angles. Both are taken; one gap remains.

| Angle | Status | Evidence |
| --- | --- | --- |
| Privacy-first, in-browser, no uploads | Crowded | [MiOffice](https://canary.mioffice.ai/vs/convertio), [WildandFreeTools](https://wildandfreetools.com/blog/jpg-to-avif-convertio-cloudconvert-alternative/) and others already advertise no uploads and no limits |
| Exam photo/signature resizer for Indian forms (SSC, NEET, JEE) | Crowded | Many browser-only tools with exam presets, e.g. [CompressTo200KB](https://compressto200kb.pages.dev/compress-photo-for-government-form/), [CoreTools](https://coretools.pages.dev/exam-tools/), [MergeImages](https://mergeimages.net/exam-photo-resizer). One reports SSC now takes the photo by webcam during registration (not verified) |
| Open-source desktop converter with right-click convert | Taken on Windows | [Tichau File Converter](https://tichau.itch.io/file-converter): open-source since 2014 (GPL-3.0), \~5,100 GitHub stars ([ecosyste.ms](https://repos.ecosyste.ms/hosts/GitHub/repositories/Tichau%2FFileConverter)). [PCWorld](https://www.pcworld.com/article/3219316/this-free-windows-app-makes-file-conversion-ridiculously-easy.html) notes no ads and no data collection |
| One modern app across web + Windows + macOS + Linux, full window UI, shared engine | Narrow gap | Tichau is Windows-only, works mostly from the right-click menu, and reviewers call its settings UI confusing ([Gizmodo](https://gizmodo.com/download/file-converter)). This is a polish-and-reach gap, not an unserved need |

**Chosen position:** "A clean, open-source converter. Runs on your machine. No ads, no account, no limits." The web version is the free, try-it-now entry point. The desktop app is the full tool.

**Honest read:** no angle here is an unserved need. Users already have good free options for every core job. The project earns its place as a well-built, cross-platform, open-source portfolio piece, and by being better-designed than what exists.

## What the real apps do (REA, Nov 16)

I captured the live pages of CloudConvert, iLovePDF, Convertio and FreeConvert in Chrome with [REA](https://github.com/morluto/rea) and searched the code they send to the browser. For Tichau File Converter, which is open source, I read the code on GitHub. The findings are folded into Conversion logic, Job workflow, Reliability mechanics and Testing below. Full evidence: `REA-findings.md`.

All five tools' main client code was captured.

| Source | Observed | Used for |
| --- | --- | --- |
| CloudConvert | A job is a graph of tasks (`import → convert → export`). Batch status: processing, then waiting, then error, then finished. The reported error skips knock-on failures (`INPUT_TASK_FAILED`). Each format pair lists several engines, each with its own options, version and experimental flag. Too-large files are rejected before upload. | `UpstreamFailed`; route stored as a step graph; options schema on each edge |
| iLovePDF | A limits table for each tool (compress 200 MB / 2 files; merge 100 MB / 25; OCR 15 MB / 1 file / 50 pages; upscale 6 MB / 33.2 M pixels). Output name templates (`{filename}_compressed`) and a bundle name for several outputs. Three compression levels: low, recommended (default), extreme. Locked PDFs pause for a password per file, with "same for all" and "ignore these and continue". A finished task's outputs feed the next tool without re-upload (`task/next`). | `presets/limits.ts`; name templates; ZIP for several outputs; three-level compress preset; Needs password state; "Use in another tool" |
| FreeConvert | The same task graph as CloudConvert (`import-{i} → convert-{i} → export`). Options are remembered per conversion type, with a reset button. A cap of 500 files per selection, shown in a toast. A watchdog every 60 s that skips while offline, cancels an upload with no response for 20 min, and fails a job stuck at 100 % for 60 s (`process_completion_timeout`). | Confirms the step graph; remembered options and reset; a cap on files per selection; watchdog changes (Reliability mechanics) |
| Convertio | Limits arrive in one settings object for each session. A signed-out visitor got `max_file_size` 1 GiB, `concurrency` 10, and 10 processing minutes per file and per day. The browser code fails files over that `max_file_size` and holds files back at the `concurrency` limit. "Download all" makes one ZIP of every finished file. The remove button is disabled mid-conversion. | Confirms limits as data and queue concurrency; "Save all as ZIP" for a batch; keep cancel-any-time |
| Tichau | Detects type by extension only. A missing output is only logged. Writes straight to the final path. Cancel is recorded as Failed. Generates unique output paths; copies the source file's timestamp. | Confirms magic bytes, verify-or-fail and atomic writes; adds the Canceled state, unique names and timestamp copy |

**Cloud-only parts; don't copy these.** They exist only because each conversion costs the company server time:

- multipart upload above 50 MB; chunked uploads with retries, server failover and re-upload; upload speed reporting
- keeping tasks alive on the server, and files that expire
- importing from Google Drive, Dropbox or OneDrive
- `api1`–`api25` server sharding and a task token issued per page load
- credits per task, and processing minutes per file or per day
- "upgrade your plan" limits, and file size caps taken from the user's plan
- showing the job as API JSON with a "Copy" button
- polling the server for task status

**Licence.** Tichau is GPL-3.0, which matches this project's suggested licence, so its logic can be adapted with credit. The other four services' code is proprietary: reimplement the behaviour, don't copy the code.

## Technical research

The browser can handle images, audio, short video and most PDF jobs. Office documents and very large files need native tools, which is why the desktop app exists.

**WebAssembly (browser) limits**

| Limit | Detail |
| --- | --- |
| ffmpeg.wasm size | Core is \~31 MB, so load it only when the user picks a media file ([soundtools.io](https://soundtools.io/blog/building-browser-audio-tools-ffmpeg-wasm)) |
| Memory | Whole file is loaded into memory. \~2 GB hard cap; files over \~500 MB can crash tabs, especially on phones ([soundtools.io](https://soundtools.io/blog/building-browser-audio-tools-ffmpeg-wasm), [ffmpeg.wasm issue #623](https://github.com/ffmpegwasm/ffmpeg.wasm/issues/623)) |
| Speed | Roughly 10–100× slower than native FFmpeg ([skill notes](https://skillselion.com/skills/josiahsiegel/claude-plugin-marketplace/ffmpeg-webassembly-workers)) |
| Background tabs | The OS may pause the worker if the tab is hidden ([freetoolonline](https://freetoolonline.com/guides/en/ffmpeg-online-conversion-stalled-three-fixes.html)) |
| Office → PDF | LibreOffice in WASM is \~150 MB and unstable in 2026; not realistic in the browser ([DEV](https://dev.to/digitalofen/i-tried-running-file-conversion-fully-in-the-browser-wasm-libreoffice-ffmpeg-57mh)) |

**Desktop framework: Tauri 2 over Electron**

|  | Tauri 2 | Electron |
| --- | --- | --- |
| Minimal installer | \~2.5–3 MB | \~85 MB |
| Idle memory | \~30–50 MB | \~200–300 MB |
| Backend | Rust | Node.js |
| Webview | OS webview (WebView2 on Windows) | Bundled Chromium |

Figures: [tech-insider](https://tech-insider.org/tauri-vs-electron-2026/), [Moklabs](https://moklabs-research.pages.dev/reports/technology/2026-03-20-tauri-vs-electron-desktop-framework-decision). Tauri wins on size, which matters because the app will also bundle FFmpeg. The cost is a small Rust backend to write and maintain.

## Project scope

v1 is a few common jobs done very well on both web and desktop, not 200 formats.

**v1 (web + desktop)**

| Area | Jobs |
| --- | --- |
| Images | HEIC/WebP/PNG/JPG ↔ each other; compress to a target size (KB); resize by pixels or cm |
| PDF | Merge, split, reorder pages; images → PDF; PDF → images; compress |
| Audio | MP3, WAV, M4A, OGG ↔ each other; trim |
| Core UX | Drag and drop, batch queue, before/after size, no account, no ads, dark mode; jobs that produce several files save as one ZIP; "Save all as ZIP" for a batch; "Use in another tool" passes a finished job's output straight in as new input |

**v1.5 (desktop only)**

- Word/Excel/PowerPoint → PDF via a bundled LibreOffice (headless)
- Video convert/compress with native FFmpeg, no size limit
- Folder batch: convert every file in a folder
- Right-click "Convert with …" in File Explorer

**Out of scope (for now)**

- PDF → editable Word (layout quality is poor without paid engines)
- OCR, e-signatures, cloud storage, accounts, API
- Mobile apps

## Architecture and stack

One UI and one job format, with two engines behind it: WebAssembly in the browser, native tools on desktop. The UI never knows which engine ran.

1. **UI** (shared): Vite + TypeScript + React or Svelte. Drag-and-drop, queue, settings.
2. **Job layer** (shared): one interface, e.g. `convert({input, from, to, options}) → output`. Picks an engine.
3. **Web engine**: runs in Web Workers so the page never freezes.
   - Images: WASM codecs (e.g. jSquash) + a HEIC decoder
   - PDF: pdf-lib (edit/merge) + pdf.js (render to images)
   - Audio/short video: ffmpeg.wasm, lazy-loaded
4. **Desktop engine**: Tauri 2 (Rust) calling bundled tools as sidecars.
   - FFmpeg (media), libvips or ImageMagick (images), Ghostscript (PDF compress)
   - LibreOffice: detect an existing install instead of bundling it (it is several hundred MB)
5. **Hosting**: static site on Cloudflare Pages, e.g. a subdomain of advaitachandra.in. Needs COOP/COEP headers for multithreaded ffmpeg.wasm.
6. **Releases**: GitHub Actions builds Windows/macOS/Linux installers; GitHub Releases for downloads.

**Licence check before coding:** FFmpeg and Ghostscript licences decide what your own licence can be. Ghostscript is AGPL; GPL FFmpeg builds need a GPL app. Simplest is to licence the project GPL-3.0.

## Conversion logic

Every conversion is planned before anything runs: find the file's real type, plan the safest route, pick an engine, then run and check. Bad inputs are rejected at the start, not halfway through.

1. **Detect.** Identify the file by its first bytes (its "magic number"), never its extension. `%PDF` = PDF; `ftypheic` = HEIC; `PK` = a ZIP, then look inside to tell DOCX, XLSX and PPTX apart. If the extension disagrees, trust the bytes and tell the user.
2. **Probe.** Read metadata without converting: dimensions, EXIF rotation, duration, codecs, page count, password protection, size. Stop here with a clear message if the file is corrupt or empty. An encrypted PDF goes to **Needs password** instead. The job card shows a password field, with "use for all locked files" and "skip these files"; a wrong password stays in Needs password and says so. This is iLovePDF's pattern. pdf-lib refuses encrypted files, so unlock first with qpdf compiled to WASM, then hand the result to pdf-lib (tested Nov 16; see below).

   Then check the operation's limits (`presets/limits.ts`: `{maxMB, maxFiles, maxPages?, maxPixels?}`, with separate values for web and desktop) and reject before any work starts. This is the pattern iLovePDF uses (see "What the real apps do").

**Tested Nov 16: unlocking PDFs in the browser works.** Setup: `@neslinesli93/qpdf-wasm@0.3.0`, run in a Web Worker in Chrome 156. Test files were made with pypdf, not qpdf, so qpdf wasn't checking its own work.
- All 8 cases passed: AES-256, AES-128 and RC4-128 with the user password; the owner password; an owner-only file that opens without a password; an unencrypted file; and a wrong or missing password.
- Each unlocked file opened in pypdf with no password and still had its 3 pages of original text. pdf-lib rejected the locked input but merged the unlocked output.
- Cost: about 370 KB to download with Brotli (1.33 MB uncompressed). Under 70 ms per small file. Load it only when a locked PDF appears.
- Code:
  - Run `qpdf --password=… --decrypt in.pdf out.pdf` with a fresh instance for each file.
  - Exit code 0 or 3 means unlocked. Exit code 2 plus the message `invalid password` means Needs password.
  - This build ignores Emscripten's `print`/`printErr` hooks and writes to `console`, so capture `console` in the worker to read that message.
- Licence: qpdf is Apache-2.0 and the wrapper is ISC, both compatible with GPL-3.0.
- Not tested: large PDFs and memory use, Firefox and Safari, and damaged encrypted files.
3. **Plan the route.** Formats are nodes in a *capability graph*; each converter is an edge with a quality cost, a speed cost and the engines that support it. Choose the route with the least quality loss, then the fastest. Maximum 2 hops, and use lossless intermediate formats (PNG, WAV). Example: HEIC → PDF = HEIC → PNG → PDF.
4. **Pick the engine.** Use the web engine only if every step exists in WASM *and* the estimated memory fits the budget. For images, base the estimate on pixel count (width × height × 4), because a 3 MB JPEG can decode to more than 100 MB. For other formats, use file size × a per-format multiplier. Otherwise use native on desktop. On the web with no desktop app, say why and offer the download, never a half-working attempt.
5. **Resolve options.** Start from presets with safe defaults. For **target size** ("under 50 KB"), binary-search the quality setting. If the lowest quality still misses the target, shrink the dimensions in steps and say so.
6. **Run** in a worker or sidecar process (Job workflow, below).
7. **Verify** the output before handing it over (Reliability mechanics, below).

The route table is plain data, so adding a format means adding edges, not new logic. Each edge is one engine, and it carries its own options schema, so the options panel is built from data. CloudConvert does the same: each format pair has several engines, each with its own options and version.

```ts
type Edge = {
  from: Format; to: Format;
  engine: 'wasm' | 'native';  // one edge per engine; two engines = two edges
  engineVersion: string;      // pinned, stored in the job record
  options: OptionSpec[];      // drives the options UI
  lossy: boolean;             // quality cost
  memFactor: number;          // est. RAM = input size × memFactor
  experimental?: boolean;
  run: (input, opts, signal) => Promise<Output>;
};
```

A planned route is stored as a small graph of steps, each with `{id, op, dependsOn, status, error}`. This is how CloudConvert models its `import → convert → export` tasks. Root-cause errors and packaging of several outputs both follow from it.

**Output naming.** Every preset has a name template, such as `{name} (converted)`, `{name}_compressed` or `{name}-p{n}`. A job that produces several files (PDF → images, split) gives one ZIP on web and a `{name}_pages/` folder on desktop.

Target-size search:

```text
lo = 10, hi = 95, best = none
repeat up to 8 times:
  q = (lo + hi) / 2;  out = encode(q)
  if size(out) <= target: best = out; lo = q + 1
  else: hi = q - 1
if best is none: scale down 10% and repeat (max 5 times), else fail with a clear reason
```

## Job workflow

Each file is one job that moves through fixed states. Only a job that passes Verify reaches Saved.

&#91;embedded content: job lifecycle · 6 steps, 2 failure states\]

Bad input stops early at Rejected, before any work is done. Crashes, timeouts and failed checks end at Failed, with temp files removed. Three extra rules:

- The user can cancel from any state: the worker is killed, temp files are deleted, and the job ends in its own **Canceled** state, not Failed. A cancel doesn't count as a failure and isn't offered "Retry". Tichau records cancels as failures; don't copy that.
- A Failed job offers one "Retry" that re-queues it, on desktop using the native engine.
- When a route has two hops and an early step fails, the later steps fail with `UpstreamFailed`. The job shows the first real error ("couldn't decode HEIC"), not the last step's. A batch's status is processing if any job is processing, otherwise waiting if any job is waiting, otherwise error if any job failed, otherwise finished. CloudConvert uses the same rule.

## Reliability mechanics

"Reliable" means three promises: the original is never damaged, a finished job is always a valid file, and every failure says why in plain words. Each mechanism below enforces one of them.

| Failure mode | Mechanism |
| --- | --- |
| Original file overwritten or lost | Inputs are read-only. Output goes to a new name (`photo (converted).jpg`); never overwrite unless the user turns it on |
| Output name already taken | Add ` (2)`, ` (3)` and so on until the name is free, checking both the disk and the other outputs in the same batch, so two `IMG.HEIC` files from different folders can't collide. Missing output folders are created. Adapted from Tichau's `GenerateUniquePath` |
| Converted photos lose their date order | Desktop sets the output's modified time to the source file's (Tichau's `ChangeOutputFileTimestampToMatchOriginal`) |
| Error points at the wrong step | Later steps report `UpstreamFailed`; the UI shows the first real error (Job workflow) |
| File over a tool's limit | Per-operation limits table, checked in Probe before any work starts |
| Half-written output after a crash | Write to a temp file, verify it, then rename into place. Rename is atomic, so the user sees a complete file or none |
| Output exists but is broken | Verify every output: re-open it with a decoder, check type, dimensions/duration/page count against the input, size > 0. Failed check = job failed, temp file deleted |
| Wrong type from a renamed file | Detect by magic bytes (Conversion logic, step 1) |
| Page freezes during a big job | All work runs in Web Workers (web) or sidecar processes (desktop); the UI thread only shows progress |
| Browser tab crashes on a big file | Memory estimate before starting; over budget → refuse on web, route to desktop. Free input buffers before reading output |
| A job hangs forever | Per-job timeout scaled to file size; heartbeat from the worker every second. No heartbeat for 10 s or timeout hit → kill the worker, mark failed. Pause the clock while the tab is hidden (`document.visibilityState`) or the machine is asleep, so throttled jobs aren't killed; FreeConvert likewise skips its check while offline |
| Job stuck at 100 % | Writing out, muxing and verifying happen after the progress bar is full. Give that last phase its own short timeout and message ("Finishing took too long"). FreeConvert fails such jobs with `process_completion_timeout` |
| User re-enters the same options every time | Remember options per format pair (`localStorage` on web, a settings file on desktop), with "Reset to defaults" |
| Too many files dropped at once | Cap one selection (for example 200 files on web) and say so in a toast; don't drop files silently |
| One bad file stops a batch | Each file is its own job. Failures are listed at the end; the rest finish |
| Too many jobs at once | Queue with a concurrency limit (web: 2; desktop: CPU cores − 1) |
| Phone photo appears sideways | Apply EXIF orientation, then strip it, on every image job |
| Quality silently ruined | Lossy steps are flagged in the plan; the UI shows before/after size and a preview before saving |
| Same input gives different output | Fixed preset values and pinned library versions; the job record stores engine + version + options |
| Cancel leaves junk behind | Cancel signal kills the worker or process and deletes its temp files |
| Desktop app silently replaced | Updates only from GitHub Releases, signed; checksum shown on the download page |

**Error messages** follow one pattern: what happened, why, what to do. Example: "Couldn't convert *scan.pdf*: the file is damaged (it ends early). Try re-downloading it." Password-protected files don't get an error; they ask for the password (Conversion logic, step 2). Raw engine errors go to a "Details" toggle, never the headline.

## Code structure

One repository, with the core logic in a package that knows nothing about browsers or desktops. The web and desktop apps are thin shells around it.

```text
refract/
├─ packages/
│  ├─ core/            # pure TypeScript, no UI, fully unit-tested
│  │  ├─ detect/       # magic-byte sniffing, metadata probe
│  │  ├─ graph/        # formats, edges, route planner
│  │  ├─ jobs/         # job state machine, queue, cancel, timeouts
│  │  ├─ verify/       # output checks
│  │  └─ presets/      # named option sets (e.g. "Web image", "Under 50 KB"), output name templates, limits.ts
│  ├─ engine-wasm/     # Web Worker host + WASM adapters (images, pdf-lib, ffmpeg.wasm)
│  ├─ engine-native/   # TS side of Tauri commands that call sidecars
│  └─ ui/              # shared components: drop zone, queue, job card, settings
├─ apps/
│  ├─ web/             # Vite app: picks engine-wasm
│  └─ desktop/         # Tauri app: picks engine-native, falls back to wasm
│     └─ src-tauri/    # Rust: spawn sidecars, file I/O, temp + atomic rename
├─ fixtures/           # golden test files (see Testing)
└─ .github/workflows/  # CI: test, build web, build installers
```

**Rules that keep it reliable**

- `core` never imports from `engine-*` or `ui`. Engines implement one `Engine` interface that `core` defines.
- All file writes happen in one place (Rust on desktop, a download helper on web), so atomic writes are enforced once.
- Every engine adapter returns typed errors (`Unsupported`, `Corrupt`, `Encrypted`, `TooLarge`, `Timeout`, `UpstreamFailed`, `Internal`), never raw strings. The UI turns these into plain messages.

## Testing and release gate

A release ships only when every check below is green in CI. Nothing is called "working" because it ran once on one machine.

**Golden files** (`fixtures/`): a fixed set of real inputs, each with an expected result.

- Normal cases: an iPhone HEIC, a rotated phone JPG, a 50-page PDF, a scanned PDF, an MP3 with cover art
- Multi-output cases: a 50-page PDF → images gives 50 files with the correct names (one ZIP on web); two inputs with the same name in one batch don't overwrite each other
- Hard cases: a 0-byte file, a truncated JPG, a password PDF, a `.jpg` that is really a PNG, a file with a Bengali/emoji filename, a 1 GB video (desktop only)
- Each test checks the output's type, dimensions/pages/duration and size range, not exact bytes. Encoders change byte-for-byte across versions

**Test layers**

| Layer | What | Where |
| --- | --- | --- |
| Unit | Detection, route planner, target-size search, state machine | `core`, every commit |
| Engine | Every edge in the graph against golden files | Web engine in headless Chrome + Firefox; native engine on Windows, macOS, Linux runners |
| End-to-end | Drop → convert → download in the real UI | Playwright, every PR |
| Manual | One real phone, one low-RAM laptop | Before each release |

**Release gate:** all tests green on all three OS runners, no open crash bugs, and the changelog lists every format change.

## Roadmap

Nothing starts until Pre-Boards end on 2 December. Everything before boards is capped at about 3 hours a week. Board exam dates are not confirmed here; adjust phases 2–3 once the datesheet is out.

| Phase | When | Work | Done when |
| --- | --- | --- | --- |
| 0. Park | Now – 2 Dec | Nothing. This doc is the note | Pre-Boards over |
| 1. Spike | 3 Dec – mid-Jan | Check for existing cross-platform open-source converters; pick the licence; build one web job (HEIC → JPG) in a Web Worker | One job works in the browser and the decision to continue is made |
| 2. Pause | Mid-Jan – end of boards | Nothing | Boards over |
| 3. Web v1 | \~4 weeks after boards | Images + PDF + audio jobs, queue, UI; deploy to Cloudflare Pages | Live URL; every v1 job tested on Chrome, Firefox and a phone |
| 4. Desktop v1 | \~4 weeks | Tauri wrapper, native engine, installers via GitHub Actions | Installers for Windows/macOS/Linux on GitHub Releases |
| 5. v1.5 | After that | Office → PDF, video, folder batch, right-click menu | Each feature shipped on its own |

**Rule:** a phase starts only when the previous one's "done when" is true. If phase 1 finds the gap already filled, stop there.

## Monetization

Expect little or no money from this. The realistic options, best first:

| Option | Fit | Note |
| --- | --- | --- |
| Donations (GitHub Sponsors) | Good | Fits open source; small amounts |
| Paid desktop build on a store, free source | Possible | Common open-source pattern: pay for convenience and auto-updates |
| Ads on the web version | Poor | Breaks the "no ads" position, the main reason to choose it |
| Subscriptions | Poor | Nothing to meter; competitors give the same jobs free |

The real return is the portfolio: a cross-platform app with a Rust backend, WebAssembly, CI-built installers and real users.

## Risks and open decisions

**Risks**

| Risk | Effect | Mitigation |
| --- | --- | --- |
| No real gap in the market | Few users | Treat users as a bonus; judge success by what you build and learn |
| Scope creep ("200 formats") | Never ships | v1 list is fixed; new formats only after v1 ships |
| Competes with exams and other projects | Bad marks or nothing finished | Phase gates and the 3 hrs/week cap; pause through boards |
| Licence mistakes (FFmpeg, Ghostscript) | Forced relicensing or takedown | Decide the licence in phase 1, before any code |
| Browser memory limits | Crashes on big files | Size check before converting; send big files to the desktop app |

**Open decisions**

- [ ] React or Svelte for the UI
- [ ] Project licence (GPL-3.0 suggested)
- [x] Project name: **Refract** (decided Nov 16)
- [ ] Domain (subdomain of advaitachandra.in?)
- [ ] Continue after the phase 1 market check, or stop

## Sources

Pricing and limits change often; recheck before relying on any figure.

- [CloudConvert pricing](https://cloudconvert.com/pricing)
- [FreeConvert pricing](https://www.freeconvert.com/pricing)
- [iLovePDF pricing](https://www.ilovepdf.com/pricing)
- [Convertio pricing snapshot (xpay)](https://www.xpay.sh/agent-ready-index/convertio-co/)
- [iLovePDF India pricing (GetPDFPro)](https://getpdfpro-web.pages.dev/blog/ilovepdf-alternatives-honest-comparison)
- [MiOffice vs Convertio](https://canary.mioffice.ai/vs/convertio)
- [CompressTo200KB exam photo tool](https://compressto200kb.pages.dev/compress-photo-for-government-form/)
- [Tichau File Converter](https://tichau.itch.io/file-converter)
- [Tichau File Converter repo stats](https://repos.ecosyste.ms/hosts/GitHub/repositories/Tichau%2FFileConverter)
- [ffmpeg.wasm in practice (soundtools.io)](https://soundtools.io/blog/building-browser-audio-tools-ffmpeg-wasm)
- [ffmpeg.wasm 2 GB limit (GitHub issue)](https://github.com/ffmpegwasm/ffmpeg.wasm/issues/623)
- [Browser LibreOffice/FFmpeg test (DEV)](https://dev.to/digitalofen/i-tried-running-file-conversion-fully-in-the-browser-wasm-libreoffice-ffmpeg-57mh)
- [Tauri vs Electron 2026 (tech-insider)](https://tech-insider.org/tauri-vs-electron-2026/)
