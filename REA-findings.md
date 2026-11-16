# Refract: what the real apps do (REA findings)

Nov 16, 2026. Supplements *Refract — Research & Project Brief* (Nov 14).

## Method and coverage

| Target | How | What was seen |
| --- | --- | --- |
| CloudConvert (`/heic-to-jpg`) | REA `capture_browser_scenario` (Chrome, 5 s idle), then `export_web_scripts`, then grep of the exported chunks | Converter, format-selector and drop-zone chunks (`Converter-*.js`, `useInputOutputFormatSelector-*.js`, `ConverterDropFiles-*.js`, `useApi-*.js`) |
| iLovePDF (`/compress_pdf`) | The same REA capture, then decoding the page HTML. Re-captured with `capture.limits.max_network_body_bytes` raised to 8 MB | The inline `ilovepdfConfig` object, the option markup, and the main bundle `app.9d906da.js` (848 KB) |
| Tichau File Converter | Source code read directly from GitHub (GPL-3.0, so REA wasn't needed) | `ConversionJobFactory.cs`, `ConversionJob.cs`, `ConversionState.cs` |
| Convertio (`/heic-jpg/`) | The same REA capture, then decoding the bodies. The main bundle was then captured on its own, by loading its URL directly, because the full page with ads went over REA's 10 MB response limit | The `/js/settings` session config and `app.js` (990 KB). The page HTML was not retained |
| FreeConvert (`/heic-to-jpg`) | The same REA capture, then decoding the Nuxt chunks | The job builder (`127b1ae.js`), the converter page and stuck-job watchdog (`a07f196.js`), the uploader (`422388d.js`). Several larger chunks were **not** retained |

Labels: **[observed]** means the claim appears in captured bytes or source. **[inferred]** means it is my reading of that evidence. Per-session tokens (`token`, `taskId`, `fp`) were deliberately left out. Proprietary JavaScript is described here, not copied.

All five targets now have their main client code captured.

---

## Changes to make to the brief

### 1. Report the step that actually broke, not every step after it

- **Brief says:** typed errors `Unsupported | Corrupt | Encrypted | TooLarge | Timeout | Internal`, with one error per job.
- **CloudConvert does [observed, `ConverterDropFiles-*.js`]:** a job is a set of tasks, and its status is derived with a fixed priority. If any task is `processing`, the job is processing. Otherwise, if any is `waiting`, the job is waiting. Otherwise, if any task errored, the job is in error. Otherwise it is finished. When several tasks fail, the job reports the first error **whose code isn't `INPUT_TASK_FAILED`**. That code marks a task that failed only because an earlier task failed.
- **Change:**
  - Add an `UpstreamFailed` error type.
  - When a route has two hops (HEIC → PNG → PDF) and the first hop fails, show the first hop's error ("couldn't decode HEIC"), not "PDF step failed".
  - Use the same status-priority rule for the batch summary.

**FreeConvert confirms the model [observed, `127b1ae.js`, `a07f196.js`]:** its job builder emits the same task graph as CloudConvert. Each input becomes `import-{i}` (`import/upload`, or `import/url` with a filename), then `convert-{i}` with `input`, `output_format` and `options`, then `export/url`. Two of the biggest services agreeing makes this the safest shape to copy.

### 2. Each format pair offers several engines, each with its own options

- **Brief says:** `Edge.engines: ('wasm' | 'native')[]`. The job record stores engine + version + options.
- **CloudConvert does [observed, `useInputOutputFormatSelector-*.js`]:** once the user picks an input and an output format, the page calls `GET /v2/operations?filter[input_format]=…&filter[output_format]=…&include=options,options.details,engine_versions`. For that pair it gets back a list of *alternatives*, one per engine, and each carries:
  - its own `options` schema
  - its `engine_versions`
  - its `credits`
  - an `experimental` flag
  
  It picks a default engine automatically when only one alternative exists. The options panel is rebuilt from whichever engine is selected. The page copy says outright that "the available options change with the selected operation".
- **Change:** give each edge an options schema, so the UI renders its option form from data:

  ```ts
  type Edge = {
    from: Format; to: Format;
    engine: 'wasm' | 'native';
    engineVersion: string;            // pinned, stored in the job record
    options: OptionSpec[];            // drives the options UI
    lossy: boolean; memFactor: number; experimental?: boolean;
    run: (input, opts, signal) => Promise<Output>;
  };
  ```

  When two edges exist for one format pair (say WASM and native), choose with the rule the brief already has: least quality loss, then fastest, then fits in memory.

- **FreeConvert adds [observed, `127b1ae.js`, `a07f196.js`]:**
  - Option settings are saved per conversion type (`advancedSetting.perConversionType[slug]`), so a user's choices for HEIC → JPG come back next time.
  - Every options panel has a reset button (`show-reset`).
  - Defaults are explicit values; for video compression, for example, `video_compress_quality_percentage: 20` and `audio_codec: "aac"`.
- **Change:** remember options per format pair, in `localStorage` on web and a settings file on desktop, and give the options panel a "Reset to defaults" button.

### 3. Keep limits per operation as data, and check them in Probe

- **Brief says:** a memory estimate (file size × multiplier) checked against a budget.
- **iLovePDF does [observed, `ilovepdfConfig.fileLimits`]:** every tool has its own limits table, and some tools add page or pixel caps. Examples:

  | Tool | MB | Files | Extra |
  | --- | --- | --- | --- |
  | compress | 200 | 2 | — |
  | merge | 100 | 25 | — |
  | imagepdf | 40 | 20 | — |
  | pdfjpg | 25 | 2 | — |
  | pdfocr | 15 | 1 | pages 50 |
  | upscaleimage | 6 | 1 | pxsize 33,177,600 |

  CloudConvert also rejects files that are too large before uploading them [observed: `file-size-exceeded` emit in `ConverterDropFiles-*.js`].

  The other two load their limits from one config object at startup too:
  - **Convertio [observed, `/js/settings`, `app.js`]:**
    - For a signed-out visitor, the settings response returned `max_file_size: 1073741824` (1 GiB), `concurrency: 10`, `filetime_limit: 10`, `daytime_limit: 10` and `day_mins_left: 10`. **[inferred]** The last three are minutes: 10 minutes of processing per file and 10 per day.
    - `app.js` checks each file against `fetchSettings().max_file_size` and fails it with "File is too large (… max)". So the browser enforces the 1 GiB from settings. The brief's 100 MB figure looks out of date; the server could still enforce something lower, which this capture can't see.
    - When the number of active files reaches the `concurrency` value, it emits `hitLimit("concurrency")` and holds further files back.
    - For URL inputs, it asks the server for the size first (`/process/upload_size`), so the limit applies before anything downloads.
  - **FreeConvert [observed, `422388d.js`]:** the uploader reads `subscription.plan.maxFileSizeInGb` and caps one selection at 500 files (`MAX_SELECT_FILES`, default 500). Selecting more shows a toast instead of failing silently.

  **Change:** also cap how many files can be added in one go (for example 200 on web), and say so in a toast rather than dropping files.
- **Change:** add `presets/limits.ts`, keyed by operation, with `{maxMB, maxFiles, maxPages?, maxPixels?}` and separate values for web and desktop. Probe already reads the page count and dimensions, so enforce the limits there and reject before any work starts. Pixel count is a better memory predictor for images than file size, because a 3 MB JPEG can decode to more than 100 MB of RGBA. Use `maxPixels` in the web-engine memory check.

### 4. Name and package jobs that produce several files *(gap in the brief)*

- **Brief says:** nothing about jobs that produce N outputs (PDF → images, split).
- **Real apps [observed]:**
  - iLovePDF's config has `output_filename: "{filename}_compressed"` and `packaged_filename: "ilovepdf_compressed"`. **[inferred]** Several outputs go into one named archive.
  - CloudConvert's export task sets `archive_multiple_files: true`.
  - Tichau generates one output path per output index: `GenerateOutputFilePath(input, index + 1, count)`.
- **Change:**
  - Every preset gets an output name template, such as `{name}_compressed`, `{name} (converted)` or `{name}-p{n}`.
  - When a job produces more than one file, the web version offers a single ZIP (one save dialog) and the desktop version writes a folder named `{name}_pages/`.
  - Add a golden test: a 50-page PDF → images gives 50 files with the correct names.

Convertio adds a "download all" action as well [observed, `app.js`]: it collects the ids of every `finished` file and requests one ZIP (`/process/zip/ids`). **Change:** offer "Save all as ZIP" for a whole batch, not only for single jobs that produce several files.

### 5. Ask for the password instead of rejecting the file

- **Brief says:** stop at Probe if the file is encrypted. Example message: "it's password-protected. Remove the password and try again."
- **iLovePDF does [observed, `app.9d906da.js`, `askForPassword`]:**
  - When processing returns `400` with `WrongPassword` for some files, it pauses and shows a password field for each locked file.
  - A checkbox (`#allPwd`) applies one password to all of them.
  - An "ignore" button (`#pwdIgnore`) sets `ignore_errors = true` and processes the rest of the batch without those files.
  - The same flow powers its Unlock PDF tool.
- **Change:**
  - At Probe, an encrypted PDF goes to a **Needs password** state, not Rejected.
  - Show the password field on the job card, with "use for all locked files" and "skip these files".
  - A wrong password stays in Needs password and says so. Skipped files appear in the end-of-batch list.
  - **[inferred, technical]** pdf.js can open encrypted PDFs for rendering, but pdf-lib can't decrypt them for merge or split. This needs a WASM build of qpdf (or similar) on the web, or qpdf as a sidecar on desktop. Check this in phase 1.

### 6. Let one tool's output feed the next

- **Brief says:** nothing. Every job starts from a dropped file.
- **iLovePDF does [observed, `app.9d906da.js`]:**
  - After a task finishes, it can start a child task from the previous outputs (`task/next`).
  - The output files appear in the new tool already "uploaded" (`origin: "chained"`, `percent: 100`), so "compress, then merge" needs no re-upload.
- **Change:** each finished job card gets "Use in another tool…". It passes the output blobs or paths straight in as new inputs. Locally this costs almost nothing.

### 7. Make output names unique

- **Brief says:** output goes to `photo (converted).jpg` and is never overwritten. It doesn't say what happens when that name already exists.
- **Tichau does [observed, `ConversionJob.cs`]:**
  - Runs `PathHelpers.GenerateUniquePath(path, this.OutputFilePaths)`, which avoids existing files *and* names already given to other outputs in the same job.
  - Creates any missing output folders.
  - If the output path would equal the input path and the input is set to be deleted or archived afterwards, first renames the input to `*_TEMP.ext`.
- **Change:** on desktop, the Rust write helper adds ` (2)`, ` (3)` and so on until the name is free, checking both the disk and the current batch, so two `IMG.HEIC` files from different folders can't collide in one output folder.

### 8. The watchdog needs two more cases

- **Brief says:** a timeout scaled to file size, plus a heartbeat every second. With no heartbeat for 10 s, or once the timeout is hit, the worker is killed and the job fails.
- **FreeConvert does [observed, `a07f196.js`, `watchStuckJobs`]:** a timer runs every 60 s and checks each job against the time of its last acknowledgement from the server.
  - **It skips the check entirely while offline** (`isInternetAvailable`).
  - A job stuck uploading with no acknowledgement for 20 minutes (`12e5` ms) has its upload cancelled and is reported as `"job stuck at upload"`.
  - A job that reached **100 % progress** but hasn't acknowledged anything for 60 s (`6e4` ms) gets its `convert` or `compress` task marked failed, with the message `process_completion_timeout`.
- **Change:**
  - **Pause the clock when it isn't the job's fault.** On web, a hidden tab throttles its workers; the brief's own Technical research flags this. Stop the heartbeat timer while `document.visibilityState === "hidden"`, and on desktop while the machine is asleep. Otherwise jobs get killed for being in a background tab.
  - **Treat "stuck at 100 %" as its own failure.** Writing out, muxing and verifying the output happen after the progress bar is full. Give that last phase its own short timeout and its own message ("Finishing took too long"), separate from the main timeout.
- **For comparison:**
  - **iLovePDF [observed, `checkTaskProcess`]** polls its task every second. `TaskWaiting` re-sends the process request, `TaskProcessing` polls again, and `500`, `504` and `429` each get their own error path.
  - **Convertio [observed]** shows weighted upload progress while uploading, then the average of the real conversion progress. It disables the remove button while a file is `uploading`, `converting`, `analyzing` or `preparing`. **[inferred]** Users can't cancel mid-conversion there. The brief's cancel-from-any-state is better; keep it.
  - Convertio's file states (`ready → analyzing/preparing → uploading → converting → finished | error`, plus `expired`) line up with the brief's lifecycle. Only `expired` is cloud-only.

### 9. Tichau is weaker than the brief in four places (keep the brief's plan)

| Concern | Tichau [observed] | Brief's plan |
| --- | --- | --- |
| Type detection | By extension only: `ConversionJobFactory.Create` branches on `GetExtension(...)` | Magic bytes first. Keep it |
| Output check | If outputs are missing after `Done`, it only logs `ErrorCantFindOutputFiles`; the job still counts as done | Decode and check the output, and fail the job if the check fails. Keep it |
| Atomic write | Engines write straight to the final path; `OnConversionFailed` deletes partial outputs afterwards | Write to a temp file, then rename. Keep it |
| Cancel | Allowed only while `InProgress`, and recorded as `Failed` with `ErrorCanceled` | **Change:** add a separate `Canceled` state, so the summary doesn't count a cancel as a failure and "Retry" isn't offered for it |

**Worth taking from Tichau [observed]:** `ChangeOutputFileTimestampToMatchOriginal()` sets the output's modified time to match the source file, so converted photos keep their place in date-sorted folders. Add it to the desktop write helper.

Its states (`Unknown → Ready → InProgress → Done | Failed`) are close to the brief's job workflow. One more thing: Tichau sends Office files (`docx/odt/doc`, `xlsx/ods/xls`, `pptx/odp/ppt`) to dedicated Office job classes ahead of any output-type rule. This matches the brief's v1.5 plan to handle Office conversions as a separate engine.

---

## Cloud-only parts: don't copy these

These exist only because each conversion costs these companies server time, as the brief's own REA section says. A local-first app needs none of them.

| What | Where [observed] | Why to skip it |
| --- | --- | --- |
| Multipart upload above 50 MB (`I=5e7`, `import/upload` with `multipart: true`) | CloudConvert `ConverterDropFiles-*.js` | Files never leave the device |
| `import/url` from Google Drive, Dropbox, OneDrive and URLs (`ye=["url","googledrive","dropbox","onedrive"]`) | CloudConvert | Cloud storage is out of scope in the brief |
| Server sharding (`servers: ["api1"…"api25", regional "api5-c-in"…]`) and a task token issued per page load | iLovePDF `ilovepdfConfig` | No servers |
| Credits per task, added up for each job | CloudConvert `ConverterDropFiles-*.js` | Nothing to meter |
| Processing minutes per file and per day (`filetime_limit`, `daytime_limit`, `day_mins_left`), OCR credits | Convertio `/js/settings` | Nothing to meter |
| File size cap taken from the user's plan (`plan.maxFileSizeInGb`) | FreeConvert `422388d.js` | No plans; limits come from device memory instead |
| Chunked upload in 2 MB pieces (`chunkSize: "2Mb"`), 4 retries (`RETRY_NUM`), a shuffled server list with failover, a DNS fallback, and re-uploading every file after a server switch | iLovePDF `app.9d906da.js` | No upload |
| Slow-connection detection, and reporting upload speed whenever it changes by more than 10 % (`/process/up_speed`) | iLovePDF, Convertio `app.js` | No upload |
| Keeping the task alive (`task/touch`), and files that expire on the server (`expired` state) | iLovePDF, Convertio | Nothing stored remotely |
| Showing the job as API JSON with a "Copy" button (`job_builder_job` in `localStorage`) | FreeConvert `127b1ae.js` | A developer-API upsell; no API in scope |
| "Limited to X for your plan, upgrade for Y" messages | iLovePDF inline strings | Upsell, and the brief has no plans |
| The plupload uploader and polling `/v2/tasks/{id}` | iLovePDF, CloudConvert | Use Web Worker messages, or Tauri events, instead |

One cloud idea **is** worth keeping in local form. CloudConvert and FreeConvert both model every job as a small graph: `import → convert → export`, where tasks name the tasks they depend on. The brief's two-hop route is already a small graph of this kind. Store it as one, with each step having `{id, op, dependsOn, status, error}`. Rules 1 (root-cause errors) and 4 (packaging) then fall out naturally.

---

## Licence note

Tichau is GPL-3.0. That is compatible with the GPL-3.0 licence the brief suggests, so its logic (unique paths, timestamp copy) can be adapted with attribution. CloudConvert's, iLovePDF's, Convertio's and FreeConvert's code is proprietary: the behaviour above is described from observation and must be reimplemented independently, not copied.

## Open items

- Convertio's server might still enforce a cap below the 1 GiB its browser code checks. Only a real upload of a 150 MB file would settle it; not worth doing for this project.
- ~~Check that encrypted PDFs can be decrypted with WASM~~ **Done Nov 16: works.**
  - `@neslinesli93/qpdf-wasm@0.3.0` in a Chrome Web Worker passed 8 of 8 cases: AES-256, AES-128, RC4-128, owner password, owner-only, unencrypted, wrong password and no password.
  - pypdf confirmed every output was unlocked with its text intact, and pdf-lib merged them.
  - About 370 KB to download with Brotli.
  - The test harness is in `qpdf-test/`.
