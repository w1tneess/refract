# Design System: Refract

Refract is an open-source, local-first file converter running 100% in-browser via WebAssembly. Files never leave the user's device. This design system codifies the visual atmosphere, typography, color architecture, component behaviors, and motion principles for Refract.

---

## 1. Visual Theme & Atmosphere

- **Atmosphere:** Apple-inspired — calm, exact, quiet. Frosted-glass surfaces, graphite neutrals, single blue accent. Feels like a native macOS app running in the browser.
- **Density:** 5/10 (Daily App Balanced). Clean, generous spatial rhythm with dense, information-rich data rows where metrics matter.
- **Variance:** 4/10 (Offset Asymmetric). Restrained layout with structured horizontal topbar, centered shell container, and balanced two-column options.
- **Motion Intensity:** 4/10 (Subtle & Tactile). Every interactive element gives instant physical feedback on press (`scale(0.97)`), with hardware-accelerated transforms and zero superfluous bounce.

---

## 2. Color Architecture

Refract uses an absolute **Graphite** neutral base with a singular, high-contrast **Apple Blue** accent. Dark mode uses pure black (`#000000`) as the shell surface.

### Light Mode

| Token | Value | Usage |
|---|---|---|
| Page Canvas | `#f5f5f7` | Main backdrop |
| Card Surface | `#ffffff` | Panel containers |
| Raised Surface | `#f0f0f5` | Inputs, secondary buttons, controls |
| Hairline | `#d2d2d7` | Dividers, panel borders |
| Control Border | `#86868b` | Interactive input & button boundaries (≥ 3:1 WCAG 1.4.11) |
| Text Primary | `#1d1d1f` | High legibility |
| Text Secondary | `#58585d` | Labels, captions (≥ 4.5:1 WCAG 1.4.3) |
| Text Muted | `#66666b` | Metadata, icons (≥ 4.5:1 WCAG 1.4.3) |
| **Accent (Apple Blue)** | `#0066cc` | Primary actions, selection indicators |
| Accent Foreground | `#ffffff` | Text on solid blue |
| Accent Ink | `#0055b3` | Text on `--accent-soft` tints (≥ 4.5:1) |

### Dark Mode

| Token | Value | Usage |
|---|---|---|
| Page Canvas | `#000000` | Main backdrop |
| Card Surface | `#1c1c1e` | Panel containers |
| Raised Surface | `#2c2c2e` | Inputs, secondary buttons, controls |
| Hairline | `#38383a` | Dividers, panel borders |
| Control Border | `#76767b` | Interactive input & button boundaries (≥ 3:1 WCAG 1.4.11) |
| Text Primary | `#f5f5f7` | Off-white reading ink |
| Text Secondary | `#a1a1a6` | Secondary text (≥ 4.5:1 WCAG 1.4.3) |
| Text Muted | `#98989d` | Metadata, icons (≥ 4.5:1 WCAG 1.4.3) |
| **Accent (Apple Blue)** | `#0a84ff` | Primary actions, selection indicators |
| Accent Foreground | `#000000` | Dark ink on solid blue |
| Accent Ink | `#70b8ff` | Text on `--accent-soft` tints (≥ 4.5:1) |

### Semantic Tokens

| Token | Light | Dark | Usage |
|---|---|---|---|
| Success / Saved | `#167231` | `#32d74b` | Completed jobs, savings |
| Error / Failed | `#c92a2a` | `#ff6961` | Failed jobs, errors |
| Warning / Notice | `#8a5300` | `#ffd60a` | Amber — notices & size increases |

---

## 3. Typographic Architecture

Refract uses the **system font stack** (SF Pro on Apple platforms, Segoe UI on Windows) with optical sizing enabled. No custom web fonts are loaded.

- **Interface Family:** `-apple-system`, `BlinkMacSystemFont`, `'SF Pro Text'`, `'SF Pro UI'`, `'Segoe UI'`, sans-serif
- **Technical/Figure Family:** `'SF Mono'`, `ui-monospace`, `Menlo`, monospace
- **Optical Sizing:** `font-optical-sizing: auto` on all text
- **Hierarchy:**
  - Page/Tool Title: 22px–28px / 1.15 line-height, weight 500, letter-spacing `-0.02em`
  - Subhead / Blurb: 13px–14px / 1.55 line-height, weight 400, secondary text color
  - Card & Row Titles: 12px–14px / 1.35 line-height, weight 500
  - Technical Metrics: 9px–12px monospace, tabular numbers (`font-variant-numeric: tabular-nums`)
  - Meta & Badges: 9px–11px monospace, sentence case
- **Banned:** Uppercase tracked labels (`TRACKED_CAPS`), font weights above 600, negative letter spacing on small text, more than two font sizes per section.

---

## 4. Component Behaviors & Micro-Interactions

### Buttons & Interactive Controls

- **Tactile Feedback:** Every pressable element compresses with `transform: scale(0.97)` on `:active`.
- **Focus Rings:** Visible exclusively via `:focus-visible`: `outline: 2px solid var(--border-focus); outline-offset: 2px;`.
- **Hover:** Surface elevation (`surface-hover` → `surface-pressed`), no transform lift on hover.

### Sidebar

- **Material:** Translucent frosted glass with `backdrop-filter: blur(24px)` and hairline right border.
- **Active State:** Fills with accent blue (`#0071e3` / `#0a84ff`), white text.
- **Inactive State:** Text-muted, transparent background.

### Topbar

- **Material:** Translucent frosted glass with `backdrop-filter: blur(20px)`, sticky at top, hairline bottom border.

### Dropzone

- Tactile solid perimeter with hairline borders (no dashed borders).
- Hover state: surface elevation. Drag-over state: accent border with accent-soft background.
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
- State dot (5px) indicates job state: amber = developing, blue = processing, green = saved, red = failed.

---

## 5. Layout & Spatial Principles

- **Geometry:** Sticky topbar (56px) + shell container (max-width `1440px`, page padding `32px 28px`).
- **Sidebar:** Fixed `240px` width, translucent material, hidden on mobile (< 940px).
- **Strict 4px Scale:** Margins, paddings, and gaps strictly use multiples of 4 (4, 8, 12, 16, 20, 24, 32, 48, 64px).
- **Nested Radii Math:**
  - Panels: 13px (`--radius-lg`)
  - Cards & Dropzone: 10px (`--radius`)
  - Buttons, Inputs, Rows: 7px (`--radius-sm`)
  - Small pills & tags: 6px
  - Math rule: `inner_radius = outer_radius − padding`.
- **Mobile Adaptations (< 640px):**
  - Sticky frosted bottom bar for thumb navigation across domain tabs.
  - Multi-column grids collapse to single column.
  - Interactive targets maintain ≥ 40px tap area.

---

## 6. Motion Philosophy

- **Single Easing Curve:** `cubic-bezier(0.22, 1, 0.36, 1)` (critically damped, no overshoot) for all interactive states.
- **Durations:**
  - Micro-interactions (hover, active press): 150ms.
  - Entrances, toasts, dialogs: 300ms.
  - Backdrops: 300ms fade.
- **Hardware Acceleration:** Transitions operate exclusively on `transform`, `opacity`, `color`, `background-color`, and `border-color`.
- **Reduced Motion:** When `prefers-reduced-motion: reduce` is detected, animations collapse to simple opacity fades; dialogs and overlays appear without transform.
- **Reduced Transparency:** When `prefers-reduced-transparency: reduce`, translucent surfaces become solid.

---

## 7. Explicit Anti-Patterns (Banned)

- No purple/violet AI glow gradients or neon buttons.
- No emojis used as interface icons.
- No `transition: all` catch-alls.
- No floating orbs, decorative mesh gradients, or holographic shapes.
- No uppercase tracked labels.
- No overlapping or unconstrained absolute elements.
- No glowing pills or tinted badges.
- More than one accent color per screen.
