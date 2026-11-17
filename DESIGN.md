# Design System: Refract

Refract is an open-source, local-first file converter running 100% in-browser via WebAssembly. Files never leave the user's device. This design system codifies the visual atmosphere, typography, color architecture, component behaviors, and motion principles for Refract.

---

## 1. Visual Theme & Atmosphere

- **Atmosphere:** Exact, calm, utilitarian darkroom and print shop — photographic negatives, contact sheets, precision rulers, and proofing presses.
- **Density:** 5/10 (Daily App Balanced). Clean, generous spatial rhythm with dense, information-rich data rows where metrics matter.
- **Variance:** 4/10 (Offset Asymmetric). Restrained layout with structured horizontal topbar, centered 840px tool workspace, and balanced two-column options.
- **Motion Intensity:** 4/10 (Subtle & Tactile). Every interactive element gives instant physical feedback on press (`scale(0.97)`), with hardware-accelerated transforms and zero superfluous bounce.

---

## 2. Color Calibration

Refract uses an absolute **Graphite** neutral base with a singular, high-contrast **Refract Cobalt** accent. Pure black (`#000000`) is banned.

### Light Mode
- **Page Canvas:** `#fafafa` (Neutral paper backdrop)
- **Card Surface:** `#ffffff` (Crisp container)
- **Raised Surface:** `#f4f4f5` (Inputs, secondary buttons, controls)
- **Hairlines:** `rgba(23, 23, 23, 0.08)` (Primary text at 8% opacity)
- **Hairline Hover:** `rgba(23, 23, 23, 0.16)` (Primary text at 16% opacity)
- **Text Primary:** `#171717` (High legibility)
- **Text Secondary:** `#525252` (Labels, captions)
- **Text Muted:** `#737373` (Metadata, units, icons)
- **Accent (Cobalt):** `#0b57d0` (Primary actions, selection indicators)
- **Accent Foreground:** `#ffffff` (Text on cobalt)

### Dark Mode
- **Deepest Surface:** `#0f0f10` (Underlying shell)
- **Page Canvas:** `#141415` (Main backdrop)
- **Card Surface:** `#19191b` (Panel containers)
- **Raised Surface:** `#232325` (Active tabs, inputs, secondary buttons)
- **Hairlines:** `rgba(237, 237, 237, 0.08)` (Primary text at 8% opacity)
- **Hairline Hover:** `rgba(237, 237, 237, 0.16)` (Primary text at 16% opacity)
- **Text Primary:** `#ededed` (Off-white reading ink)
- **Text Secondary:** `#a1a1a6` (Secondary text)
- **Text Muted:** `#85858b` (Metadata, icons)
- **Accent (Ice Cobalt):** `#8ab4f8` (High-contrast singular accent)
- **Accent Foreground:** `#141415` (Dark ink on light accent)

### Semantic Tokens
- **Success / Saved:** `#16a34a` (Light) / `#4cc38a` (Dark)
- **Error / Failed:** `#dc2626` (Light) / `#e5484d` (Dark)
- **Warning / Notice:** `#b45309` (Light) / `#e5a035` (Dark)

---

## 3. Typographic Architecture

Refract pairs **IBM Plex Sans** for interface labels and prose with **IBM Plex Mono** for technical figures, metadata, and format indicators.

- **Interface Family:** `IBM Plex Sans`, `-apple-system`, `BlinkMacSystemFont`, `sans-serif`
- **Technical/Figure Family:** `IBM Plex Mono`, `ui-monospace`, `monospace`
- **Hierarchy:**
  - Page/Tool Title: 22px / 28px line-height, weight 500, letter-spacing `-0.01em`
  - Subhead / Blurb: 13.5px / 20px line-height, weight 400, secondary text color
  - Card & Row Titles: 14px / 20px line-height, weight 500
  - Technical Metrics: 12px monospace, tabular numbers (`font-variant-numeric: tabular-nums`)
  - Meta & Badges: 11px to 11.5px monospace, sentence case
- **Banned:** Uppercase tracked labels (`TRACKED_CAPS`), font weights above 600, negative letter spacing on small text.

---

## 4. Component Behaviors & Micro-Interactions

### Buttons & Interactive Controls
- **Tactile Feedback:** Every pressable element (`button`, `.button`, `.domain-tab`, `.tool-group a`) compresses with `transform: scale(0.97)` on `:active`.
- **Top Highlights:** On dark themes, raised buttons and cards use `box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08)` to communicate tactile depth without glowing outer shadows.
- **Focus Rings:** Visible exclusively via `:focus-visible`: `outline: 2px solid var(--border-focus); outline-offset: 2px;`.

### Dropzone
- Tactile dashed perimeter with hairline borders.
- Hover state: soft surface elevation. Drag-over state: solid border with accent tint.
- Format badges underneath explicitly indicate accepted formats (`HEIC`, `JPEG`, `PNG`, `WebP`, etc.).
- Clipboard paste (`Ctrl+V` / `⌘V`) supported globally with immediate visual feedback.

### Options Panel
- Modular card layout with 4px grid alignment.
- Select dropdowns feature custom minimal chevrons and hardware-accelerated focus states.
- Number inputs feature unit indicators (`%`, `KB`, `cm`, `px`) set in monospace.
- "Reset to defaults" text button restores default settings in one click.

### Job Rows & Progress
- Flat row pattern under a 1px baseline rule.
- File type icons match the text-muted role.
- Dynamic size differential calculates byte savings (e.g., `3.4 MB → 412 KB (-88%)`) and highlights reductions in muted success green.
- Hardware-accelerated progress bar with GPU-rendered indeterminate translation.

---

## 5. Layout & Spatial Principles

- **Geometry:** Sticky topbar (56px) + Centered 840px shell container (`.shell`).
- **Strict 4px Scale:** Margins, paddings, and gaps strictly use multiples of 4 (4, 8, 12, 16, 20, 24, 32, 48, 64px).
- **Nested Radii Math:**
  - Panels: 14px (`--r-xl`)
  - Cards & Dropzone: 12px (`--r-l`)
  - Buttons, Inputs, Rows: 8px (`--r-m`)
  - Small pills & tags: 4px–6px (`--r-s` / `--r-xs`)
  - Math rule: `inner_radius = outer_radius - padding`.
- **Mobile Adaptations (< 640px):**
  - Sticky frosted bottom bar for thumb navigation across domain tabs.
  - Multi-column grids collapse to single column.
  - Interactive targets maintain $\ge 40$px tap area.

---

## 6. Motion Philosophy

- **Single Easing Curve:** `cubic-bezier(0.22, 1, 0.36, 1)` (unslop / Emil Kowalski tactile ease-out) for all interactive states and entrances.
- **Fast Durations:**
  - Micro-interactions (hover, active press): 120ms–160ms.
  - Entrances and toast transitions: 200ms–240ms.
- **Hardware Acceleration:** Transitions operate exclusively on `transform`, `opacity`, `color`, `background-color`, and `border-color`.
- **Reduced Motion:** When `prefers-reduced-motion: reduce` is detected, animations collapse to simple opacity fades.

---

## 7. Explicit Anti-Patterns (Banned)

- No purple/violet AI glow gradients or neon buttons.
- No emojis used as interface icons.
- No `transition: all` catch-alls.
- No floating orbs, decorative mesh gradients, or holographic shapes.
- No uppercase tracked labels.
- No overlapping or unconstrained absolute elements.
- No pure black (`#000000`).
