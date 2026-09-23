# Dray design system

A portable specification of Dray's visual language: stack, philosophy, tokens, typography, components and rules. Written so any agent, on any model, can rebuild the same look in another project without reading Dray's source. Every value here is copied from `apps/desktop/src/App.css` and `apps/desktop/src/components/ui/*` as of 2026-09-24. The reasoning behind each decision lives in `apps/desktop/DESIGN.md`; this file holds the result.

## 1. Stack

| Layer | Choice | Notes |
|---|---|---|
| Framework | React 19 + TypeScript, Vite | Tauri 2 webview on desktop; same bundle serves the phone client |
| Styling | Tailwind CSS v4 via `@tailwindcss/vite` | No `tailwind.config.*`. All theme config lives in CSS (`@theme inline`) |
| Component base | shadcn/ui, style `radix-nova`, base colour `neutral`, CSS variables on | `components.json` at the app root. Generated primitives live in `src/components/ui/` |
| Headless primitives | `radix-ui` (single package import) | `Slot`, `DropdownMenu`, `Dialog`, `AlertDialog`, `Tooltip`, `Switch`, `ContextMenu` |
| Variants | `class-variance-authority` (`cva`) + `clsx` + `tailwind-merge` via `cn()` | `cn` extends tailwind-merge so custom `text-chat/code/tool/ui/composer` sizes are not dropped by a `text-<colour>` |
| Animation | `tw-animate-css` | `animate-in`, `fade-in-0`, `zoom-in-95`, `slide-in-from-*` on menus, dialogs, tooltips |
| Fonts | `@fontsource-variable/geist`, `@fontsource-variable/geist-mono` | Self-hosted, imported in CSS |
| Icons | `lucide-react` | Default glyph `size-4` (16px) inside buttons, `size-3`/`size-3.5` in xs/sm |
| File icons | `vscode-material-icons` | Tree and tool rows |
| Avatars | `blobatar` / `@blobatar/react` | Session avatars |
| Working indicator | `thinking-orbs` | Wrapped in `Orb.tsx` which feeds it the resolved light/dark mode |
| Markdown | `streamdown` + `@streamdown/code` | Code fences highlighted through one shared Shiki instance |
| Diffs / code view | `@pierre/diffs` | Shared Shiki highlighter, WASM Oniguruma engine, worker pool |

**What is custom and what is not.** Everything visual is Tailwind utility classes plus CSS custom properties. There is no in-house component library and no CSS-in-JS. The `src/components/ui/` folder is shadcn-generated code, edited in place: `button`, `input`, `kbd`, `tooltip`, `switch`, `dialog`, `alert-dialog`, `dropdown-menu`, `context-menu`, `alert`, `spinner`, `questionnaire`. Two small app-level controls are hand-written on the same tokens: `Segmented.tsx` (sliding-thumb two-way switch) and `TabButton.tsx`. Everything else is a feature component composed from these.

## 2. Philosophy

1. **One radius scale, and almost everything is a rounded square.** Never pill, never sharp. `--radius: 0.625rem` is the single knob.
2. **Filled buttons carry a shadow; chrome does not.** Only the `default` (filled) variant gets `--shadow-button`. `ghost` and `outline` stay flat. Shadows drop straight down (x offset 0), no spread.
3. **Colour is three layers.** `:root` holds aliases and what never changes. `[data-mode]` holds the lightness ramp and veils. `[data-theme][data-mode]` holds a palette as about thirteen values. A theme cannot set `--card` and forget `--popover`, because both are aliases of `--surface-card`.
4. **Light mode is a second palette, never an inverted filter.** Rungs reorder: `--surface-raised` sits between background and card in dark, below background in light. `--secondary` is `--muted` in dark and `--surface-card` in light.
5. **Veil direction follows meaning, not mode.** A surface that means "raised" is veiled white in both modes. One that means "inset or highlighted" is veiled black in both modes. `--surface-well` is a black scrim in both.
6. **No hardcoded colours in components.** No `oklch(1 0 0 / N%)`, no `emerald-500`, no `white/10`. Every colour is a token that flips with mode. Literals vanish silently in the other mode.
7. **Shadow is light mode's only depth.** Dark surfaces read as raised by being lighter; every shadow token is `none` in dark except `--shadow-button`. Light has no room above near-white, so shadows do the separating there.
8. **Two kinds of floating veil.** A surface *in* the page (`--card`) takes `--veil-card`, a little white. A surface floating *over* the page (`--popover`, `--composer`) takes `--veil-float`, a wash of its own colour, and must carry `backdrop-blur-xl` to stay readable.
9. **One accent means one thing.** Yellow (`--accent-command`) is "this is for you": a session waiting on the reader. Green (`--accent-add`) is added lines, open PR, unread session. Purple (`--accent-merged`) is merged. Red (`--destructive`) is failure or deletion. Never spend the yellow on a decoration.
10. **Shortcuts live in real tooltips with keycaps, never on `title`.** Plain name on `aria-label`. `title` is reserved for text the app truncates.
11. **Presence is the message.** A qualifier like "Fast" or "free" beside a model name is a muted word; its being there says the thing. No second colour to say it again.
12. **Sections are separated by space, never by rules.** Three horizontal lines in a 32rem pane read as a table of contents.
13. **Disabled, not hidden.** A setting that cannot apply on this platform is drawn disabled with its reason underneath; a row that vanishes reads as forgotten.
14. **Every control that looks clickable does something.** No chevron on a single non-collapsible item, no button that redraws the same screen.
15. **No dash characters in UI strings.** No em dash, no en dash, no spaced hyphen inside text a user reads. Rewrite with a comma, colon or second sentence.

## 3. Tokens

Paste this block as the start of your global CSS. It is self-contained: Tailwind v4 imports, three colour layers, the shipped default palette (Gruvbox, section 3.4), typography and radius scale. The other four palettes Dray ships are in section 3.6; ship all of them, the theme picker in section 12 expects the full list.

### 3.1 Imports and root aliases

```css
@import "tailwindcss";
@import "tw-animate-css";
@import "@fontsource-variable/geist";
@import "@fontsource-variable/geist-mono";

@custom-variant dark (&:is(.dark *));

:root {
  --radius: 0.625rem;

  --sidebar: transparent;
  --card: var(--surface-card);
  --popover: var(--surface-card);
  --composer: var(--surface-card);
  --picker: var(--surface-card);
  --accent: var(--muted);
  --card-foreground: var(--foreground);
  --popover-foreground: var(--foreground);
  --secondary-foreground: var(--foreground);
  --accent-foreground: var(--foreground);
  --sidebar-foreground: var(--foreground);
  --sidebar-primary: var(--primary);
  --sidebar-primary-foreground: var(--primary-foreground);
  --button-primary: var(--primary);
  --button-primary-foreground: var(--primary-foreground);
  --sidebar-accent: var(--surface-selected);
  --sidebar-accent-foreground: var(--foreground);
  --sidebar-ring: var(--ring);
  --chart-1: var(--foreground);
  --chart-2: var(--ring);
  --chart-3: var(--muted-foreground);
  --chart-4: var(--muted);
  --chart-5: var(--surface-card);

  --backdrop-top: var(--background);
  --backdrop-bottom: var(--background);
  --vibrancy-alpha: 80%;

  --accent-merge: oklch(0.546 0.139 149);
  --accent-merge-hover: oklch(0.617 0.152 149);
  --accent-merge-foreground: oklch(0.985 0 0);
  --accent-merge-seam: inset 1px 0 1px oklch(1 0 0 / 10%);
}
```

### 3.2 Dark ramp

The ramp is derived: fixed lightness per rung, chroma and hue set by the theme. `--chroma: 0` collapses every `calc()` to pure grey, which is the Dray grey palette (`data-theme="default"`). Every palette falls through to this ramp for any token it does not name, so the ramp ships even when Gruvbox is the default.

```css
[data-mode="dark"] {
  --hue: 0;
  --chroma: 0;

  --background: oklch(0.145 calc(var(--chroma) * 0.8) var(--hue));
  --foreground: oklch(0.985 calc(var(--chroma) * 0.12) var(--hue));
  --surface-card: oklch(0.205 calc(var(--chroma) * 1) var(--hue));
  --surface-raised: oklch(0.19 calc(var(--chroma) * 0.9) var(--hue));
  --muted: oklch(0.269 calc(var(--chroma) * 1.1) var(--hue));
  --secondary: var(--muted);
  --surface-selected: color-mix(in oklab, var(--foreground) 15%, var(--background));
  --muted-foreground: oklch(0.708 calc(var(--chroma) * 0.9) var(--hue));
  --primary: oklch(0.922 calc(var(--chroma) * 0.25) var(--hue));
  --primary-foreground: oklch(0.205 calc(var(--chroma) * 1) var(--hue));
  --ring: oklch(0.556 calc(var(--chroma) * 1) var(--hue));
  --destructive: oklch(0.704 0.191 22.216);

  --accent-add: oklch(0.7 0.15 163);
  --accent-merged: oklch(0.63 0.23 304);
  --border: oklch(1 0 0 / 10%);
  --input: oklch(1 0 0 / 15%);
  --sidebar-border: oklch(1 0 0 / 8%);
  --hairline: oklch(1 0 0 / 6%);
  --hairline-strong: oklch(1 0 0 / 8%);

  --accent-thinking: oklch(0.62 0.04 264);
  --accent-command: oklch(0.82 0.13 90);
  --accent-mention: oklch(0.82 0.13 230);
  --accent-issue: oklch(0.82 0.13 310);
  --accent-session: oklch(0.82 0.13 160);

  --surface-well: oklch(0 0 0 / 22%);
  --surface-thumb: color-mix(in oklab, oklch(1 0 0) 16%, var(--surface-card));
  --veil-card: oklch(1 0 0 / 5.5%);
  --veil-raised: oklch(1 0 0 / 4%);
  --veil-strong: oklch(1 0 0 / 11.5%);
  --veil-selected: oklch(1 0 0 / 11.5%);
  --veil-float: color-mix(in oklab, var(--surface-card) 62%, transparent);
  --veil-panel: color-mix(in oklab, var(--surface-card) 76%, transparent);

  --shadow-button: 0 1px 4px oklch(0 0 0 / 45%);
  --shadow-surface: none;
  --edge-surface: var(--hairline);
  --shadow-card: none;
}
```

### 3.3 Light ramp

```css
[data-mode="light"] {
  --hue: 0;
  --chroma: 0;

  --background: oklch(0.965 calc(var(--chroma) * 0.4) var(--hue));
  --foreground: oklch(0.22 calc(var(--chroma) * 0.5) var(--hue));
  --surface-card: oklch(1 calc(var(--chroma) * 0.2) var(--hue));
  --surface-raised: oklch(0.94 calc(var(--chroma) * 0.3) var(--hue));
  --muted: oklch(0.92 calc(var(--chroma) * 0.5) var(--hue));
  --secondary: var(--surface-card);
  --surface-selected: color-mix(in oklab, var(--foreground) 10%, var(--background));
  --muted-foreground: oklch(0.52 calc(var(--chroma) * 0.7) var(--hue));
  --primary: oklch(0.28 calc(var(--chroma) * 0.5) var(--hue));
  --primary-foreground: oklch(0.99 calc(var(--chroma) * 0.1) var(--hue));
  --ring: oklch(0.62 calc(var(--chroma) * 0.9) var(--hue));
  --destructive: oklch(0.55 0.2 25);

  --accent-add: oklch(0.585 0.174 163);
  --accent-merged: oklch(0.56 0.2 304);
  --border: oklch(0 0 0 / 10%);
  --input: oklch(0 0 0 / 14%);
  --sidebar-border: oklch(0 0 0 / 8%);
  --hairline: oklch(0 0 0 / 9%);
  --hairline-strong: oklch(0 0 0 / 13%);

  --accent-thinking: oklch(0.55 0.05 264);
  --accent-command: oklch(0.65 0.14 78);
  --accent-mention: oklch(0.55 0.16 245);
  --accent-issue: oklch(0.55 0.16 310);
  --accent-session: oklch(0.55 0.16 160);

  --surface-well: oklch(0 0 0 / 7%);
  --surface-thumb: var(--surface-card);
  --veil-card: oklch(1 0 0 / 60%);
  --veil-raised: oklch(0 0 0 / 4%);
  --veil-strong: oklch(0 0 0 / 10%);
  --veil-selected: oklch(0 0 0 / 10%);
  --veil-float: color-mix(in oklab, var(--surface-card) 74%, transparent);
  --veil-panel: color-mix(in oklab, var(--surface-card) 84%, transparent);

  --shadow-button: 0 1px 3px oklch(0 0 0 / 12%);
  --accent-merge: oklch(0.59 0.155 149);
  --accent-merge-hover: oklch(0.655 0.165 149);
  --shadow-surface:
    0 1px 1px oklch(0 0 1 / 8%), 0 1px 1px oklch(0 0 0 / 10%), 0 1px 4px oklch(0 0 0 / 8%);
  --edge-surface: transparent;
  --shadow-card: 0 1px 1px oklch(0 0 1 / 4%), 0 1px 1px oklch(0 0 0 / 6%), 0 1px 4px oklch(0 0 0 / 4%);
  --vibrancy-alpha: 70%;
}
```

### 3.4 Shipped default palette: Gruvbox

Dray's default theme is Gruvbox, both modes, ported from https://github.com/morhetz/gruvbox (MIT). Its background is a true neutral (chroma 0.000) with all the warmth in the cream foreground, which is why it is a palette block and not a hue-and-chroma ramp: a derived ramp would tint the background to match the text, the one thing Gruvbox never does. `DEFAULT_THEME` in the registry (3.6) and the fallback in the pre-paint script (3.8) both say `gruvbox`.

Each palette block names its surfaces, text, ring, red and the three prose accents, then stops. Green (`--accent-add`), purple (`--accent-merged`), the merge button and every veil keep the ramp's values, so they still mean GitHub's colours on every theme.

```css
/* Gruvbox dark: bg0_h is the hard background and takes the page, bg0 the raised
   surface, bg0_s the card. fg1 is prose, fg4 the muted text. */
[data-theme="gruvbox"][data-mode="dark"] {
  --background: #1d2021;        /* bg0_h */
  --surface-raised: #282828;    /* bg0 */
  --surface-card: #32302f;      /* bg0_s */
  --muted: #3c3836;             /* bg1 */
  --muted-foreground: #a89984;  /* fg4 */
  --foreground: #ebdbb2;        /* fg1 */
  --primary: #ebdbb2;           /* fg1 */
  --primary-foreground: #1d2021; /* bg0_h */
  --ring: #7c6f64;              /* bg4 */
  --destructive: #fb4934;       /* bright red */
  --accent-command: #fabd2f;    /* bright yellow */
  --accent-mention: #83a598;    /* bright blue */
  --accent-thinking: #928374;   /* gray */
  --vibrancy-alpha: 70%;
}

/* Gruvbox light: the same three background tokens re-dealt. bg0_h is the
   lightest here, so it takes the card either way, and bg0_s (soft) drops below
   the page to be the inset. Reds, yellows and blues are the faded set. */
[data-theme="gruvbox"][data-mode="light"] {
  --background: #fbf1c7;        /* bg0 */
  --surface-raised: #f2e5bc;    /* bg0_s */
  --surface-card: #f9f5d7;      /* bg0_h */
  --muted: #ebdbb2;             /* bg1 */
  --muted-foreground: #7c6f64;  /* fg4 */
  --foreground: #3c3836;        /* fg1 */
  --primary: #3c3836;           /* fg1 */
  --primary-foreground: #fbf1c7; /* bg0 */
  --ring: #7c6f64;              /* fg4 */
  --destructive: #9d0006;       /* faded red */
  --accent-command: #b57614;    /* faded yellow */
  --accent-mention: #076678;    /* faded blue */
  --accent-thinking: #928374;   /* gray */
  --vibrancy-alpha: 50%;
}
```

Filled buttons on Gruvbox take `--primary` (the cream on dark, the dark brown on light) through `--button-primary`, which aliases `--primary` unless a palette says otherwise. Only the Dray grey palette overrides that alias (3.6).

### 3.5 Tailwind theme mapping and typography

This is what turns the tokens into utilities (`bg-card`, `text-muted-foreground`, `rounded-lg`, `text-chat`, `shadow-(--shadow-button)`).

```css
@theme inline {
  --font-sans: 'Geist Variable', sans-serif;
  --font-heading: var(--font-sans);
  --font-mono: 'Geist Mono Variable', ui-monospace, monospace;

  /* Semantic text sizes. `--fs-*` are runtime overrides written on <html>
     from the reader's font-size settings; the fallback is the default. */
  --text-chat: var(--fs-chat, 0.9375rem);
  --text-chat--line-height: 1.65;
  --text-code: var(--fs-code, 0.875rem);
  --text-code--line-height: 1.6;
  --text-tool: var(--fs-tool, 0.8125rem);
  --text-tool--line-height: 1.55;
  --text-ui: var(--fs-ui, 0.8125rem);
  --text-ui--line-height: 1.4;
  --text-prompt: var(--fs-prompt, 0.9375rem);
  --text-prompt--line-height: 1.5;

  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-composer: var(--composer);
  --color-picker: var(--picker);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-button-primary: var(--button-primary);
  --color-button-primary-foreground: var(--button-primary-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-destructive: var(--destructive);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --color-hairline: var(--hairline);
  --color-hairline-strong: var(--hairline-strong);
  --color-edge-surface: var(--edge-surface);
  --color-veil-strong: var(--veil-strong);
  --color-veil-selected: var(--veil-selected);
  --color-surface-raised: var(--surface-raised);
  --color-surface-well: var(--surface-well);
  --color-surface-thumb: var(--surface-thumb);
  --color-surface-card: var(--surface-card);
  --color-accent-add: var(--accent-add);
  --color-accent-merged: var(--accent-merged);
  --color-accent-thinking: var(--accent-thinking);
  --color-accent-command: var(--accent-command);
  --color-accent-mention: var(--accent-mention);
  --color-accent-issue: var(--accent-issue);
  --color-accent-session: var(--accent-session);
  --color-accent-merge: var(--accent-merge);
  --color-accent-merge-hover: var(--accent-merge-hover);
  --color-accent-merge-foreground: var(--accent-merge-foreground);
  --color-sidebar: var(--sidebar);
  --color-sidebar-foreground: var(--sidebar-foreground);
  --color-sidebar-primary: var(--sidebar-primary);
  --color-sidebar-primary-foreground: var(--sidebar-primary-foreground);
  --color-sidebar-accent: var(--sidebar-accent);
  --color-sidebar-accent-foreground: var(--sidebar-accent-foreground);
  --color-sidebar-border: var(--sidebar-border);
  --color-sidebar-ring: var(--sidebar-ring);
  --color-chart-1: var(--chart-1);
  --color-chart-2: var(--chart-2);
  --color-chart-3: var(--chart-3);
  --color-chart-4: var(--chart-4);
  --color-chart-5: var(--chart-5);

  --radius-sm: calc(var(--radius) * 0.6);   /* 6px  */
  --radius-md: calc(var(--radius) * 0.8);   /* 8px  */
  --radius-lg: var(--radius);               /* 10px */
  --radius-xl: calc(var(--radius) * 1.4);   /* 14px */
  --radius-2xl: calc(var(--radius) * 1.8);  /* 18px */
  --radius-3xl: calc(var(--radius) * 2.2);
  --radius-4xl: calc(var(--radius) * 2.6);

  --animate-shimmer: shimmer 2s linear infinite;
  @keyframes shimmer {
    from { background-position: 200% 0; }
    to   { background-position: -200% 0; }
  }
}

@layer base {
  * { @apply border-border outline-ring/50; }
  html {
    @apply font-sans;
    --titlebar-h: 40px;
    --traffic-lights-w: 78px;
    --window-controls-w: 96px;
  }
  body {
    @apply text-foreground overscroll-none;
    background-color: var(--background);
    background-image: linear-gradient(to bottom, var(--backdrop-top), var(--backdrop-bottom));
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }
  html, body, #root { @apply h-full overflow-hidden; }
}
```

**Font defaults in px:** interface 13, chat 15, chat input 15, code 14. Reader-adjustable 10 to 24, written as `--fs-ui`, `--fs-chat`, `--fs-prompt`, `--fs-code` on `<html>`. Under 820px (phone) the defaults rise: ui 15px, chat 16px, prompt 16px, code 13px, titlebar 3rem.

**Where each size is used:** `text-ui` for chrome (sidebar rows, tabs, dialog titles, menu items), `text-chat` for assistant prose, `text-prompt` for the composer and user bubble, `text-tool` for tool rows and their output, `text-code` for code blocks and diffs. Default Tailwind `text-sm` (14px) and `text-xs` (12px) appear inside shadcn primitives.

### 3.6 Other shipped palettes and the theme registry

Ship all five. A palette that is not in the registry cannot be picked, and a stored pick that the registry does not know falls back to `DEFAULT_THEME` (`coerceTheme`). Order below is the order the theme picker draws them.

```ts
// lib/theme.ts (trimmed to what a port needs)
export type ThemeName = "gruvbox" | "default" | "catppuccin" | "one-dark-pro" | "cobalt2";
export type ThemeMode = "light" | "dark" | "system";

export type Theme = { id: ThemeName; label: string; darkOnly?: boolean; credit?: { name: string; url: string } };

export const THEMES: Theme[] = [
  { id: "gruvbox", label: "Gruvbox", credit: { name: "gruvbox", url: "https://github.com/morhetz/gruvbox" } },
  { id: "default", label: "Dray" },
  { id: "catppuccin", label: "Catppuccin", credit: { name: "Catppuccin", url: "https://github.com/catppuccin/catppuccin" } },
  { id: "one-dark-pro", label: "One Dark Pro", darkOnly: true, credit: { name: "One Dark Pro", url: "https://github.com/Binaryify/OneDark-Pro" } },
  { id: "cobalt2", label: "Cobalt2", darkOnly: true, credit: { name: "Cobalt2", url: "https://github.com/wesbos/cobalt2-vscode" } },
];

export const DEFAULT_THEME: ThemeName = "gruvbox";
const DEFAULT_MODE: ThemeMode = "light";
const THEME_KEY = "ade.theme";
const MODE_KEY = "ade.mode";

export function hasLightMode(name: ThemeName) { return !THEMES.find((t) => t.id === name)?.darkOnly; }
export function modeFor(name: ThemeName, mode: ThemeMode): ThemeMode { return hasLightMode(name) ? mode : "dark"; }
export function coerceTheme(raw: string | null): ThemeName {
  return THEMES.some((t) => t.id === raw) ? (raw as ThemeName) : DEFAULT_THEME;
}
export function applyTheme(name: ThemeName, mode: ThemeMode): "light" | "dark" {
  const resolved = mode === "system" ? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : mode;
  const el = document.documentElement;
  el.dataset.theme = name;
  el.dataset.mode = resolved;
  el.classList.toggle("dark", resolved === "dark");   // shadcn and Streamdown style through `dark:`
  el.style.colorScheme = resolved;                     // native form controls and scrollbars follow
  try { localStorage.setItem(THEME_KEY, name); localStorage.setItem(MODE_KEY, mode); } catch {}
  return resolved;
}
```

Rules the registry encodes: `darkOnly` is an opt-out, so a new palette gets both modes by saying nothing. `modeFor` runs on every write and on the first read, so a dark-only theme can never be stamped with `data-mode="light"` (which matches no block and falls through to the light ramp's neutrals). `useTheme` is one module-level store, never per-component state, or two copies disagree the first time the OS colour scheme changes.

**Dray grey (`default`).** Dark is the pure grey ramp and sets nothing but vibrancy. Light is a cool near-white with a real blue `--primary` reserved for the Send button; every other filled button takes the neutral `--button-primary`. This is the one palette that fills the sent bubble.

```css
[data-theme="default"][data-mode="dark"] {
  --vibrancy-alpha: 70%;
}

[data-theme="default"][data-mode="light"] {
  --background: oklch(0.965 0.012 250);
  --surface-raised: oklch(0.94 0.01 250);
  --surface-card: oklch(0.99 0.004 250);
  --muted: oklch(0.92 0.01 250);
  --muted-foreground: oklch(0.52 0.01 250);
  --foreground: oklch(0.22 0.01 250);
  --primary: oklch(0.65 0.17 250);
  --primary-foreground: oklch(0.99 0 0);
  --button-primary: oklch(0.28 0.01 250);
  --button-primary-foreground: var(--surface-card);
  --ring: var(--primary);
  --destructive: oklch(0.55 0.2 25);
  --accent-command: oklch(0.65 0.14 78);
  --accent-mention: oklch(0.55 0.16 245);
  --accent-thinking: oklch(0.55 0.05 264);
  --surface-selected: oklch(0.9 0.012 250);
  --vibrancy-alpha: 50%;
  --veil-card: oklch(0.99 0.004 250 / 60%);
  --veil-raised: color-mix(in oklab, var(--foreground) 4.5%, transparent);
  --veil-strong: color-mix(in oklab, var(--foreground) 11.5%, transparent);
  --veil-selected: color-mix(in oklab, var(--foreground) 11.5%, transparent);
}

/* Dray light fills the sent bubble with the blue primary, so it is a dark
   surface inside a light window: tags inside it take brighter accents, and two
   hues rotate so they sit off the blue (mention goes cyan, issue goes pink). */
[data-theme="default"][data-mode="light"] .user-bubble {
  background: var(--primary);
  color: var(--primary-foreground);
  box-shadow: none;
  --accent-command: oklch(0.85 0.19 75);
  --accent-mention: oklch(0.92 0.19 205);
  --accent-issue: oklch(1 0.195 330);
  --accent-session: oklch(0.95 0.19 150);
}
```

**Catppuccin (Mocha dark, Latte light), One Dark Pro (dark only), Cobalt2 (dark only).**

```css
[data-theme="catppuccin"][data-mode="dark"] {
  --background: #181825; --surface-raised: #1e1e2e; --surface-card: #313244;
  --muted: #45475a; --muted-foreground: #a6adc8; --foreground: #cdd6f4;
  --primary: #cdd6f4; --primary-foreground: #181825; --ring: #7f849c;
  --destructive: #f38ba8; --accent-command: #f9e2af; --accent-mention: #89b4fa;
  --accent-thinking: #9399b2; --vibrancy-alpha: 70%;
}
[data-theme="catppuccin"][data-mode="light"] {
  --background: #e6e9ef; --surface-raised: #dce0e8; --surface-card: #eff1f5;
  --muted: #ccd0da; --muted-foreground: #6c6f85; --foreground: #4c4f69;
  --primary: #4c4f69; --primary-foreground: #eff1f5; --ring: #8c8fa1;
  --destructive: #d20f39; --accent-command: #df8e1d; --accent-mention: #1e66f5;
  --accent-thinking: #8c8fa1; --vibrancy-alpha: 50%;
}
[data-theme="one-dark-pro"][data-mode="dark"] {
  --background: #1e2227; --surface-raised: #23272e; --surface-card: #323842;
  --muted: #3e4452; --muted-foreground: #9da5b4; --foreground: #d7dae0;
  --primary: #d7dae0; --primary-foreground: #1e2227; --ring: #4d78cc;
  --destructive: #e06c75; --accent-command: #e5c07b; --accent-mention: #61afef;
  --accent-thinking: #7f848e; --vibrancy-alpha: 80%;
}
[data-theme="cobalt2"][data-mode="dark"] {
  --background: #193549; --surface-raised: #243e51; --surface-card: #1f4662;
  --muted: #355166; --muted-foreground: #aaa; --foreground: #fff;
  --primary: #fff; --primary-foreground: #193549; --ring: #437da3;
  --destructive: #ff628c; --accent-command: #ffc600; --accent-mention: #9effff;
  --accent-thinking: #0088ff; --vibrancy-alpha: 65%;
}
```

A dark-only theme in light mode falls through to the light ramp's neutrals, so mark it `darkOnly` and disable the mode picker with a sentence saying why rather than inventing a light side. One Dark Pro's foreground pair is deliberately not upstream's editor colours: `#abb2bf` sits below the lightness this app uses for secondary labels, so `#d7dae0` takes foreground and `#9da5b4` takes muted.

### 3.7 Transparency (glass) layer

Windowed, the app is translucent and surfaces become veils over the backdrop. This is the ordinary state, stamped before first paint as `data-transparency` on `<html>`. On macOS `data-vibrancy` is added too and the body itself goes see-through. Skip this section for an ordinary web app.

```css
html[data-transparency] body {
  --card: var(--veil-card);
  --popover: var(--veil-float);
  --composer: var(--veil-float);
  --picker: var(--veil-panel);
  --sidebar-accent: var(--veil-selected);
  --surface-raised: var(--veil-raised);
  --muted: var(--veil-strong);
}
html[data-vibrancy][data-transparency] body {
  background-color: transparent;
  background-image: linear-gradient(
    to bottom,
    color-mix(in oklab, var(--backdrop-top) var(--vibrancy-alpha), transparent),
    color-mix(in oklab, var(--backdrop-bottom) var(--vibrancy-alpha), transparent)
  );
}
```

Every floating frame (menu, tooltip, dialog, composer) carries `backdrop-blur-xl`. Without blur a 62% wash reads as a slab.

### 3.8 Pre-paint script

Theme, mode and font sizes are stamped on `<html>` before React mounts so a stored theme never flashes. The fallback theme here must equal `DEFAULT_THEME` in the registry, or the first frame draws one palette and React swaps in another. Local storage keys: `ade.theme` (palette id), `ade.mode` (`light`, `dark`, `system`), `ade.fontSizes` (JSON of px per slot).

```html
<script>
  try {
    var t = localStorage.getItem("ade.theme") || "gruvbox";
    var m = localStorage.getItem("ade.mode") || "light";
    var dark = m === "dark" ||
      (m === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
    var el = document.documentElement;
    el.dataset.theme = t;
    el.dataset.mode = dark ? "dark" : "light";
    el.classList.toggle("dark", dark);
    el.style.colorScheme = dark ? "dark" : "light";
    el.dataset.transparency = "";
    if (/Mac/.test(navigator.platform)) el.dataset.vibrancy = "";
  } catch (e) {}
  try {
    var sizes = JSON.parse(localStorage.getItem("ade.fontSizes") || "{}");
    ["chat", "code", "prompt", "ui"].forEach(function (slot) {
      var px = sizes[slot];
      if (typeof px === "number" && isFinite(px)) {
        px = Math.min(24, Math.max(10, Math.round(px)));
        document.documentElement.style.setProperty("--fs-" + slot, px + "px");
      }
    });
  } catch (e) {}
</script>
```

Rules that fail silently if broken: palette overrides go on `body`, never `html` (the two-attribute palette selector out-specifies any `html[...]`). The ramp lives on `[data-mode]`, never `:root`. A theme swatch must carry its own `data-theme` and `data-mode` and re-declare both backdrop stops.

## 4. Scales

### Radius

| Utility | Value | Used for |
|---|---|---|
| `rounded-sm` | 6px | keycaps, dialog close cross |
| `rounded-md` | 8px | menu items, small tiles, tab buttons, tooltips, code blocks |
| `rounded-lg` | 10px | buttons, inputs, menus, popovers |
| `rounded-xl` | 14px | dialogs, cards, user bubble |
| `rounded-2xl` | 18px | composer card, drop overlay |
| `rounded-full` | pill | segmented control track and thumb, switch only |

Small buttons clamp the radius to the height: `xs` and `icon-xs` use `rounded-[min(var(--radius-md),10px)]`, `sm` and `icon-sm` use `rounded-[min(var(--radius-md),12px)]`. An icon button therefore needs no radius class of its own.

### Sizing

| Element | Height | Notes |
|---|---|---|
| Button default | `h-8` (32px) | `px-2.5 gap-1.5 text-sm` |
| Button sm | `h-7` (28px) | `px-2.5 text-[0.8rem]`, icons `size-3.5` |
| Button xs | `h-6` (24px) | `px-2 text-xs`, icons `size-3` |
| Icon buttons | `size-8`, `size-7`, `size-6` | square |
| Input | `h-8` | `px-2.5 py-1 rounded-lg` |
| Keycap | `h-5 min-w-5 px-1` | `rounded-sm text-xs` |
| Switch (settings) | `h-4 w-7`, thumb `size-3.5` | composer toggle one rung down at `h-3 w-5` |
| Titlebar row | `40px` (`--titlebar-h`) | the whole drag region |
| Menu content | `min-w-32 p-1` | items `px-1.5 py-1 gap-1.5 text-sm` |
| Dialog | `max-w-100 p-5 gap-4` (25rem) | settings 44rem; user bubble `max-w-[85%]` |
| Crew column | 320px fixed | |
| Phone breakpoint | `< 820px` | sidebar becomes a drawer, panel takes the window |

### Shadows

| Token | Dark | Light | On |
|---|---|---|---|
| `--shadow-button` | `0 1px 4px oklch(0 0 0 / 45%)` | `0 1px 3px oklch(0 0 0 / 12%)` | filled `default` buttons, segmented thumb |
| `--shadow-surface` | `none` | three crisp 1px layers | secondary buttons, composer, surfaces at the window edge |
| `--shadow-card` | `none` | three soft 1px layers | user bubble, plan card |
| `--edge-surface` | `--hairline` | `transparent` | 1px border partner of `--shadow-surface`, kept 1px in both modes so focus never reflows |

Tailwind `shadow-lg` appears on menus and dialogs, `shadow-xs` on tooltips. Use tokens as `shadow-(--shadow-button)`.

### Hairlines and borders

`--border` 10% (both modes), `--input` 15% dark / 14% light, `--hairline` 6% / 9%, `--hairline-strong` 8% / 13%, `--sidebar-border` 8%. White alpha in dark, black alpha in light. A floating menu adds `ring-1 ring-foreground/10`.

## 5. Components

Each recipe is the exact class string. Copy it, or port it with the same tokens.

### Button (`cva`)

```ts
const buttonVariants = cva(
  "group/button inline-flex shrink-0 cursor-pointer items-center justify-center rounded-lg border border-transparent text-sm font-medium whitespace-nowrap transition-all outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 active:not-aria-[haspopup]:translate-y-px disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default:
          "bg-button-primary text-button-primary-foreground shadow-(--shadow-button) hover:bg-button-primary/90",
        outline:
          "bg-clip-padding border-border bg-background hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground dark:border-input dark:bg-input/30 dark:hover:bg-input/50",
        secondary:
          "bg-secondary text-secondary-foreground shadow-(--shadow-surface) hover:bg-[color-mix(in_oklch,var(--secondary),var(--foreground)_5%)] aria-expanded:bg-secondary aria-expanded:text-secondary-foreground",
        ghost:
          "hover:bg-muted/70 hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground dark:hover:bg-muted/50",
        destructive:
          "bg-destructive/10 text-destructive hover:bg-destructive/20 focus-visible:border-destructive/40 focus-visible:ring-destructive/20 dark:bg-destructive/20 dark:hover:bg-destructive/30 dark:focus-visible:ring-destructive/40",
      },
      size: {
        default: "h-8 gap-1.5 px-2.5 has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2",
        xs: "h-6 gap-1 rounded-[min(var(--radius-md),10px)] px-2 text-xs in-data-[slot=button-group]:rounded-lg has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "h-7 gap-1 rounded-[min(var(--radius-md),12px)] px-2.5 text-[0.8rem] in-data-[slot=button-group]:rounded-lg has-data-[icon=inline-end]:pr-1.5 has-data-[icon=inline-start]:pl-1.5 [&_svg:not([class*='size-'])]:size-3.5",
        icon: "size-8",
        "icon-xs": "size-6 rounded-[min(var(--radius-md),10px)] in-data-[slot=button-group]:rounded-lg [&_svg:not([class*='size-'])]:size-3",
        "icon-sm": "size-7 rounded-[min(var(--radius-md),12px)] in-data-[slot=button-group]:rounded-lg",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  }
)
```

Rules: `cursor-pointer` lives here, not on callers. Every button has a transparent 1px border so focus can colour it without moving anything. No `bg-clip-padding` on filled variants (a rim of page shows through the border in light). `destructive` is a tint for a control among others; a solid red fill is reserved for the confirm action of an alert dialog, placed rightmost. A merge button uses `--accent-merge` (GitHub's green), never `--primary`. Hover on filled is `/90`, not `/80`.

### Input

```
h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2.5 py-1 text-base transition-colors outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:cursor-not-allowed disabled:bg-input/50 disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 md:text-sm dark:bg-input/30 dark:disabled:bg-input/80 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40
```

### Focus ring (everywhere)

`focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50`. Three pixels, half-alpha ring colour, border swaps colour. Never `outline`.

### Kbd

```
pointer-events-none inline-flex h-5 w-fit min-w-5 items-center justify-center gap-1 rounded-sm bg-muted px-1 font-sans text-xs font-medium text-muted-foreground select-none [&_svg:not([class*='size-'])]:size-3
```

`KbdGroup` is `inline-flex items-center gap-1`. Keycaps sit inside tooltips, never inline in a button label. Do not fade a keycap with `opacity` to dim it; the fill and page move in opposite directions per mode. Use `dark:opacity-50` only.

### Tooltip content

```
z-50 inline-flex w-fit max-w-xs items-center gap-1.5 rounded-md border border-border bg-popover backdrop-blur-xl px-3 py-1.5 text-xs text-popover-foreground shadow-xs has-data-[slot=kbd]:pr-1.5 has-[>[data-slot=kbd]:first-child]:pl-1.5 has-[>[data-slot=kbd-group]:first-child]:pl-1.5 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2
```

`sideOffset` 6. Returns nothing on a device that cannot hover. Not an inverted chip: same popover surface as menus.

### Dropdown / context menu

Content: `z-50 min-w-32 overflow-y-auto rounded-lg bg-popover backdrop-blur-xl p-1 text-popover-foreground shadow-lg ring-1 ring-foreground/10` plus the animate-in classes above.
Item: `relative flex cursor-default items-center gap-1.5 rounded-md px-1.5 py-1 text-sm outline-hidden select-none focus:bg-accent focus:text-accent-foreground data-inset:pl-7 data-disabled:pointer-events-none data-disabled:opacity-50 [&_svg:not([class*='size-'])]:size-4`.
A row that is a switch rather than a pick keeps the menu open and carries `role="switch"` with a `Switch` drawn `pointer-events-none` as its picture.

### Dialog

Overlay: `fixed inset-0 z-50 bg-black/50 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0`.
Content: `fixed top-1/2 left-1/2 z-50 grid w-full max-w-100 -translate-x-1/2 -translate-y-1/2 gap-4 rounded-xl border border-border bg-popover backdrop-blur-xl p-5 text-popover-foreground shadow-lg data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95`.
Title: `text-ui font-medium`. Header: `flex flex-col gap-1.5`. Close cross: `absolute top-5 right-5 rounded-sm text-muted-foreground hover:text-foreground` with a 16px `X`.
Dialogs use `--popover`, not `--card`: they float over the page. An alert dialog is the same frame without a close cross; a dialog has one.

### Switch

Track: `peer inline-flex h-4 w-7 shrink-0 cursor-pointer items-center rounded-full p-px transition-colors outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-primary data-[state=unchecked]:bg-muted-foreground/30`.
Thumb: `pointer-events-none block size-3.5 rounded-full bg-background transition-transform data-[state=checked]:translate-x-3 data-[state=unchecked]:translate-x-0`.

### Segmented (two-way, sliding thumb)

Track: `relative inline-grid shrink-0 grid-cols-2 rounded-full bg-surface-well p-0.5`.
Thumb: `absolute inset-y-0.5 left-0.5 w-[calc(50%-2px)] rounded-full bg-surface-thumb shadow-(--shadow-button) transition-transform duration-200 ease-out motion-reduce:transition-none`, translated 100% for the second segment.
Segment: `relative flex items-center justify-center whitespace-nowrap rounded-full`, inactive `text-muted-foreground hover:text-foreground`.
The thumb slides; it does not fill whichever segment is pressed. The track is a recess, so it uses the black scrim, never `--surface-raised`.

### Tab button

`rounded-md px-2 py-1 text-ui transition-colors`, active `bg-sidebar-accent text-sidebar-accent-foreground`, inactive `text-muted-foreground hover:text-foreground`. Tab rows are `h-(--titlebar-h)` and double as the drag region.

### User bubble

`user-bubble max-w-[85%] rounded-xl bg-card px-3 py-2 text-card-foreground shadow-(--shadow-card)`. Attached images sit above the bubble, outside it, as 80px squares in a wrapping row. Body clamps at twenty measured lines with a "Show more" control.

### Shimmer (working text)

```css
.shimmer-text {
  animation: var(--animate-shimmer);
  display: inline-block;
  will-change: transform;
  background-image: linear-gradient(90deg,
    var(--color-muted-foreground) 35%, var(--color-foreground) 50%, var(--color-muted-foreground) 65%);
  background-size: 200% 100%;
  background-clip: text;
  color: transparent;
}
@media (prefers-reduced-motion: reduce) { .shimmer-text { animation: none; } }
```

Muted-to-foreground only, so it reads as the same text brightening, never a tinted highlight. `display: inline-block` and `will-change: transform` are load-bearing on WebKit (repaint leak otherwise).

### Spinner

Own SVG, not lucide's `Loader2`: one circle concentric with the viewBox, `pathLength` normalised so the gap is a share of the ring, `animate-spin` on the `svg` element (a spin on the `circle` orbits the corner).

### Code block (markdown)

`border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden;` on the block, inner body with no border, no radius, transparent background. Copy actions fade in on hover (`opacity 0` to `1`, 150ms). All code goes through one shared Shiki highlighter with a light/dark theme pair; the default pair follows the app theme.

### Scrollbars

`.scrollbar-none` hides them. `.scrollbar-overlay` reserves a stable gutter and shows a thin thumb only on hover, coloured `color-mix(in oklab, var(--color-muted-foreground) 35%, transparent)`.

## 6. Layout

- **Three columns:** sidebar (transparent, reads as part of the page), main column with view tabs, right panel (one frame with a tab row, bodies carry no chrome). A fixed 320px crew column can sit beside the transcript.
- **Titlebar row** `h-(--titlebar-h)` (40px) on each column is the drag region. Leave `--traffic-lights-w` (78px) at the leading edge on macOS when the sidebar is collapsed; `--window-controls-w` (96px) at the trailing edge elsewhere.
- **Composer** is a `rounded-2xl` card on `--composer` with `backdrop-blur-xl`, `max-w-3xl`, toolbar left and dictate plus send right on a control row under the text. Before a session exists it stands alone mid-window with no fill, border or padding, toolbar above the input, and a "Press ⏎ to send" hint instead of a button.
- **Right panel** and **sidebar** hide rather than unmount. Under 820px the sidebar is a drawer over the chat and the panel takes the whole window.
- **Sections separated by space**, not rules. Group headings arrive with the second group, never before.
- **Settings dialog:** 44rem, a rail of tabs down the left with the title heading the rail, panel fixed 512px capped at 60vh and scrolling. Row shape is label and control on the top line, description full width beneath; a control wider than a switch goes under the label.

## 7. Colour usage rules

| Token | Meaning | Where |
|---|---|---|
| `--accent-command` (yellow) | waiting on the reader | sidebar rail mark, slash command chips, `Unknown` account state |
| `--accent-add` (green) | new, open, unread | added lines, open PR glyph, unread session mark, `AccountState` connected |
| `--accent-merged` (purple) | merged | merged PR glyph |
| `--accent-mention` (blue) | `@file` mention | composer and transcript |
| `--accent-issue` (pink/violet) | `#ISSUE` tag | composer and transcript |
| `--accent-session` (teal) | `&session` tag | composer and transcript |
| `--accent-thinking` (grey-blue) | reasoning text | thinking rows |
| `--accent-merge` | GitHub's merge green | the merge button only |
| `--destructive` | failure, deletion | error rows, destructive tint, alert confirm fill |
| `--surface-well` | a recess | segmented track, anything cut into a surface |
| `--surface-raised` | inset code | `<pre>` of tool arguments and output |
| `--surface-selected` / `--sidebar-accent` | selected row | sidebar and tab highlight |

Passing checks draw nothing. Red is only shown while a PR is open or draft. Yellow outranks green where both apply.

## 8. Checklist for a new screen

1. Radius from the scale; icon buttons take no radius class.
2. Filled button has `--shadow-button`; nothing else in chrome has a shadow in dark.
3. Every colour is a token; grep for `oklch(`, `white/`, `black/`, `emerald`, `gray-` in components and remove them.
4. Floating surface uses `bg-popover backdrop-blur-xl`; in-page surface uses `bg-card`.
5. Text sizes are `text-ui`, `text-chat`, `text-prompt`, `text-tool`, `text-code`.
6. Shortcut in a `Tooltip` with `Kbd`, plain name on `aria-label`, no `title`.
7. Focus is `focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50`.
8. Menus animate with `animate-in fade-in-0 zoom-in-95` and their `slide-in-from-*` per side.
9. Nothing states the same fact twice on one screen; no developer metrics in the UI.
10. No dash characters in any user-facing string.
11. Check both modes on Gruvbox and at least one other palette before handing over.
12. Before inventing a shape, find its nearest row in section 14.1 and start from that class string.

## 9. How the stock shadcn components were changed, and how to change the next one

Section 5 holds the *result*. This section holds the *pattern*, so a component not listed there (Select, Popover, Tabs, Command, Sheet, Badge, Card, Textarea) can be pulled from the shadcn registry and brought in line with the same edits.

### 9.1 What Dray changed on the generated components

| Component | Typical shadcn default | Dray's change |
|---|---|---|
| Button | radius per style, no cursor, `bg-clip-padding`, hover `/80`, shadow on none or all | `rounded-lg` frame; `cursor-pointer` in the base (callers had been adding it eight times); `bg-clip-padding` kept on `outline` only; filled variant takes `--shadow-button` and hover `/90`; fill is `bg-button-primary` not `bg-primary`; `secondary` takes `--shadow-surface` and a `color-mix` hover; `destructive` is a tint (`/10`, `/20`), never a solid fill; small sizes clamp radius to height |
| Input | `rounded-md`, `h-9` | `rounded-lg`, `h-8`, `px-2.5` |
| Kbd | translucent white chip inside an inverted tooltip | `bg-muted text-muted-foreground`, same fill everywhere, since tooltips are no longer inverted |
| Tooltip | inverted (dark chip on light page), arrow, `sideOffset 0`, `text-primary-foreground` | same surface as menus (`bg-popover border-border backdrop-blur-xl`), **no arrow**, `sideOffset 6`, `shadow-xs`; returns `null` where the device cannot hover; padding tightens when a keycap is the first child |
| Dropdown / Context menu | `rounded-md` content, opaque popover | `rounded-lg bg-popover backdrop-blur-xl ring-1 ring-foreground/10`; items stay `rounded-md`; a switch row keeps the menu open and draws its `Switch` `pointer-events-none` |
| Dialog | `bg-background`, `rounded-lg`, `max-w-lg`, `p-6` | `bg-popover backdrop-blur-xl` (it floats), `rounded-xl`, `max-w-100`, `p-5`, close cross `rounded-sm`, title `text-ui font-medium` |
| Alert dialog | same frame as Dialog | identical frame, no close cross; `AlertDialogAction` gained a `destructive` prop that applies a **solid** red fill for the one thing in the dialog that destroys |
| Alert | opaque fill | veil surface plus blur, same as every floating frame |
| Switch | `h-5 w-9` | `h-4 w-7`, thumb `size-3.5`; unchecked track `bg-muted-foreground/30` |
| Spinner | lucide `Loader2` | own SVG, concentric circle, `animate-spin` on the `svg` |

The middle column describes the classic shadcn style from memory and was not checked against the `radix-nova` registry; treat it as orientation. The right column is read from Dray's files and is authoritative. Everything else in those files is stock. Nothing was renamed, so `npx shadcn add <name>` still drops a compatible file next to them.

### 9.2 The porting rule for a new shadcn component

Run `npx shadcn@latest add <name>` (config in `components.json`: style `radix-nova`, base `neutral`, CSS variables on), then make these substitutions in order. Each is mechanical.

1. **Radius.** Frames (popover, dialog, sheet, card, select content, command palette) become `rounded-lg`; dialogs and cards `rounded-xl`; items inside a list `rounded-md`; tiny chips `rounded-sm`. Delete any `rounded-full` unless the control is a switch or segmented track.
2. **Surface.** A component that floats over the page (popover, select content, hover card, sheet, command, toast) takes `bg-popover text-popover-foreground backdrop-blur-xl border border-border` and `shadow-lg` or `ring-1 ring-foreground/10`. A component that sits in the page (card, alert inline, table) takes `bg-card`. Replace any `bg-background` on a floating frame.
3. **Colour literals.** Replace `bg-white`, `bg-black/…`, `text-gray-*`, `emerald-*`, `red-*`, `border-neutral-*` with tokens: `bg-muted`, `text-muted-foreground`, `text-accent-add`, `text-destructive`, `border-border`, `bg-veil-strong`, `bg-surface-well`. A component with zero literals is finished on this step.
4. **Focus.** Every focusable element uses `outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50`. Delete `focus-visible:outline-2`, `ring-offset-*`, `ring-2`.
5. **Sizes.** Control height `h-8`, small `h-7`, tiny `h-6`; horizontal padding `px-2.5`; list item `px-1.5 py-1 gap-1.5 text-sm`; frame padding `p-1` for menus, `p-5` for dialogs. Icons default `size-4`, `size-3.5` in sm, `size-3` in xs.
6. **Shadows.** Only a filled action button carries `shadow-(--shadow-button)`. A surface at the window edge carries `shadow-(--shadow-surface)`. Floating frames keep Tailwind `shadow-lg`. Nothing else gets a shadow; remove `shadow-sm` and `shadow-md` from stock code.
7. **Motion.** Keep the `tw-animate-css` classes stock ships: `data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95` plus `data-[side=*]:slide-in-from-*`. Add `motion-reduce:transition-none` on anything that translates.
8. **Text.** Chrome text is `text-ui`; body copy in a card is `text-chat`. Stock `text-sm` is fine inside menus and buttons. Never `title=` on an element; use `Tooltip` with `Kbd` for a shortcut and `aria-label` for the name.
9. **Cursor.** Interactive elements get `cursor-pointer` in the base class, plus `disabled:pointer-events-none disabled:opacity-50`.
10. **Check both modes** and one ported palette. The failure to hunt is a colour that vanished, not a colour that looks wrong.

Worked example, stock shadcn `Badge` to Dray:

```
stock:  inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium ... bg-primary text-primary-foreground
dray:   inline-flex items-center rounded-sm bg-muted px-1.5 py-0.5 text-xs font-medium text-muted-foreground
```

A badge is a qualifier; presence is the message, so it takes the muted pair and no border, exactly as `Kbd` and the "Fast" and "free" marks do.

## 10. Where this can be used

**Any React + Tailwind v4 web app, as is.** Sections 3.1 through 3.6 are plain CSS with no Tauri dependency: paste them into the global stylesheet, install `tailwindcss @tailwindcss/vite tw-animate-css @fontsource-variable/geist @fontsource-variable/geist-mono radix-ui class-variance-authority clsx tailwind-merge lucide-react`, copy `components.json` and `cn()`, and every recipe in section 5 works unchanged. Add the pre-paint script from 3.8 to `index.html` (or the `<head>` of a Next.js root layout) for flicker-free theme restore.

**Skip for a web app:** section 3.7 (glass layer; a browser tab has no desktop behind it), the `--titlebar-h`, `--traffic-lights-w` and `--window-controls-w` constants, the `data-vibrancy` line in the pre-paint script, and the `html, body, #root { h-full overflow-hidden }` rule if the page should scroll normally. Without 3.7 every surface is an opaque fill, which is exactly what fullscreen Dray draws today, so nothing else changes.

**Other frameworks.** The token CSS is framework-agnostic. Class strings port to Vue, Svelte or Solid verbatim through `shadcn-vue` or `shadcn-svelte`, which generate the same Radix-shaped primitives. Only `cva` usage and the `Slot` import are React-specific.

**Tailwind v3 projects.** Move the `@theme inline` block into `tailwind.config` `theme.extend` (`colors`, `borderRadius`, `fontSize`, `fontFamily`, `keyframes`, `animation`), keep the `:root` and `[data-mode]` blocks as they are, and replace v4-only syntax: `shadow-(--x)` becomes `shadow-[var(--x)]`, `ring-3` becomes `ring-[3px]`, `data-open:` becomes `data-[state=open]:`, `**:` becomes `[&_*]:`, `in-data-[…]` has no v3 equivalent and is dropped.

**What the recipes assume about the host.** Geist at 13 to 15px, oklch colour support (every evergreen browser since 2023), `color-mix()`, and `backdrop-filter`. Where `backdrop-filter` is unavailable the `--veil-float` surfaces read as slabs, so under 3.7 they should fall back to the opaque `--surface-card`.

## 11. Sidebar

The sidebar is one list of sessions under a strip of controls. It has no fill of its own (`--sidebar: transparent`), so it reads as part of the page, and every control in it is drawn as a session row would be. Copy the structure and the class strings in this order.

### 11.1 Frame and resize

```html
<aside class="relative flex shrink-0 flex-col border-r border-sidebar-border" style="width: 240px">
  <!-- resize handle, titlebar strip, action buttons, filter row, list, update row -->
</aside>
```

- Width is the reader's, stored in local storage (`ade.sidebarWidth`), initial and minimum `240`. The handle is a `role="separator"` div: `absolute inset-y-0 z-10 w-1 cursor-col-resize focus-visible:bg-ring focus-visible:outline-none -right-0.5`, `aria-valuemin`/`aria-valuemax`/`aria-valuenow` set, arrow keys step, Home and double-click reset to the initial width. **No hover fill**: the border beside it is the edge, and lighting a second one reads as the edge thickening.
- Under 820px the sidebar is a drawer: `fixed inset-0 z-30 bg-black/40` scrim behind it, and picking a row closes it.
- The column never unmounts when collapsed; it hides, so scroll position and open groups survive.

### 11.2 Titlebar strip

```html
<div class="flex h-(--titlebar-h) shrink-0 items-center px-2 justify-end" data-tauri-drag-region="deep">
  <SpaceSwitcher />          <!-- innermost -->
  <SettingsButton />
  <SidebarToggle />          <!-- always the outer edge -->
</div>
```

- `justify-end` clears macOS traffic lights. When the leading edge is free (fullscreen, or no traffic lights on the platform) it becomes `justify-start pl-2`, the toggle and settings move to the left, and the switcher takes `ml-auto` so it stays in the same corner.
- The two icon buttons: `Button variant="ghost" size="icon-sm" className="opacity-80 transition-opacity hover:opacity-100"`, glyph `size-4` (the panel toggle glyph is `size-4.5`). Each sits in a `Tooltip` opening `side="right"` whose content is the name plus `ShortcutKeys`. Chrome is held back at rest and comes to full strength under the cursor.
- The toggle is also drawn in the main column's header when the sidebar is collapsed, so it must never change which end of the strip it is on. Settings has no second home, so it is the one that moves.

### 11.3 Space switcher (a word with a dot track under it)

```html
<div role="button" tabindex="0"
     class="group/spaces relative flex cursor-pointer items-center px-1.5 select-none focus-visible:outline-none">
  <span class="grid max-w-28 text-ui">
    <!-- every entry in the same grid cell; all but the active one `invisible`, so the box is as wide as the longest name and never resizes -->
    <span class="col-start-1 row-start-1 truncate text-center text-muted-foreground transition-colors group-hover/spaces:text-foreground [invisible when not active]">All Spaces</span>
  </span>
  <div class="pointer-events-none absolute top-full left-1/2 -translate-x-1/2">
    <DotTrack class="opacity-0 transition-opacity duration-150 group-hover/spaces:opacity-100" />
  </div>
</div>
```

A click steps to the next entry and wraps. No tooltip: the control draws its answer in words. The dot track is out of flow (`absolute top-full`) so the word sits on the same baseline as the header text beside it.

### 11.4 Dot track (shared by the space switcher and the project filter)

```ts
const DOT = 4, DOT_GAP = 4, DOT_PITCH = 8;
const DOT_TRACK_W = DOT_PITCH * 5 - DOT_GAP;   // five slots, odd, so the active dot has a real middle
```

```html
<div class="h-1.5">   <!-- reserved height, so revealing the track never shifts what is under it -->
  <!-- only when count > 1 -->
  <div class="flex h-full items-center overflow-hidden" style="width: 36px; mask-image: linear-gradient(to right, transparent, black 30%, black 70%, transparent)">
    <div class="flex items-center transition-transform duration-200 ease-out"
         style="gap: 4px; transform: translateX(calc(18px - 2px - activeIndex * 8px))">
      <span class="size-1 shrink-0 rounded-full transition-colors bg-foreground/80" />        <!-- active -->
      <span class="size-1 shrink-0 rounded-full transition-colors bg-muted-foreground/30" /> <!-- others -->
    </div>
  </div>
</div>
```

The active dot is always in the middle and the row slides under it. Hugging the dots and centring the group was tried: with the group centred nothing moves, a switch becomes a colour change, and the control stops feeling like anything. The mask fades both ends so a dot leaving reads as sliding out rather than being cut off.

### 11.5 Action buttons (New Task, Issues, Search)

```html
<div class="flex flex-col gap-px px-2">
  <Button variant="ghost" size="sm"
          class="w-full justify-start px-1.5 text-ui text-sidebar-foreground/80 hover:bg-sidebar-accent/50 hover:text-sidebar-foreground/80 dark:hover:bg-sidebar-accent/50">
    <Plus /> New Task <ShortcutKeys ids={["session.new"]} class="ml-auto" />
  </Button>
  <!-- a button that can be "the current page" adds: -->
  <Button ... data-active class="... data-[active]:bg-sidebar-accent data-[active]:text-sidebar-accent-foreground">
    <CircleDot /> Issues <ShortcutKeys ids={["issues.open"]} class="ml-auto" />
  </Button>
</div>
```

- `px-1.5` rather than `size="sm"`'s `px-2.5`, so the glyph lands on the same 12px inset as the icon buttons above.
- `text-sidebar-foreground/80` and `--sidebar-accent` at half strength on hover: exactly what an unselected session row carries, so the strip is one kind of control. Never `ghost`'s own `hover:bg-muted hover:text-foreground` here. The `dark:hover:` is restated because the variant's `dark:hover:bg-muted/50` out-specifies an unprefixed `hover:` and `tailwind-merge` cannot fold them.
- The shortcut keycap sits at the row's right edge inside the button (`ml-auto`), the one place keycaps appear outside a tooltip.

**Search becomes the field in place.** The button is replaced on the same row at the same height:

```html
<div class="group flex h-7 w-full items-center gap-1 border border-transparent px-1.5 text-ui">
  <Search class="size-3.5 shrink-0" />
  <input autofocus placeholder="Search" aria-label="Search tasks"
         class="min-w-0 flex-1 bg-transparent outline-none placeholder:text-muted-foreground" />
  <Kbd class="hidden group-focus-within:inline-flex">Esc</Kbd>          <!-- only while the field holds text -->
  <ShortcutKeys ids={["search"]} class="group-focus-within:hidden" />
</div>
```

No fill, no border colour, no focus ring: it is a row of plain buttons and the field stays plain too. The transparent border is what keeps the glyph on the same pixel as the buttons around it, since every button carries one. Escape closes and clears; blur closes only when empty.

### 11.6 Filter row

```html
<div class="mt-4 flex items-start justify-between py-1 pr-2 pl-3">
  <div role="button" tabindex="0"
       class="group/projects -my-1 flex min-w-0 flex-1 cursor-pointer flex-col items-start py-1 pl-1 select-none focus-visible:outline-none">
    <div class="flex max-w-full flex-col items-center gap-1">
      <span class="flex items-center gap-1 text-ui text-muted-foreground transition-colors group-hover/projects:text-foreground">
        <span class="max-w-40 truncate">All Projects</span>
        <ChevronDown class="size-3 shrink-0 opacity-60" />   <!-- only from 5 projects, when a tap opens a menu instead of stepping -->
      </span>
      <DotTrack />
    </div>
  </div>
  <div class="flex items-center gap-0.5">
    <Button variant="ghost" size="icon-xs" aria-label="Show settled" class="text-muted-foreground hover:text-foreground">
      <CheckCheck />   <!-- swaps to <Undo2 /> when the settled list is up; the glyph names the destination -->
    </Button>
  </div>
</div>
```

Menu mode: `DropdownMenuContent align="start" className="min-w-52"` holding a `DropdownMenuRadioGroup` of `DropdownMenuRadioItem className="text-ui"` rows with `<span class="truncate">`. The band is wrapped in a `Tooltip delayDuration={400}` opening `side="bottom" align="start" className="px-1.5"` that shows only the two keycaps.

### 11.7 The list

```html
<div class="scrollbar-overlay flex min-h-0 flex-1 flex-col gap-px overflow-y-auto pb-3 pl-2 pr-0">
  <!-- empty -->
  <p class="px-2 py-6 text-ui text-muted-foreground">No tasks yet.</p>

  <!-- between runs, never before the first: -->
  <div aria-hidden class="shrink-0 h-4" />   <!-- new project, Pinned, or a named group -->
  <div aria-hidden class="shrink-0 h-3" />   <!-- same project, next state run -->

  <HeadingRow />
  <SessionRow />…
  <ShortcutHint />   <!-- last row, scrolls away with the list -->
</div>
```

- `pr-0`: the scrollbar gutter is the right spacing; rows balance the track with their own `pr-0.5`.
- `shrink-0` on every spacer is load-bearing. A bare height in a flex column collapses to nothing once the list overflows, which is exactly the long list the break exists for.
- Runs are separated by space alone. No run names itself: the rail marks already say the state.

### 11.8 Heading row

```html
<!-- label only (Pinned) -->
<div class="flex min-h-6 items-center truncate pr-0.5 pl-2 text-ui text-muted-foreground/70">Pinned</div>

<!-- project heading, which is also "new task in this project" -->
<button type="button" aria-label="New task in dray"
        class="group/heading flex min-h-6 w-full cursor-pointer items-center truncate pr-0.5 pl-2 text-left text-ui text-muted-foreground/70 transition-colors duration-150 hover:text-foreground/75">
  <span class="truncate">dray</span>
  <Plus class="ml-auto size-3.5 shrink-0 opacity-0 transition-opacity duration-150 group-hover/heading:opacity-100" />
</button>
```

Hover stops at `text-foreground/75`: at full strength the heading became the brightest text in the pane, louder than the rows under it. Word only, no icon, no count. A heading is drawn only when the list spans more than one project; under a project filter the filter label already names it.

### 11.9 Session row

The most important recipe in the sidebar. A `div` with `role="button"` rather than a `<button>`, because the row holds real buttons.

```html
<div role="button" tabindex="0"
     class="group relative flex min-h-7 w-full cursor-pointer items-center rounded-md pl-0 pr-0.5
            transition-[color,background-color,opacity]
            focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sidebar-ring
            [inactive] text-sidebar-foreground/80 hover:bg-sidebar-accent/50 data-[state=open]:bg-sidebar-accent/50
            [active]   bg-sidebar-accent text-sidebar-accent-foreground
            [faded]    opacity-50 hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100">

  <!-- 1. rail slot: always present, 8px wide, the mark inside comes and goes -->
  <span class="flex w-2 shrink-0 items-center self-stretch">
    <span role="img" aria-label="Unread" class="h-1.5 w-0.5 rounded-[1px] bg-accent-add" />
    <!-- waiting on the reader: bg-accent-command, and it wins over the green -->
  </span>

  <!-- 2. lineage rails (nested rows only), absolute, aria-hidden, colour --sidebar-border -->
  <!-- 3. indent slot (nested rows only) -->
  <span aria-hidden class="shrink-0" style="width: ownRail + 2px" />

  <!-- 4. PR glyph, takes no room when absent -->
  <span class="mr-1 flex shrink-0 items-center" title="Pull request #12 · open"><PrStateIcon strokeWidth={1.5} /></span>

  <!-- 5. title -->
  <span class="min-w-0 flex-1 truncate text-ui">Fix the flaky test</span>

  <!-- 6. meta slot: one fixed slot, timestamp and hover actions crossfade -->
  <div data-row-meta class="relative flex min-w-[4em] shrink-0 items-center justify-end self-stretch pl-2 text-ui">
    <span class="pointer-events-none absolute right-0 flex items-center whitespace-nowrap text-ui text-muted-foreground transition-opacity duration-150 group-hover:opacity-0 group-data-[state=open]:opacity-0">
      2h ago
      <!-- or, in this order of precedence: -->
      <!-- checks running: <CircleDashed class="mr-[3px] size-3.5 animate-spin text-accent-command [animation-duration:3s]" strokeWidth={1.5} /> -->
      <!-- working:        <Orb state="listening" size={20} /> -->
    </span>
    <div class="pointer-events-none relative flex items-center gap-0.5 opacity-0 transition-opacity duration-150 group-hover:pointer-events-auto group-hover:opacity-100 group-data-[state=open]:pointer-events-auto group-data-[state=open]:opacity-100">
      <RowAction label="Pin" />      <!-- <Pin />, active when pinned -->
      <RowAction label="Settle" />   <!-- <Check />, or <Undo2 /> "Unsettle" on a settled row -->
    </div>
  </div>
</div>
```

**RowAction** is `Button variant="ghost" size="icon-xs" aria-label aria-pressed className="cursor-pointer text-muted-foreground"` (`text-foreground` when active), inside a `Tooltip` opening `side="bottom"` with the label. It calls `stopPropagation` so the row does not select.

Rules this row encodes:

- **No vertical padding.** The 24px hover buttons are the tallest thing in it; `min-h-7` keeps the height when they are not rendered.
- **The rail slot is always there and the mark inside it is what appears.** A mark that reflowed the title would move text because an agent finished. `h-1.5 w-0.5 rounded-[1px]`: a short rail, not a dot. Green means finished and unread, yellow means stopped and waiting on you; yellow wins.
- **Working is shown on the right, in the timestamp's place.** One indicator per row keeps the right edge quiet. Precedence: CI running (dashed yellow spinner, 3s turn), then the orb, then the relative time.
- **Hover controls crossfade with the timestamp in one fixed slot** sized by the buttons and floored at `min-w-[4em]` for the date. `opacity-0` not `hidden` (the button base's `inline-flex` beats a `display` utility), and `pointer-events-none` on both layers unconditionally so a faded layer never swallows the cursor.
- **Selected is the full `--sidebar-accent`; hover is half of it.** An open context menu (`data-[state=open]`, set on the row by `ContextMenuTrigger asChild`) holds the hover state.
- **Faded rows** (older than today in the settled list) take `opacity-50` and come back to full on hover and focus. Opacity rides the same `transition-[…]` declaration as colour, or `cn` drops one of them.
- **Focus ring is `ring-2 ring-sidebar-ring`**, the one place the app uses a two pixel ring, because the row is dense and a 3px ring collides with its neighbours.

**Nesting geometry** (a spawned session under its parent):

```ts
const RAIL_X = 12;   // x of the top-level rail, px from the row's left edge
const STEP = 12;     // per level
const ELBOW = 10;
const ownRail = RAIL_X + (depth - 1) * STEP;
```

- Pass-through for each ancestor whose line is still open: `<span aria-hidden class="pointer-events-none absolute top-0 -bottom-px w-px bg-sidebar-border" style="left: RAIL_X + level * STEP">`.
- This row's own connector: vertical `absolute top-0 w-px bg-sidebar-border` at `left: ownRail`, `height: calc(100% + 1px)` if the parent's rail continues below, else `50%`; then the elbow `absolute h-px bg-sidebar-border` at `left: ownRail + 1; top: 50%; width: ELBOW - 5`.
- A row with children opens its own rail from its centre down: `absolute -bottom-px w-px bg-sidebar-border` at `left: RAIL_X + depth * STEP; top: 50%`.
- Every piece sits on its own pixel; two segments in one column stack to ~15% and read as a bright patch. Pieces that continue reach 1px past the row's bottom because rows sit in a `gap-px` column.

### 11.10 Row context menu

```html
<ContextMenuContent class="w-40">   <!-- fixed width, so the frame does not resize when the confirm step swaps in -->
  <ContextMenuSub>
    <ContextMenuSubTrigger class="text-ui" disabled={inProgress}><GitBranchPlus /> Fork</ContextMenuSubTrigger>
    <ContextMenuSubContent>
      <ContextMenuItem class="text-ui">Fork in new worktree <Kbd class="ml-auto">1</Kbd></ContextMenuItem>
      <ContextMenuItem class="text-ui">Fork here <Kbd class="ml-auto">2</Kbd></ContextMenuItem>
    </ContextMenuSubContent>
  </ContextMenuSub>
  <ContextMenuItem class="text-ui"><Circle /> Mark unread</ContextMenuItem>
  <ContextMenuItem class="text-ui"><Unlink /> Detach from parent</ContextMenuItem>
  <ContextMenuItem variant="destructive" class="text-ui"><Trash2 /> Delete</ContextMenuItem>
</ContextMenuContent>

<!-- Delete swaps the whole menu body for a confirm step, in place: -->
<p class="px-1.5 py-1 text-ui text-muted-foreground">Are you sure?</p>
<div class="mt-1 flex gap-1">
  <ContextMenuItem class="flex-1 justify-center text-ui">Cancel</ContextMenuItem>
  <ContextMenuItem variant="destructive" class="flex-1 justify-center bg-destructive/10 text-ui">Delete</ContextMenuItem>
</div>
```

The confirm step is menu items, not buttons, so either choice closes the menu. `preventDefault` on the Delete item's `onSelect` is what holds the menu open for the swap. Digits 1 and 2 pick the fork rows while the submenu is open. Every item takes `text-ui` so the menu tracks the interface font size.

### 11.11 Hint rows, update row, dev badge

```html
<!-- keyboard hint, drawn as the list's last row -->
<div class="flex min-h-7 items-center justify-between pr-0.5 pl-2 text-ui text-muted-foreground/60">
  Switch tasks
  <ShortcutKeys ids={["session.prev","session.next"]} class="[&_kbd]:bg-muted/40 [&_kbd]:text-muted-foreground/60" />
</div>

<!-- update offer, outside the scroll container, only when there is something to say -->
<div class="shrink-0 px-2 py-2">
  <Button variant="ghost" class="w-full justify-start px-1.5 text-ui aria-disabled:cursor-default aria-disabled:opacity-50">
    <Download class="size-4 shrink-0" /> Update to v0.9 <span class="ml-auto text-muted-foreground tabular-nums">42%</span>
  </Button>
</div>

<!-- dev build badge, bottom of the column -->
<div class="shrink-0 truncate px-3.5 pb-2 font-mono text-[10px] leading-none text-muted-foreground/60">Dev · feature-x</div>
```

A hint row fades its keycaps against their own token (`bg-muted/40`), never with `opacity`, since the cap's fill and the page move in opposite directions per mode. A hint that teaches a gesture the reader has already made once is removed for good.

### 11.12 Ordering rules

- Sessions group by project; each project's run splits again by state in this order: **Needs attention** (yellow), **Completed** (green), **Idle**. Mid-turn is not a run of its own; the orb already says it.
- A project with fewer than three rows is not split (`STATE_SPLIT_MIN = 3`).
- A nest (parent plus spawned children) moves as one unit and takes the strongest state anything in it holds.
- Pinned is a group above the projects, spans them, and carries a whole nest. The settled list draws no Pinned group and no state runs: it is a history.
- Rows inside a run are newest `modified` first; nesting is depth-first.
- ⌘⇧↑/↓ walks exactly the drawn order, so the sort function is shared between render and shortcut.

## 12. Settings dialog

### 12.1 Frame

```html
<DialogContent data-phone-sheet aria-describedby={undefined} class="max-w-176 gap-0">
  <SettingsTabs />
</DialogContent>
```

44rem wide (`max-w-176`), `gap-0` because the dialog holds one child. The standard dialog close cross stays at `absolute top-5 right-5`. On a phone `data-phone-sheet` makes it the whole screen. Open state is not persisted; the tab resets on close. Mounted in `App`, since ⌘, must open it with the sidebar collapsed.

### 12.2 Rail and panel

```html
<div class="flex gap-5">
  <!-- rail: the title heads it; there is no dialog header row -->
  <div class="flex shrink-0 flex-col gap-3 w-32">
    <DialogTitle class="px-2">Settings</DialogTitle>
    <div role="tablist" aria-label="Settings" aria-orientation="vertical" class="flex gap-0.5 flex-col">
      <TabButton role="tab" aria-selected active class="cursor-pointer text-left">Appearance</TabButton>
      <!-- Spaces, Accounts, Transcription, Integrations, Shortcuts, About -->
    </div>
  </div>

  <div class="relative flex min-h-0 min-w-0 flex-1 flex-col">
    <!-- header action strip: a tab may portal one control up here (Refresh glyph, back arrow) -->
    <div class="pointer-events-none absolute top-0 right-6 left-0 z-10 flex h-4 items-center justify-end gap-1 [&>*]:pointer-events-auto" />

    <div role="tabpanel" class="-mx-1 flex flex-col gap-7 overflow-y-auto px-1 [&::-webkit-scrollbar-track]:my-4 [&>*]:shrink-0 h-[32rem] max-h-[60vh]">
      <!-- the active tab's body; bodies are switched, not hidden -->
    </div>
  </div>
</div>
```

- `TabButton`: `rounded-md px-2 py-1 text-ui transition-colors`, active `bg-sidebar-accent text-sidebar-accent-foreground`, inactive `text-muted-foreground hover:text-foreground`. Roving `tabIndex` (0 on the active tab, -1 elsewhere), arrows move both axes.
- The panel is a **fixed** 512px capped at `60vh` and scrolls. A floor made the dialog jump in height between tabs. `[&>*]:shrink-0` because a column flex item shrinks toward its content before the container scrolls. The `-mx-1 px-1` pair keeps a focus ring at the panel's edge from being clipped by `overflow-y`.
- `min-w-0` on the panel, or one long command string widens the dialog.
- Header slot: `right-6` leaves the corner to the close cross; `h-4` is the cross's own height, so a taller button centres on its line. The strip takes no pointer events and its children take them back. A tab renders into it through `createPortal`, so the control's state stays in the tab component.
- Narrow (phone): outer `flex h-full min-h-0 flex-col gap-3`, rail `min-w-0`, tablist `-mx-1 overflow-x-auto px-1 [&>*]:shrink-0` horizontal, panel `min-h-0 flex-1`.

### 12.3 Section and row shapes

```html
<section class="flex flex-col gap-4">
  <h2 class="text-ui font-medium text-muted-foreground">Font size</h2>   <!-- optional; only the block that lacks its own label wants one -->
  <SettingRow />…
</section>
```

**SettingRow, inline** (a control no wider than a switch):

```html
<div class="flex flex-col gap-1">
  <div class="flex items-center justify-between gap-4">
    <label for={id} class="text-ui font-medium">Auto-hide sidebar</label>
    <div class="shrink-0"><Switch id={id} /></div>
  </div>
  <p class="text-ui text-muted-foreground">Why this is off by default, in one sentence.</p>
</div>
```

**SettingRow, stacked** (a control wider than a switch, or a radio group):

```html
<div class="flex flex-col gap-2.5">
  <div class="flex flex-col gap-1">
    <label id={id} class="text-ui font-medium">Theme</label>
    <p class="text-ui text-muted-foreground">…</p>
  </div>
  <div role="radiogroup" aria-labelledby={id}>…</div>
</div>
```

The description is not optional in spirit: it is where "why is this off by default" lives. A setting that cannot apply here is drawn disabled with the reason in that slot, never hidden. Sections sit `gap-7` apart in the panel, rows `gap-4` inside a section.

### 12.4 Theme swatches

```html
<div role="radiogroup" aria-labelledby={id} class="flex items-start gap-3">
  <button type="button" role="radio" aria-checked tabindex={checked ? 0 : -1}
          class="group flex cursor-pointer flex-col items-center gap-1.5 outline-none">
    <span data-theme="gruvbox" data-mode="dark"
          class="theme-swatch size-12 rounded-md border transition-colors
                 [checked]   border-transparent ring-2 ring-ring ring-offset-2 ring-offset-popover
                 [unchecked] border-border group-hover:border-muted-foreground/60 group-focus-visible:border-ring" />
    <span class="text-ui [checked] text-foreground [unchecked] text-muted-foreground">Gruvbox</span>
  </button>
</div>
```

A swatch carries its own `data-theme` and `data-mode` (a dark-only theme always passes `dark`), so the palette block paints it. `.theme-swatch` re-declares both backdrop stops and layers a `--surface-card` wedge over the page colour along a 135deg cut at 52%:

```css
.theme-swatch {
  --backdrop-top: var(--background);
  --backdrop-bottom: var(--background);
  background-image:
    linear-gradient(135deg, transparent 0 52%, var(--surface-card) 52% 100%),
    linear-gradient(to bottom, var(--backdrop-top), var(--backdrop-bottom));
}
```

The wedge is what tells two similar palettes apart, and it is the surface the reader is about to see under every card.

### 12.5 Mode pills (a radiogroup drawn as a segmented control)

```html
<div role="radiogroup" class="inline-flex w-fit gap-0.5 rounded-lg bg-muted p-0.5 [disabled] opacity-50">
  <button type="button" role="radio" aria-checked
          class="rounded-[calc(var(--radius)-4px)] px-3 py-1 text-ui transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none cursor-pointer
                 [checked]   bg-card text-foreground shadow-2xs
                 [unchecked] text-muted-foreground hover:text-foreground">Light</button>
  <!-- Dark, System -->
</div>
```

This is the **fill** kind of segmented control (the checked pill is filled). The **sliding thumb** kind (`Segmented`, section 5, and the agent switch in 13.5) is for exactly two or three fixed-width segments where the motion is the message. Pick fill for words of different lengths, thumb for equal icons.

### 12.6 Numeric field

```html
<label for={id} class="[inputClassName] flex h-7 w-fit cursor-text items-center gap-1 px-2.5 text-ui focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50">
  <input id={id} inputmode="numeric" class="w-[2.5ch] bg-transparent text-right outline-none" />
  <span class="text-muted-foreground select-none">px</span>
</label>
```

The label is the box, so the unit sits inside the frame and the focus ring wraps both. Arrow keys step; the value is clamped on blur, never on change (clamping a controlled input on change turns a typed `15` into `105`).

### 12.7 List rows inside a tab

The Spaces, Accounts, Transcription and Shortcuts tabs are lists. Their rows share one grammar:

```html
<!-- one-line item with actions -->
<div class="flex h-7 items-center justify-between gap-3">
  <span class="min-w-0 flex-1 truncate text-ui">Design</span>
  <div class="flex shrink-0 items-center gap-0.5">
    <Button variant="ghost" size="icon-xs" aria-label="Rename" class="text-muted-foreground hover:text-foreground"><Pencil /></Button>
    <Button variant="ghost" size="icon-xs" aria-label="Remove" class="text-muted-foreground hover:text-foreground"><Trash2 /></Button>
  </div>
</div>

<!-- two-line item -->
<div class="flex items-center justify-between gap-4">
  <div class="flex min-w-0 flex-col">
    <span class="truncate text-ui font-medium">dray</span>
    <span class="truncate text-ui text-muted-foreground">~/dray</span>
  </div>
  <div class="flex shrink-0 items-center gap-0.5">
    <!-- a pick drawn as a small ghost button with a chevron -->
    <Button variant="ghost" size="sm" class="max-w-40 shrink-0 gap-1 px-2 text-ui">
      <span class="truncate">No space</span><ChevronDown class="size-3 shrink-0 opacity-60" />
    </Button>
    <Button variant="ghost" size="icon-xs" class="text-muted-foreground hover:text-foreground"><Trash2 /></Button>
  </div>
</div>

<!-- destructive confirm, in the row, replacing its actions -->
<div class="flex shrink-0 items-center gap-1">
  <Button size="xs" variant="destructive">Remove</Button>
  <Button variant="ghost" size="icon-xs" aria-label="Cancel" class="text-muted-foreground hover:text-foreground"><X /></Button>
</div>
<!-- or, with words: -->
<div class="flex items-center gap-1">
  <span class="text-ui text-muted-foreground">Sign out?</span>
  <Button variant="ghost" size="sm">Cancel</Button>
  <Button variant="destructive" size="sm">Sign out</Button>
</div>

<!-- account row: dot, identity, action -->
<div class="flex items-center gap-3 py-1.5">
  <span aria-hidden class="size-1.5 shrink-0 rounded-full bg-accent-add" />   <!-- signed in; bg-accent-command unknown; bg-muted-foreground/40 signed out -->
  <div class="min-w-0 flex-1">
    <p class="truncate text-ui text-foreground">Claude Code</p>
    <p class="truncate text-ui text-muted-foreground">honey@… · Claude.ai · Max</p>
  </div>
  <Button variant="ghost" size="sm" aria-label="More"><MoreHorizontal class="size-3.5" /></Button>
</div>

<!-- shortcut row -->
<div class="flex min-h-7 items-center justify-between gap-4">
  <span class="text-ui">New task</span>
  <span class="flex items-center gap-0.5"><ShortcutKeys /> <Button variant="ghost" size="icon-xs"><RotateCcw /></Button></span>
</div>
<p class="text-ui text-destructive">Already used by Search.</p>   <!-- refusal under the row -->
<Button variant="outline" size="sm" class="self-start">Reset all</Button>

<!-- radio card (transcription model) -->
<div role="radiogroup" aria-label="Transcription model" class="flex flex-col gap-2">
  <div role="radio" aria-checked class="flex flex-col gap-2 rounded-lg border p-3 transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none [selected] border-ring bg-muted/40 [else] border-border [pickable] cursor-pointer hover:bg-muted/30">
    <div class="flex flex-col gap-0.5">
      <div class="flex items-center gap-2">
        <span class="text-ui font-medium">Parakeet</span>
        <span class="rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">Recommended</span>
        <Check class="ml-auto size-4 shrink-0" />
      </div>
      <p class="text-ui text-muted-foreground">One sentence.</p>
    </div>
    <div class="flex items-center justify-between gap-3">
      <!-- meters -->
      <span class="flex items-center gap-1.5"><span class="text-[10px] text-muted-foreground">Speed</span>
        <span role="meter" class="h-1 w-12 overflow-hidden rounded-full bg-foreground/20"><span class="block h-full rounded-full bg-accent-add dark:bg-foreground/90" style="width: 80%" /></span></span>
      <!-- actions on the meters' row, not the title's, so a confirm can widen without re-wrapping the prose -->
      <div class="flex shrink-0 items-center gap-1">…</div>
    </div>
    <!-- download progress -->
    <div class="h-1 overflow-hidden rounded-full bg-foreground/20"><div class="h-full bg-primary transition-[width] duration-300" /></div>
  </div>
</div>

<!-- sign-in form card -->
<div class="mt-6 flex flex-col gap-3 rounded-xl border border-border/60 p-3">
  <span class="flex items-center gap-2 text-ui font-medium"><AgentIcon class="size-4 text-muted-foreground" /> Claude Code</span>
  <label class="flex items-center gap-3">
    <span class="w-20 shrink-0 text-ui text-muted-foreground">Key</span>
    <input class="[inputClassName] min-w-0 flex-1" />
  </label>
  <div class="flex gap-3">
    <span class="w-20 shrink-0 pt-1.5 text-ui text-muted-foreground">Method</span>
    <div role="radiogroup" class="flex min-w-0 flex-1 flex-col gap-1">…</div>
  </div>
  <div class="flex items-center justify-end gap-2"><Button type="submit" variant="secondary" size="sm">Sign in</Button></div>
</div>

<!-- copyable command -->
<div class="flex flex-wrap items-center gap-1.5">
  <div class="flex h-6 max-w-full items-center gap-1 rounded-md border border-border pr-0.5 pl-2 dark:border-input">
    <code class="min-w-0 overflow-x-auto font-mono text-code whitespace-nowrap text-foreground">claude auth login</code>
    <button class="flex size-5 cursor-pointer items-center justify-center rounded-sm text-muted-foreground transition-colors outline-none hover:bg-sidebar-accent hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"><Copy class="size-3.5" /></button>
  </div>
  <Button variant="secondary" size="sm"><SquareTerminal class="size-3.5" /> Run in Terminal</Button>
</div>

<!-- status lines -->
<p class="text-ui text-muted-foreground">Reading transcription settings…</p>
<p class="text-ui text-destructive">Could not run gh.</p>
```

Rules across all of them: row height `h-7` or `min-h-7`; the label truncates and the actions never do (`shrink-0`); icon actions are `icon-xs` ghost in `text-muted-foreground hover:text-foreground`; a destructive act confirms **in the row** with `size="xs" variant="destructive"` plus a cancel glyph, never in a modal; one `confirming` id lives in the parent so only one row asks at a time; a picker inside a row is a small ghost button with `ChevronDown size-3 shrink-0 opacity-60`.

### 12.8 Header actions

A tab with a whole-tab action puts one glyph in the header strip through the portal: `Button variant="ghost" size="icon-sm" aria-label="Refresh"` holding `RefreshCw class="size-3.5"` (or a `Spinner class="size-3.5"` while busy), name in a tooltip. A form that takes the tab over leads the strip with a back arrow: the same button with `ArrowLeft` and `class="mr-auto"`. Never a word in that strip, and never a row spent on it inside the panel.

## 13. Composer and its pickers

### 13.1 Frame and the two states

```html
<div class="px-4 pb-4">
  <form class="mx-auto max-w-3xl">
    [new task only] <div aria-hidden class="mb-4 h-10 w-full max-w-30 bg-current text-foreground/10" style="mask: url(logo.svg) contain no-repeat" />
    [error]          <div class="mb-2 flex items-start gap-2 px-1 text-ui text-destructive">
                       <span class="min-w-0 flex-1 break-words whitespace-pre-wrap">{error}</span>
                       <button aria-label="Dismiss error" class="mt-px shrink-0 rounded p-0.5 opacity-70 transition-opacity hover:opacity-100"><X class="size-3.5" strokeWidth={2} /></button>
                     </div>
    [new task only]  <div class="-ml-2.5 pb-1.5">{toolbar}</div>
    [session only]   <HandoffRow />

    <div class="relative">               <!-- pickers anchor here, not on the card: a backdrop-filter element is a backdrop root -->
      <PickerMenu />                      <!-- @ / # & rows, above the card in a session, below it on a new task -->
      <div class="relative rounded-2xl transition-colors
                  [session] border border-edge-surface bg-composer shadow-(--shadow-surface) backdrop-blur-xl">
        [dragging] <div class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center gap-2 rounded-2xl border-2 border-muted-foreground/25 bg-background/70 text-ui text-muted-foreground"><Paperclip class="size-3.5" strokeWidth={2} /> Drop to attach</div>
        [attach error] <p class="pt-3 px-3 text-ui text-destructive">…</p>
        [attachments]  <div class="pt-3 px-3"><AttachmentTray /></div>
        <div class="flex items-end gap-1 py-3 px-3">          <!-- px-0 on a new task -->
          <div class="relative min-w-0 flex-1"><RichInput class="py-1 text-prompt px-1" /></div>   <!-- px-0 on a new task -->
          <div class="flex shrink-0 items-center gap-1">{dictate}{send}</div>
        </div>
      </div>
    </div>

    [new task]  <div class="flex items-center gap-1 pt-2 text-ui text-muted-foreground/60">Press <CornerDownLeft class="size-3" strokeWidth={2} /> to send</div>
    [session]   <div class="pt-1.5">{toolbar}</div>
  </form>
</div>
```

- **New task**: no card fill, border, shadow or padding; the toolbar sits above the text, pulled left by `-ml-2.5` so the `+` glyph lands on the text's edge; a hint replaces the Send button. Enter-to-send lives in `onKeyDown`, which is what lets the button go.
- **Session**: `bg-composer` (not `bg-card`) with `backdrop-blur-xl`, `border-edge-surface`, `shadow-(--shadow-surface)`. The composer is a floating surface at the window's edge.
- Controls ride the text's own row with `items-end`: beside one line of text, at the bottom of several. Never a second row that appears on wrap.
- Text caps at 10 rows (`max-h-(--composer-cap)`, `--composer-cap: 10lh`) in a session and 20 on a new task.

### 13.1a The two states side by side

Same form, same `max-w-3xl`, same controls. What moves is where the toolbar sits and whether the card has a surface.

```
NEW CHAT (no session yet), centred in the window, top padding 13vh
┌ form max-w-3xl ─────────────────────────────────────────────────────┐
│ ▒▒ wordmark  h-10 max-w-30 text-foreground/10          (mb-4)       │
│ [+] [✳ Fable 5.1 High] [📁 dray] [⊙ Worktree] [⑂ main] [Auto]  (toolbar, -ml-2.5 pb-1.5)
│ Describe a task. @files. /skills. &tasks. #issues.     (text, px-0, py-1, text-prompt, up to 20 rows)
│ Press ⏎ to send                                        (pt-2, text-ui text-muted-foreground/60)
└─────────────────────────────────────────────────────────────────────┘
No card: no fill, no border, no shadow, no padding. Pickers open BELOW the text and are `bare`.

IN CHAT (session exists), pinned to the bottom of the column, px-4 pb-4
                    [Commit] [Create PR] [Run server]   (handoff row, hidden behind the card, 4px reserve)
┌ card rounded-2xl border-edge-surface bg-composer shadow-(--shadow-surface) backdrop-blur-xl ┐
│ (attachments tray, pt-3 px-3, only when present)                                          │
│ ┌ flex items-end gap-1 py-3 px-3 ───────────────────────────────────────────────────────┐ │
│ │ Send follow-up                                                    [🎤] [⬆]           │ │
│ │ (RichInput px-1 py-1 text-prompt, up to 10 rows)          icon-sm  icon-sm filled     │ │
│ └───────────────────────────────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────────────────────────────┘
[+] [✳ Fable 5.1 High] [Auto]                                            (◔)   (toolbar, pt-1.5)
Pickers open ABOVE the card and are framed.
```

**Toolbar contents by state.** Both start with Attach (`+`) then the model switcher. A new chat adds, in order, project, worktree toggle, branch (or `from origin/main` when the worktree is on). Both end with the permission picker ("Auto"). In chat only, the context ring sits at the far right on `ml-auto`. Nothing about a project, branch or worktree is drawn once a session exists; those are creation-time picks.

**Dimensions, all from the shared vocabulary in 14.4, nothing bespoke:**

| Part | Value |
|---|---|
| Column padding around the form | `px-4 pb-4` |
| Form width | `max-w-3xl` (48rem), `mx-auto` |
| Card | `rounded-2xl` (16px), 1px `border-edge-surface`, inner row `py-3 px-3` (12px), attachments `pt-3 px-3` |
| Text | `text-prompt` (15px default), `py-1`, `px-1` in chat and `px-0` on a new chat, line cap `10lh` in chat, `20lh` new chat |
| Control row | `flex items-end gap-1`; both buttons `icon-sm` (28px), `rounded-full` |
| Send | filled `bg-primary text-primary-foreground hover:bg-primary/90`, disabled `bg-muted text-muted-foreground shadow-none opacity-100`; glyph `size-4`, `ArrowUp strokeWidth={2}` or `Square fill-current` |
| Dictate | `ghost icon-sm rounded-full text-muted-foreground`, `Mic` glyph; while transcribing the same filled circle holds a `Spinner`; a saved recording adds `FolderOpen` ghost and a filled `RotateCcw` retry beside it, all `h-7` |
| Toolbar row | `flex min-w-0 items-center gap-0.5 px-1`, `pt-1.5` under the card, or `-ml-2.5 pb-1.5` above the text on a new chat |
| Every toolbar trigger | `ghost sm`: `h-7 px-1.5 gap-1.5 text-ui text-muted-foreground`, glyph `size-3.5`, name `truncate` capped `max-w-40` (`max-w-[11rem]` on the model) |
| Attach | `ghost icon-sm text-muted-foreground`, `Plus` `size-4` |
| Permission trigger | same trigger, word only ("Auto", "Ask", "Plan"), no glyph |
| Context ring | 14px SVG (`size-3.5`) in a `px-1.5` box on `ml-auto`; track `strokeWidth 3 opacity .25`, arc from twelve o'clock, `text-muted-foreground`, `text-destructive` from 80% |
| Static fact beside pickers | `px-1.5 text-ui text-muted-foreground/60` |
| Hint under a new chat | `pt-2 text-ui text-muted-foreground/60`, `CornerDownLeft size-3 strokeWidth={2}` |
| Error above the toolbar | `mb-2 px-1 text-ui text-destructive`, dismiss `X size-3.5` at `opacity-70 hover:opacity-100` |
| Drop overlay | `absolute inset-0 rounded-2xl border-2 border-muted-foreground/25 bg-background/70 text-ui text-muted-foreground` |

### 13.2 RichInput (contenteditable that reads back as a string)

```html
<div role="textbox" aria-multiline contenteditable data-placeholder="Send follow-up"
     class="relative block w-full overflow-y-auto whitespace-pre-wrap break-words text-foreground outline-none max-h-(--composer-cap)
            before:pointer-events-none before:absolute before:text-muted-foreground before:content-[attr(data-placeholder)]
            [has text] before:content-none" style="--composer-cap: 10lh" />
```

Placeholder is a `::before` reading `data-placeholder`, so it is styled like every other placeholder. Prose tokens while being typed are coloured runs: `text-accent-command` for `/command`, `text-accent-mention` for `@file`, `text-accent-issue` for `#DRA-53`, `text-accent-session` for `&session`, a URL underlined only. A finished token the caret has left becomes a **chip**:

```ts
const CHIP_SHAPE = "inline cursor-default whitespace-nowrap rounded-sm px-1 align-baseline text-[0.94em] leading-tight";
const CHIP_ACCENT = {
  mention: "bg-accent-mention/12 text-accent-mention dark:bg-accent-mention dark:text-background",
  session: "bg-accent-session/12 text-accent-session dark:bg-accent-session dark:text-background",
  issue:   "bg-accent-issue/12   text-accent-issue   dark:bg-accent-issue   dark:text-background",
};
```

A chip is `contenteditable=false`, its face is short (filename, title) and `data-tag` holds the full run. Light mode is a 12% tint with coloured text; dark mode is a solid accent fill with page-coloured text. Chips carry no `title`. Slash commands stay a coloured run, never a chip.

### 13.3 Send and dictate

```html
<!-- Send / Stop: one button, the icons swap in place -->
<Button type="submit" size="icon-sm"
        class="rounded-full bg-primary text-primary-foreground hover:bg-primary/90 disabled:bg-muted disabled:text-muted-foreground disabled:shadow-none disabled:opacity-100">
  <span class="grid size-4 place-items-center">
    <ArrowUp strokeWidth={2} class="col-start-1 row-start-1 transition-all duration-200 ease-out motion-reduce:transition-none [stopping] scale-50 rotate-90 opacity-0 [else] scale-100 rotate-0 opacity-100" />
    <Square class="col-start-1 row-start-1 fill-current transition-all duration-200 ease-out motion-reduce:transition-none [stopping] scale-100 rotate-0 opacity-100 [else] scale-50 -rotate-90 opacity-0" />
  </span>
</Button>

<!-- Dictate: the one round ghost button -->
<Button variant="ghost" size="icon-sm" class="rounded-full text-muted-foreground"><Mic /></Button>
```

Send is the app's one `rounded-full` filled control and the one place `--primary` is a brand colour. Disabled keeps full opacity and swaps to `bg-muted text-muted-foreground`, so the control reads as "nothing to send" rather than as faded. Both icons stay mounted in one grid cell and crossfade over 200ms in both directions; a shorter return read as a flinch. While transcribing the stop button is redrawn with a `Spinner` in place of the tick, in the same filled circle.

### 13.4 Toolbar and the trigger anatomy

```html
<div data-composer-toolbar class="flex min-w-0 items-center gap-0.5 px-1">
  <Button variant="ghost" size="icon-sm" aria-label="Attach files" class="text-muted-foreground"><Plus /></Button>
  <ModelSelector />
  [new session] <ProjectSelector /> <WorktreeToggle /> <BranchSelector />   <!-- or "from origin/main" when a worktree is on -->
  <PermissionSelector />                                                       <!-- last of the pickers -->
  <div class="ml-auto"><ContextMeter /></div>
</div>
```

**Every picker trigger in the toolbar is the same button:**

```html
<Button variant="ghost" size="sm" class="px-1.5 text-ui text-muted-foreground [with icon] gap-1.5 [with long text] max-w-40">
  <Icon class="size-3.5 shrink-0" />
  <span class="truncate">Label</span>
</Button>
```

wrapped as `Tooltip > TooltipTrigger asChild > DropdownMenuTrigger asChild > Button`, tooltip `side="top" className="max-w-none whitespace-nowrap"` holding the verb ("Switch model") and its `ShortcutKeys`. Menus open `align="start"` at `min-w-44` (short lists) or `min-w-52` (names). Rows are `DropdownMenuRadioItem className="text-ui"` with `<span class="truncate">`. A dangerous option (`bypassPermissions`) is the same row in `text-destructive`. A menu that needs a heading uses `DropdownMenuLabel className="text-ui font-normal text-muted-foreground"`.

- `text-ui` overrides the button's `text-sm` so the toolbar follows the interface font size setting.
- The worktree toggle is a trigger-shaped `role="switch"` with a miniature track inside: `<span class="flex h-3 w-5 shrink-0 items-center rounded-full p-px transition-colors [on] bg-primary [off] bg-muted-foreground/30"><span class="size-2.5 rounded-full bg-background transition-transform [on] translate-x-2" /></span> Worktree`, label `aria-checked:text-foreground`.
- A static fact beside the pickers is `<span class="truncate px-1.5 text-ui text-muted-foreground/60">from origin/main</span>`.
- The context meter is a 14px ring (`size-3.5 text-muted-foreground`, `text-destructive` when tight) in `flex shrink-0 items-center px-1.5 focus:outline-none`, opening a menu whose label is the count.

### 13.5 Model selector

Trigger: the standard trigger with `min-w-0 gap-1`, an `AgentIcon class="size-3.5 shrink-0"`, the name in `<span class="max-w-[11rem] min-w-0 truncate">`, then qualifiers that never truncate: `<span class="shrink-0 text-muted-foreground/60">High</span>` for effort and `<span class="shrink-0 text-muted-foreground/60">Fast</span>` for fast mode. Qualifiers are muted words, never coloured.

```html
<DropdownMenuContent align="start" class="max-h-[min(60vh,var(--radix-dropdown-menu-content-available-height))] w-[min(280px,calc(100vw-1rem))]">

  <!-- agent switch: a well with a sliding thumb; plain buttons so the menu stays open -->
  <div class="mb-1 flex items-center gap-1 rounded-md bg-surface-well p-1">
    <div role="radiogroup" aria-label="Agent" class="relative flex items-center">
      <span aria-hidden class="absolute top-0 left-0 size-6 rounded-sm bg-surface-thumb shadow-(--shadow-button) transition-transform duration-150 ease-out" style="transform: translateX(activeIndex * 100%)" />
      <button type="button" role="radio" aria-checked
              class="relative flex size-6 items-center justify-center rounded-sm opacity-55 transition-opacity hover:opacity-100 aria-checked:opacity-100">
        <AgentIcon brand class="size-3.5" />
        [not installed] <span aria-hidden class="absolute -top-px -right-px size-1.5 rounded-full bg-destructive ring-1 ring-surface-well" />
      </button>
    </div>
    <ShortcutKeys ids={["harness.next"]} class="ml-auto pr-0.5" />
  </div>

  <!-- optional provider heading, with a separator between groups and none above the first -->
  <DropdownMenuSeparator />
  <p class="px-2 pt-1.5 pb-0.5 text-ui text-muted-foreground">anthropic</p>

  <!-- model row with an effort ladder: the row picks, the submenu refines -->
  <DropdownMenuSub>
    <DropdownMenuSubTrigger class="cursor-pointer gap-1 text-ui" trailingIcon={<Check class="ml-auto size-3.5" />}>
      <span class="min-w-0 truncate">Fable 5.1</span>
      <span class="shrink-0 text-muted-foreground/60">High</span>
    </DropdownMenuSubTrigger>
    <DropdownMenuSubContent> <!-- radio items: Low, Medium, High, Max, with the ⇧⇥ keycap on the current one --> </DropdownMenuSubContent>
  </DropdownMenuSub>
  <!-- a model with no ladder is a plain DropdownMenuItem class="cursor-pointer text-ui" with the Check ml-auto -->

  <!-- empty and waiting states are a sentence in a row's slot, never a DropdownMenuItem -->
  <p class="px-2 py-1.5 text-ui text-muted-foreground">Loading models…</p>

  <!-- a switch row: keeps the menu open, the Switch is only the picture -->
  <DropdownMenuItem role="switch" aria-checked class="cursor-pointer text-ui">
    Fast mode <Switch checked tabindex={-1} aria-hidden class="pointer-events-none ml-auto" />
  </DropdownMenuItem>
  <p class="px-2 pb-1.5 text-ui text-muted-foreground">Fast mode disabled · usage credits exhausted</p>   <!-- the harness's own sentence, only while the switch is on -->

  <DropdownMenuSub>
    <DropdownMenuSubTrigger class="text-ui text-muted-foreground">More models</DropdownMenuSubTrigger>
    <DropdownMenuSubContent>…</DropdownMenuSubContent>
  </DropdownMenuSub>

  <DropdownMenuItem class="cursor-pointer gap-2 text-ui text-muted-foreground"><Sliders class="size-3.5" /> Choose models…</DropdownMenuItem>

  <!-- foot: figures, right-aligned, tabular -->
  <div class="mt-1 border-t px-2 pt-1.5 pb-0.5 text-ui text-muted-foreground">
    <p class="pb-0.5">Plan usage</p>
    <p class="flex items-baseline justify-between gap-3"><span class="truncate">Current week (all models)</span><span class="text-foreground shrink-0 tabular-nums">42%</span></p>
    <p class="truncate text-muted-foreground/60">resets Sat 9am</p>
  </div>
</DropdownMenuContent>
```

**Exact dimensions of the switcher, measured off the primitives (defaults at 13px `text-ui`):**

| Part | Size and padding | Hover / open |
|---|---|---|
| Trigger | `Button variant="ghost" size="sm"`: `h-7` (28px), `px-1.5` (6px), `gap-1`, `rounded-[min(var(--radius-md),12px)]`, icon `size-3.5` | rest `text-muted-foreground`; hover `bg-muted/70 text-foreground` (`dark:bg-muted/50`); open (`aria-expanded`) `bg-muted text-foreground`, which is the filled pill in the screenshot |
| Menu | `w-[min(280px,calc(100vw-1rem))]`, `p-1` (4px), `rounded-lg`, `bg-popover backdrop-blur-xl shadow-md ring-1 ring-foreground/10`, opens `align="start"` with `sideOffset` 4, `duration-100` zoom-in from the trigger corner | |
| Model row | `px-1.5 py-1` (6px by 4px, so ~26px tall at `text-ui`), `gap-1.5`, `rounded-md`, `text-ui` | keyboard or pointer focus `bg-accent text-accent-foreground`; a row with a submenu also keeps `data-open:bg-accent` while its submenu is up (the tan "Opus 5" row) |
| Row with submenu | same, plus `ChevronRight` `size-4` at `ml-auto`; the picked model swaps the chevron for `Check class="ml-auto size-3.5"` | |
| Radio row (effort ladder) | `py-1 pr-8 pl-1.5`; the check is an absolute `span` at `right-2`, `CheckIcon size-4` | same `focus:bg-accent` |
| Submenu | `min-w-[96px] p-1 rounded-lg shadow-lg`, same ring and blur, opens to the right with `slide-in-from-left-2` | |
| Separator | `-mx-1 my-1 h-px bg-border` (bleeds to the frame edge) | |
| Heading in menu | `px-1.5 py-1 text-xs font-medium text-muted-foreground` for the primitive's label; Dray's own provider and foot headings use `px-2 pt-1.5 pb-0.5 text-ui text-muted-foreground` | |
| Agent well | `p-1 gap-1 rounded-md bg-surface-well`, marks `size-6 rounded-sm`, thumb `size-6 rounded-sm` sliding 150ms ease-out | mark `opacity-55` to `100` |
| Foot ("Used this session") | `mt-1 border-t px-2 pt-1.5 pb-0.5`, rows `flex items-baseline justify-between gap-3`, figures `shrink-0 tabular-nums`, total row `text-foreground` | none, it is not interactive |
| Keycaps in a row | `Kbd`: `h-5 min-w-5 rounded-sm bg-muted px-1 text-xs`, grouped `gap-1`, at `ml-auto` | |

Hover is **focus** on a Radix menu item: the pointer moves focus, so `focus:bg-accent` is the hover fill and there is no separate `hover:` class. `--accent` aliases `--muted`, a black or white veil on glass. Nothing in a menu carries a shadow of its own except the thumb.

Rules: the agent marks are dimmed by **opacity** (`opacity-55` to `100`), never by colour, because brand marks carry their own colour. The thumb is `--surface-thumb` with `--shadow-button`, coming up past the menu surface out of the well the track cuts. Dividers only between provider groups, never above the first or above "More models". The Fast row is a real switch, not a tick. Every numeric column is `shrink-0 tabular-nums`; the label beside it truncates. On a device that cannot hover a model row toggles its submenu on click instead of opening on hover.

### 13.6 Attachment tray and handoff row

```html
<ul class="flex flex-wrap gap-2">
  <li class="group relative">
    <img class="size-14 rounded-lg border border-hairline-strong bg-card object-cover" />
    <!-- or a file: -->
    <div class="flex h-14 max-w-56 items-center gap-2 rounded-lg border border-hairline-strong bg-card px-2.5">
      <FileIcon class="size-5" /><div class="flex min-w-0 flex-col"><span class="truncate text-ui">notes.md</span></div>
    </div>
    <button aria-label="Remove notes.md" class="absolute -top-1.5 -right-1.5 rounded-full border border-border bg-secondary p-0.5 text-secondary-foreground opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none"><X class="size-3" strokeWidth={2.5} /></button>
  </li>
  <li class="w-full text-ui text-muted-foreground">This model does not take images; they will be sent as file mentions.</li>
</ul>

<!-- handoff row: hidden behind the card, a 4px reserve that grows to 32px on hover -->
<div class="relative h-1 px-3">
  <div class="group absolute -top-7 left-3 flex h-8 w-fit flex-col justify-end">
    <div class="overflow-hidden transition-[height] delay-150 duration-200 ease-out h-1 group-hover:h-8 group-focus-within:h-8">
      <div class="flex gap-1">
        <Button size="sm" variant="secondary" class="pointer-events-none group-hover:pointer-events-auto group-focus-within:pointer-events-auto"><GitCommit class="size-3.5" strokeWidth={1.5} /> Commit</Button>
        <!-- Create PR, Run server -->
      </div>
    </div>
  </div>
</div>
```

`w-fit` keeps the hover zone off the rest of the transcript's bottom edge. `justify-end` pins the row to the card's top so the height it gains is gained upward. `delay-150` stops a cursor crossing to the composer from flashing it.

### 13.7 Picker menu (`/`, `@`, `#`, `&`)

One component, positioned against the wrapper around the card, never focused (the editor keeps focus so typing keeps filtering; arrows, Enter and Tab are handled by the editor's `onKeyDown`).

```html
<div class="absolute left-0 z-50 w-full [above] bottom-full mb-1.5 [below] top-full mt-1.5 [bare] -mb-1.5 / -mt-1.5">
  {hint}   <!-- above the frame when the list opens upward, below it otherwise; dropped while the list is empty -->
  <div class="overflow-hidden [framed] rounded-xl border border-hairline-strong bg-picker backdrop-blur-lg text-popover-foreground shadow-md [bare] text-foreground">
    <div class="p-1 pb-0">{header}</div>   <!-- e.g. the tracker Segmented; no rule under it -->
    <div role="listbox" class="scrollbar-none overflow-x-hidden overflow-y-auto overscroll-contain [framed] max-h-[14.5rem] p-1 [bare] max-h-[14rem]">
      <!-- empty -->
      <div class="flex h-8 items-center px-2 text-ui text-muted-foreground">No matching files</div>
      <!-- loading: three skeleton rows -->
      <div class="flex h-8 items-center gap-2 px-2">
        <span class="size-3.5 shrink-0 animate-pulse rounded-full bg-muted-foreground/20" />
        <span class="h-3 w-14 shrink-0 animate-pulse rounded bg-muted-foreground/20" />
        <span class="h-3 w-32 animate-pulse rounded bg-muted-foreground/10" />
      </div>
      <!-- group -->
      <div class="[not first] mt-2 [previous group labelled] border-t border-dotted border-border/40 pt-2">
        <div class="px-2 pb-0.5 text-ui text-muted-foreground/50">Plugins</div>
        <button type="button" role="option" aria-selected
                class="flex h-8 w-full cursor-pointer items-center gap-2 rounded-lg px-2 text-left text-ui
                       [active framed] bg-accent text-accent-foreground [active bare] bg-veil-strong text-accent-foreground [else] text-foreground">
          <span class="shrink-0 font-medium">/commit</span>
          <span class="shrink-0 text-muted-foreground/60">[message]</span>
          <span class="min-w-0 truncate text-muted-foreground">Write a commit</span>
        </button>
      </div>
    </div>
  </div>
</div>
```

- `bg-picker` is `--surface-card` opaque, and the heavier `--veil-panel` on glass; with `backdrop-blur-lg` (not `xl`, since this surface already spends the heavier wash).
- The frame and the scroller are separate elements: the radius on an `overflow-hidden` parent clips the scrollbar to the curve.
- `bare` (new task, where nothing is behind the list) drops fill, border, radius and shadow; the highlighted row is what marks the list.
- Rows are `h-8 rounded-lg` where menu items are `py-1 rounded-md`: this list is taller and lives over the transcript.
- The frame holds `role="status"` instead of `listbox` when it draws only a sentence.

## 14. Patterns to reuse on components not covered here

Every recipe above is an instance of a small set of rules. When you build something this document does not name, find the nearest shape below, take its class string, and change only the words.

### 14.1 Shapes

| You are building | Start from | The class string |
|---|---|---|
| A row in a list the reader picks from | Session row (11.9) | `group relative flex min-h-7 w-full cursor-pointer items-center rounded-md pr-0.5 text-ui`, inactive `text-sidebar-foreground/80 hover:bg-sidebar-accent/50`, active `bg-sidebar-accent text-sidebar-accent-foreground` |
| A row in a settings or detail list | 12.7 | `flex h-7 items-center justify-between gap-3`, label `min-w-0 flex-1 truncate text-ui`, actions `flex shrink-0 items-center gap-0.5` |
| A heading over a group of rows | 11.8 | `flex min-h-6 items-center truncate pr-0.5 pl-2 text-ui text-muted-foreground/70`; group labels inside a menu are `px-2 pt-1.5 pb-0.5 text-ui text-muted-foreground`; inside a picker `px-2 pb-0.5 text-ui text-muted-foreground/50` |
| A button in a strip of buttons | 11.5 | `Button variant="ghost" size="sm" className="w-full justify-start px-1.5 text-ui text-sidebar-foreground/80 hover:bg-sidebar-accent/50 hover:text-sidebar-foreground/80 dark:hover:bg-sidebar-accent/50"` |
| A control that opens a menu, in a toolbar | 13.4 | `Button variant="ghost" size="sm" className="px-1.5 text-ui text-muted-foreground"` + `Tooltip side="top" className="max-w-none whitespace-nowrap"` + `DropdownMenuContent align="start" className="min-w-44"` |
| A pick inside a row | 12.7 | `Button variant="ghost" size="sm" className="max-w-40 shrink-0 gap-1 px-2 text-ui"` + `ChevronDown className="size-3 shrink-0 opacity-60"` |
| An icon action on a row | RowAction (11.9) | `Button variant="ghost" size="icon-xs" aria-label className="text-muted-foreground hover:text-foreground"` in a `Tooltip side="bottom"` |
| An icon button in chrome (titlebar, header strip) | 11.2 | `Button variant="ghost" size="icon-sm" className="opacity-80 transition-opacity hover:opacity-100"`, glyph `size-4` |
| One-of-N, equal icons | Agent switch (13.5) | well `flex items-center gap-1 rounded-md bg-surface-well p-1`, thumb `absolute top-0 left-0 size-6 rounded-sm bg-surface-thumb shadow-(--shadow-button) transition-transform duration-150 ease-out`, marks `size-6 rounded-sm opacity-55 hover:opacity-100 aria-checked:opacity-100` |
| One-of-N, words | Mode pills (12.5) | track `inline-flex w-fit gap-0.5 rounded-lg bg-muted p-0.5`, pill `rounded-[calc(var(--radius)-4px)] px-3 py-1 text-ui`, checked `bg-card text-foreground shadow-2xs` |
| Exactly two, sliding | `Segmented` (5) | pill track `rounded-full bg-surface-well p-0.5`, thumb `bg-surface-thumb shadow-(--shadow-button)` |
| A tab in a tab row | `TabButton` (5) | `rounded-md px-2 py-1 text-ui transition-colors`, active `bg-sidebar-accent text-sidebar-accent-foreground` |
| A card sitting in the page | Sign-in form (12.7), radio card | `rounded-xl border border-border/60 p-3` or `rounded-lg border border-border p-3`, fill `bg-card` only where it must separate from the page |
| A card floating over the page | Menu, dialog (5), picker (13.7) | `bg-popover backdrop-blur-xl shadow-lg ring-1 ring-foreground/10` and the animate-in classes |
| A chip or tag | RichInput chip (13.2), pill tag (12.7) | `rounded-sm px-1 text-[0.94em]` with a `/12` tint and coloured text (light) or solid accent and `text-background` (dark); a neutral tag is `rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground` |
| A qualifier beside a name | "Fast", "Free", effort | `<span class="shrink-0 text-muted-foreground/60">Word</span>` |
| A status dot | Account row (12.7), missing agent (13.5) | `size-1.5 shrink-0 rounded-full` in `bg-accent-add` / `bg-accent-command` / `bg-muted-foreground/40`; a corner dot is `absolute -top-px -right-px size-1.5 rounded-full bg-destructive ring-1 ring-[the surface behind it]` |
| A meter or progress bar | 12.7 | `h-1 overflow-hidden rounded-full bg-foreground/20`, fill `h-full rounded-full bg-primary transition-[width] duration-300` (or `bg-accent-add dark:bg-foreground/90` for a score) |
| A keyboard hint that is the row | Hint row (11.11) | `flex min-h-7 items-center justify-between pr-0.5 pl-2 text-ui text-muted-foreground/60`, keycaps `[&_kbd]:bg-muted/40 [&_kbd]:text-muted-foreground/60` |
| An empty state | List (11.7), picker (13.7), menu (13.5) | one sentence in the slot a row would take: `px-2 py-6 text-ui text-muted-foreground` in a list, `flex h-8 items-center px-2 text-ui text-muted-foreground` in a picker, `px-2 py-1.5 text-ui text-muted-foreground` in a menu |
| A loading state | Picker skeleton (13.7) | `animate-pulse rounded bg-muted-foreground/20` bars at row height; a spinner (`Spinner className="size-3.5"`) only where a glyph was |
| An error line | 12.7, 13.1 | `text-ui text-destructive` under the thing that failed, `whitespace-pre-wrap` if it quotes a tool |
| A numeric readout | Model selector foot (13.5) | `flex items-baseline justify-between gap-3`, label `truncate`, figure `shrink-0 tabular-nums` |
| A divider | 13.5, 13.7 | between groups only, never above the first: `DropdownMenuSeparator`, or `border-t border-dotted border-border/40 pt-2` in a picker, or `mt-1 border-t` before a foot |
| A working indicator | 15.1 | `<Orb state="listening" size={20} aria-label="Working" />`, in the slot the fact it replaces used to hold |
| A session identity mark | 15.2 | `<SessionAvatar sessionId name className="size-5" />` |
| A resize handle | 11.1 | `absolute inset-y-0 z-10 w-1 cursor-col-resize focus-visible:bg-ring focus-visible:outline-none`, no hover fill |

### 14.2 Rules that generated every shape above

1. **Text is `text-ui` in chrome, and the muted ladder has four rungs.** `text-foreground` for the thing the reader is working with, `text-muted-foreground` for labels and descriptions, `/70` for headings over rows, `/60` for hints, qualifiers and keycaps that are the row, `/50` for group labels inside a picker. Never a fifth rung and never a grey literal.
2. **Icon sizes follow the control.** `size-4` in a default or `icon-sm` button, `size-3.5` in `sm`, `icon-xs` and inline beside `text-ui`, `size-3` for a chevron or a glyph inside a hint. A chevron that opens a menu is always `size-3 shrink-0 opacity-60`.
3. **Hover on chrome is one step, not two.** Text goes `text-muted-foreground` to `text-foreground`; an icon button goes `opacity-80` to `100`; a row gets `--sidebar-accent/50`. Never both a fill change and a text change on a ghost control that sits among rows.
4. **Selected is full `--sidebar-accent`, hover is half.** One token feeds the session row, both tab rows and every list row in every pane. If a new list needs a selected state, it is this pair.
5. **The row is `min-h-7` and the heading is `min-h-6`.** A control inside a row is `icon-xs` (24px) so it never grows the row. Lists are `gap-px` columns; groups are separated by `h-3` or `h-4` spacers with `shrink-0`, never rules.
6. **Reveal on hover with opacity plus pointer-events, in a fixed slot.** `opacity-0 group-hover:opacity-100 pointer-events-none group-hover:pointer-events-auto`, `transition-opacity duration-150`. Never `hidden`, never a slot that appears, never a layout shift.
7. **A slot for a mark that comes and goes is always present; a mark for a standing fact takes no room when absent.** The unread rail keeps its 8px; the PR glyph is `mr-1 shrink-0` and gone when there is no PR.
8. **Confirm in place.** A destructive act asks inside the row or menu it came from: `size="xs" variant="destructive"` plus a cancel glyph, or two menu items under "Are you sure?". A modal is for a question that carries information the reader has not seen (a count of files that will be lost). One `confirming` id in the parent, so one question is open at a time.
9. **Disabled is drawn with a reason, not hidden.** A disabled control gets `aria-disabled` and keeps pointer events when a tooltip must explain it, or puts the sentence in the row's description slot.
10. **One moving part per control.** A segmented control has one thumb; a switcher has one dot track; a row has one indicator in its right slot. Ordering when several things want the slot: the thing that happened elsewhere (CI) beats the thing the reader started (working), which beats history (timestamp).
11. **Words stay one width, names give way.** In a trigger the name truncates (`min-w-0 truncate`, capped with `max-w-…`) and every qualifier keeps its width (`shrink-0`). In a menu the label truncates and the figure is `shrink-0 tabular-nums`.
12. **Menus and pickers hold their shape.** A fixed `w-40` on a menu that swaps a confirm step in; a sentence in the row slot for empty and loading; a skeleton at row height. Nothing collapses to zero and springs back.
13. **Keycaps live in tooltips or at a row's right edge.** In a strip button, `ShortcutKeys className="ml-auto"`; in a tooltip, after the verb; in a hint row, faded against their own token. Never inline in a label.
14. **Tooltips are one sentence, not a menu.** `side` matches where there is room (right of the sidebar strip, top of the composer, bottom of a row action); a sentence that would wrap takes `max-w-none whitespace-nowrap`. A control that draws its answer in words takes no tooltip.
15. **Brand art dims by opacity; monochrome glyphs dim by colour.** An agent or provider mark goes `opacity-55` to `100`; a lucide glyph goes muted to foreground.
16. **Roving focus for any radiogroup or tablist.** `tabIndex={checked ? 0 : -1}`, arrows move within the group, `focus-visible:ring-2 focus-visible:ring-ring` on a pill, `ring-2 ring-ring ring-offset-2 ring-offset-popover` on a swatch.
17. **A count is drawn only when it can be acted on.** A dot track with one entry draws nothing; a hint with one row draws nothing; a heading over one group draws nothing.
18. **State is stored per intent.** Widths and picks in local storage under an `ade.` key; open menus, confirming ids and drafts in memory, module-level where a remount would lose them.
19. **`text-ui` beats `text-sm` on every control that sits in chrome**, so the reader's interface font size moves it. Stock `text-sm` is only left inside primitives the reader does not resize.
20. **A glyph names the destination, not the current state.** The settled toggle shows `CheckCheck` when it goes to the settled list and `Undo2` when it comes back; Send shows an arrow and Stop a square, in one button.

### 14.4 Shared size vocabulary

Every number in sections 11 to 16 comes from this table. A new component picks from it; it does not introduce a value. If a size is missing here, add it here first and name where else it applies.

| Role | Value | Used by |
|---|---|---|
| Titlebar / drag row | `h-(--titlebar-h)` = 40px | sidebar strip, main header, right panel tab row |
| Control, default | `h-8` (32px), `px-2.5`, `gap-1.5`, `rounded-lg` | dialog buttons, `Input` |
| Control, small | `h-7` (28px), `px-2.5` (or `px-1.5` when it sits in a row of siblings), `gap-1`, `rounded-[min(var(--radius-md),12px)]` | every toolbar trigger, sidebar strip buttons, settings pick buttons, sign-in submit |
| Control, tiny | `h-6` (24px), `px-2`, `gap-1`, `text-xs` | in-row destructive confirm |
| Icon button, small | `size-7` (28px) | titlebar icons, composer attach, dictate, send, header refresh and back |
| Icon button, tiny | `size-6` (24px) | row actions, list-row actions, shortcut reset, agent and provider marks |
| List row | `min-h-7` (28px), `rounded-md`, `pr-0.5` | session rows, hint rows, settings list rows (`h-7`), shortcut rows |
| Heading over rows | `min-h-6` (24px), `pl-2 pr-0.5` | sidebar project heading, Pinned |
| Menu item | `px-1.5 py-1`, `gap-1.5`, `rounded-md` (about 26px at `text-ui`) | every dropdown, context and sub menu row |
| Picker row | `h-8` (32px), `px-2`, `gap-2`, `rounded-lg` | `/` `@` `#` `&` menus |
| Menu frame | `p-1`, `rounded-lg`, `min-w-32` default, `min-w-44` short lists, `min-w-52` names, `w-40` fixed when a confirm swaps in, `w-[min(280px,…)]` model menu, submenu `min-w-[96px]` | |
| Picker frame | `p-1`, `rounded-xl`, `max-h-[14.5rem]` | |
| Dialog | `p-5`, `rounded-xl`, `max-w-100` default, `max-w-176` settings | |
| Card in page | `p-3`, `rounded-lg` (radio card) or `rounded-xl` (form card) | |
| Composer card | `rounded-2xl`, inner `py-3 px-3` | |
| Chip | `rounded-sm px-1`, `text-[0.94em]` | prose tags; neutral tag `rounded-full px-1.5 py-0.5 text-[10px]` |
| Keycap | `h-5 min-w-5 px-1 rounded-sm text-xs` | |
| Glyph in default / icon-sm | `size-4` (16px); titlebar toggles `size-4.5` | |
| Glyph in sm / icon-xs / inline | `size-3.5` (14px) | toolbar triggers, row actions, check marks, context ring |
| Glyph, chevron or hint | `size-3` (12px) | menu chevrons (`opacity-60`), Enter hint, tray remove |
| Identity mark | `size-5` (20px) blob; orb `size={20}` | |
| Status dot | `size-1.5` (6px); corner dot `size-1.5` with `ring-1` | account rows, missing agent |
| Rail mark | `h-1.5 w-0.5 rounded-[1px]` in an always-present `w-2` slot | session row |
| Dot track | dot `size-1`, gap 4px, five slots (36px) | switchers |
| Gap between siblings | `gap-0.5` in a tab row or icon cluster, `gap-1` between controls, `gap-1.5` glyph to label, `gap-2` header items, `gap-3` label to control, `gap-4` rows in a section, `gap-7` sections | |
| Column padding | sidebar list `pl-2`, strip `px-2`, filter row `pl-3 pr-2`, header `px-3`, composer column `px-4` | |
| Run break | `h-3` same project, `h-4` new group, both `shrink-0` | sidebar |
| Transitions | `duration-150` reveal and colour, `duration-200 ease-out` movement (thumbs, dots, send icon), `duration-100` menu open, `delay-150` handoff reveal, 3s spin for CI | |
| Opacity ladder | chrome at rest `opacity-80`, brand mark at rest `opacity-55`, faded row `opacity-50`, hidden layer `opacity-0`; text uses the `/70 /60 /50` colour rungs instead | |

### 14.3 Procedure for a new component

1. Name what it is in one of the shape table's words: row, heading, strip button, trigger, pick-in-row, icon action, one-of-N, tab, card in page, card over page, chip, qualifier, dot, meter, hint, empty, loading, error, readout, divider.
2. Copy that row's class string verbatim. Change words, not utilities.
3. Take every size from 14.4 and the text rung from rule 1; do not invent a `/40`, a `size-[15px]` or a `h-[26px]`.
4. If it has a selected state, use rule 4. If it reveals on hover, use rule 6. If it can destroy, use rule 8.
5. Put its shortcut in a tooltip (rule 13), its name on `aria-label`, and nothing on `title` unless it truncates text.
6. Run the section 8 checklist in both modes and on Gruvbox plus one other palette.

## 15. Indicators and identity marks: the orb and the blob

Two third-party pieces draw motion and identity. Use them, never a spinner or an initial in a circle where one of these is the right answer.

### 15.1 The orb (`thinking-orbs`, wrapped in `Orb.tsx`)

```tsx
import { ThinkingOrb, type ThinkingOrbProps } from "thinking-orbs";
import { useTheme } from "@/hooks/useTheme";

export default function Orb({ style, ...props }: Omit<ThinkingOrbProps, "theme">) {
  const { resolvedMode } = useTheme();
  return <ThinkingOrb theme={resolvedMode} style={{ willChange: "transform", ...style }} {...props} />;
}
```

- **Always through the wrapper.** The library's `theme="auto"` looks for `data-theme="dark|light"` on the document, and this app stamps a palette name there, so `auto` reads nothing. The wrapper pins the resolved mode. `willChange: transform` is what keeps WebKit from repainting the row around it.
- **One size, `size={20}`**, the inline-with-text preset. Every orb in the app is 20px: sidebar row, working indicator, tool row, subagent row, crew strip, retry and compaction indicators. Do not scale it to a heading.
- **State names the kind of wait, and the table is closed:**

| `state` | Means | Where |
|---|---|---|
| `listening` | an agent is working on a turn | sidebar row (in the timestamp's slot), working indicator under the transcript, pending tool row, subagent row, crew strip |
| `composing` | the model is reasoning | thinking (`Reasoning`) row |
| `searching` | retrying after an API error | `ApiRetryIndicator` |
| `shaping` | context is being compacted | `CompactingIndicator` |
| `weaving` | background tasks are running | `BackgroundTasksIndicator` |

- An orb replaces a fact, it does not sit beside one. In the sidebar it takes the timestamp's place; in a tool row it takes the status glyph's place. One indicator per row.
- `aria-label` says what it means ("Working"); the orb itself is decoration.
- Where a glyph, not an orb, is wanted for a spinning wait (a check running on CI, a download), use lucide `CircleDashed` with `animate-spin [animation-duration:3s]` in `text-accent-command`, or the app's own `Spinner` (section 5). Never `Loader2`.

### 15.2 The blob (`blobatar` / `@blobatar/react`, wrapped in `SessionAvatar.tsx`)

```tsx
<Blobatar name={sessionId} aria-hidden onError={() => setFailed(true)} className={cn("size-5 shrink-0", className)} />
// fallback, only when the blob cannot draw at all:
<span aria-hidden className="flex size-5 shrink-0 items-center justify-center overflow-hidden rounded-full bg-muted text-[10px] uppercase text-muted-foreground">{name.slice(0, 1)}</span>
```

- **A blob marks a session, never a person.** A person (GitHub, Gravatar) gets `Avatar` with a real picture and an initial while it loads; a generated shape there would read as a face someone chose. A session has no picture, so the blob is the one place a generated mark says something true.
- **Seed on the session id, never the title.** Titles are rewritten by the app; a mark that changes on retitle is not a mark.
- **`size-5` (20px), the figure alone, on no plate.** The library's own near-white disc stays near-white in dark mode and became the brightest thing on the row. One rung larger than `Avatar`'s `size-4` because the figure only fills ~70% of its viewBox.
- Swapped, not layered: the blob is a `data:` URI built locally, so there is no load to cover; the initial is the answer for one that failed, not a resting state.
- Drawn on the user bubble (who sent this prompt, when several sessions relay into one), on crew strips, and wherever a session is named among other sessions. Not in the sidebar row, which the title already identifies.

### 15.3 Which one, and what else

| Need | Use |
|---|---|
| Something is happening right now, agent-driven | `Orb` (15.1), 20px, the state that names the wait |
| Something is happening right now, machine elsewhere or a transfer | `CircleDashed animate-spin` 3s, or the download bar (12.7) |
| A short wait inside a control that already has a glyph | `Spinner className="size-3.5"` in the glyph's slot |
| Which session this is | `SessionAvatar` (15.2), 20px |
| Which agent or provider this is | `AgentIcon` / `ProviderIcon` brand marks, `size-3.5` in a trigger, `size-4` in a heading, dimmed by opacity |
| Which file this is | `vscode-material-icons` through `FileIcon`, `size-5` in a tray tile, `size-4` in a row |
| Which person this is | `Avatar` with picture, initial while loading |
| Waiting on the reader, unread, merged, failed | the rail marks and status dots in 7 and 14.1, never an orb |

## 16. Main column header

The strip over the transcript. Same height as every other titlebar row, and it is the drag region for the main column.

```html
<header class="flex h-(--titlebar-h) shrink-0 items-center gap-2 overflow-hidden px-3 [no right panel, non-mac] pr-(--window-controls-w)"
        data-tauri-drag-region="deep">

  <!-- 1. only while the sidebar is collapsed: the sidebar's own toggle, moved here -->
  <div class="flex items-center pl-(--traffic-lights-w) [leading edge free] -ml-1">
    <SidebarToggle collapsed />                    <!-- ghost icon-sm opacity-80 hover:opacity-100, PanelLeft size-4.5 dimmed -->
  </div>

  <!-- 2. breadcrumb, takes the slack -->
  <div class="flex min-w-0 items-center gap-3 text-ui flex-1">
    <span class="flex min-w-0 items-center gap-1.5 overflow-hidden">
      <span class="shrink-0 text-muted-foreground">dray</span>                       <!-- project basename -->
      <span aria-hidden class="shrink-0 text-muted-foreground/50">/</span>
      <span class="truncate font-medium text-foreground">Add Dray sidebar and gruvbox themes</span>   <!-- session title -->
    </span>
    <button aria-label="Copy the working directory, /home/…"
            class="flex min-w-0 cursor-pointer items-center gap-1 rounded-md text-muted-foreground outline-none transition-colors select-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50">
      <GitBranch class="size-3.5 shrink-0" />        <!-- swaps to Check for 1.5s after a copy -->
      <span class="truncate">main</span>
    </button>
  </div>
  <!-- with no session: <span class="text-ui text-muted-foreground">New session</span>, or the page name ("Issues") -->

  <!-- 3. view tabs, right of the title -->
  <div class="flex min-w-0 items-center gap-0.5 overflow-hidden">
    <TabButton active class="bg-transparent px-1.5">Chat</TabButton>   <!-- Browser, Diff, Files -->
  </div>

  <!-- 4. panel toggle, outer edge -->
  <Button variant="ghost" size="icon-sm" aria-label="Toggle panel" class="shrink-0 transition-opacity opacity-80 hover:opacity-100">
    <PanelRight class="size-4.5" />
    <!-- when the panel is closed and has news: opacity-100 and the glyph swaps to a changes or PR mark -->
  </Button>
</header>
```

Rules:

- **Height is `--titlebar-h` (40px), the same row the sidebar strip and the right panel's tab row use.** All three are drag regions. Horizontal padding `px-3`, items `gap-2`.
- **Three text weights, nothing else.** Project `text-muted-foreground`, slash `text-muted-foreground/50`, title `font-medium text-foreground`. The breadcrumb is `text-ui` like all chrome. Only the title truncates; project and slash are `shrink-0`, and the box carries `min-w-0 overflow-hidden` so what shrinks clips instead of spilling over the branch.
- **The branch is a button that copies the cwd.** Glyph `size-3.5`, text muted going foreground on hover, no fill, focus ring only. Its tooltip says what the click does, never the path.
- **View tabs are `TabButton` with the fill removed** (`bg-transparent px-1.5`): the same control as the settings rail and the right panel's tabs, but this row sits over glass beside the title, so colour alone marks the active one. Each tab's tooltip (`side="bottom" className="px-1.5"`) carries only the keycaps (⌘⌥1 to ⌘⌥4), the name being on the button.
- **The two icon buttons at the ends are chrome**: `opacity-80 hover:opacity-100`, glyph `size-4.5`. The sidebar toggle appears here only while the sidebar is collapsed, at the leading edge, clearing `--traffic-lights-w` (78px) unless the edge is free. The panel toggle holds the trailing edge; when the panel is closed and has something to show, it drops the fade and swaps its glyph, since an indicator is content.
- **Under 820px** the header gets a leading `PanelLeft` button (`px-3 py-2 text-muted-foreground hover:text-foreground`) that opens the sidebar drawer, and the view tabs move to a scrolling sub-row: `flex shrink-0 items-center overflow-x-auto border-b px-2 py-1`.
