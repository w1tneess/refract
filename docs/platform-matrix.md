<!-- coderabbit-ai-generated:v1 -->

<!-- GENERATED FILE — DO NOT EDIT BY HAND. -->
<!-- Source: specs/tool-manifest.json -->
<!-- Regenerate with: pnpm manifest:generate -->

# Refract Platform Support & Capabilities Matrix

Generated from `specs/tool-manifest.json` (manifest version `1`).

Refract ships **11 canonical tools** across **3 domains** (Images, PDF, Audio) on both **Refract Web** (WebAssembly in dedicated Web Workers) and **Refract Desktop** (native Rust + Slint UI).

---

## 1. Format Support

| Format | Kind | Extensions | Web (Decode / Encode) | Desktop (Decode / Encode) | Notes |
| --- | --- | --- | --- | --- | --- |
| **HEIC** | image | `heic`, `heif` | Yes / No | No / No | Decoded via libheif-js WASM; outputs convert to JPG, PNG, or WebP. Unsupported in native desktop builds; use Refract Web for HEIC inputs. |
| **JPEG** | image | `jpg`, `jpeg`, `jfif` | Yes / Yes | Yes / Yes | — |
| **PNG** | image | `png` | Yes / Yes | Yes / Yes | — |
| **WebP** | image | `webp` | Yes / Yes | Yes / Yes | — |
| **PDF** | pdf | `pdf` | Yes / Yes | Yes / Yes | — |
| **MP3** | audio | `mp3` | Yes / Yes | Yes / Yes | Requires ffmpeg and ffprobe on PATH. |
| **WAV** | audio | `wav` | Yes / Yes | Yes / Yes | Requires ffmpeg and ffprobe on PATH. |
| **M4A** | audio | `m4a`, `aac`, `mp4` | Yes / Yes | Yes / Yes | Requires ffmpeg and ffprobe on PATH. |
| **OGG** | audio | `ogg`, `oga` | Yes / Yes | Yes / Yes | Requires ffmpeg and ffprobe on PATH. |

---

## 2. Tool Support & External Dependencies

| Tool ID | Domain | Inputs (Web / Desktop) | Outputs (Web / Desktop) | Web Engine | Desktop Engine & Status | External Dependencies | Password Unlock (Web / Desktop) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `convert-image` | image | Web: HEIC, JPEG, PNG, WebP<br>Desktop: JPEG, PNG, WebP | JPEG, PNG, WebP | libheif / @jsquash WASM | Rust image crate — Available (built-in) | None | No / No |
| `compress-image` | image | Web: HEIC, JPEG, PNG, WebP<br>Desktop: JPEG, PNG, WebP | Web: JPEG, WebP<br>Desktop: JPEG | @jsquash encoders WASM | Rust image crate — Available (built-in) | None | No / No |
| `resize-image` | image | Web: HEIC, JPEG, PNG, WebP<br>Desktop: JPEG, PNG, WebP | JPEG, PNG, WebP | @jsquash/resize (Lanczos-3) WASM | Rust image crate (Lanczos-3) — Available (built-in) | None | No / No |
| `merge-pdf` | pdf | PDF | PDF | pdf-lib | Rust lopdf — Available (built-in) | None | Yes / Yes |
| `split-pdf` | pdf | PDF | PDF | pdf-lib | Rust lopdf — Available (built-in) | None | Yes / No |
| `reorder-pdf` | pdf | PDF | PDF | pdf-lib | Rust lopdf — Available (built-in) | None | Yes / No |
| `images-to-pdf` | pdf | Web: HEIC, JPEG, PNG, WebP<br>Desktop: JPEG, PNG, WebP | PDF | pdf-lib | Rust lopdf + image — Available (built-in) | None | No / No |
| `pdf-to-images` | pdf | PDF | PNG, JPEG | pdfjs-dist + OffscreenCanvas | PDF renderer (mutool / pdftoppm / pdftocairo / gs) — Requires external binary | `mutool \| pdftoppm \| pdftocairo \| gs` *(fallback: Extracts embedded JPEG/PNG XObject images when no rasterizer is installed on PATH.)* | Yes / Yes |
| `compress-pdf` | pdf | PDF | PDF | @neslinesli93/qpdf-wasm | Rust lopdf — Available (built-in) | None | Yes / Yes |
| `convert-audio` | audio | MP3, WAV, M4A, OGG | MP3, WAV, M4A, OGG | FFmpeg WebAssembly (lazy-loaded) | System FFmpeg — Requires external binary | `ffmpeg`, `ffprobe` | No / No |
| `trim-audio` | audio | MP3, WAV, M4A, OGG | MP3, WAV, M4A, OGG | FFmpeg WebAssembly (lazy-loaded) | System FFmpeg — Requires external binary | `ffmpeg`, `ffprobe` | No / No |

---

## 3. Option Scope (Shared vs. Platform-Specific)

| Tool ID | Shared Options | Web-Only Options | Desktop-Only Options | Canonical Output Template |
| --- | --- | --- | --- | --- |
| `convert-image` | `to`, `quality` | — | — | `{name} (converted)` |
| `compress-image` | `targetKB` | `to` | — | `{name}_compressed` |
| `resize-image` | `unit`, `width`, `height`, `dpi`, `to` | — | — | `{name}_resized` |
| `merge-pdf` | — | — | — | `{name}_merged` |
| `split-pdf` | `ranges` | — | — | `{name}_split` |
| `reorder-pdf` | `order` | — | — | `{name}_reordered` |
| `images-to-pdf` | — | `pageSize` | `dpi`, `quality` | `{name}` |
| `pdf-to-images` | `to`, `dpi` | — | — | `{name}_pages` |
| `compress-pdf` | — | — | — | `{name}_compressed` |
| `convert-audio` | `to`, `bitrate` | — | — | `{name} (converted)` |
| `trim-audio` | `start`, `end`, `bitrate` | — | — | `{name}_trimmed` |

---

## 4. Resource Limits & Memory Budgets

| Limit Profile | Web Limits | Desktop Limits |
| --- | --- | --- |
| `image.convert` | 200 MB/file, 200 files, 100 MP | 2048 MB/file, 10000 files, 1000 MP |
| `pdf` | 200 MB/file, 50 files, 2000 pages | 4096 MB/file, 1000 files, 20000 pages |
| `audio` | 200 MB/file, 50 files | 4096 MB/file, 1000 files |
| **Memory budget** | 1024 MB | 8192 MB |
