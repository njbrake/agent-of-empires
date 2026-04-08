# Web Dashboard Design System -- Agent of Empires

> Extends DESIGN.md. All brand colors, fonts, and principles from the main design system apply here. This document covers web-specific patterns.

## Product Context
- **What this is:** Browser-based dashboard for monitoring and controlling AI agent sessions
- **Classifier:** APP UI (workspace-driven, task-focused, data-dense)
- **Who it's for:** Developers managing parallel AI agents who want remote/mobile access
- **Competitors:** Conductor Build (native Mac app), Webmux (web-based tmux viewer)
- **Mood:** The TUI, but in a browser. Industrial warmth. Dense but breathable. Not a generic SaaS dashboard -- this is a terminal tool that happens to render in a browser.

## Design Principles

1. **Terminal is the hero.** The terminal pane dominates the viewport. Everything else exists to help you find and interact with the right terminal session.
2. **Density over chrome.** Show more sessions, less UI. Every pixel of border, padding, and decoration earns its space.
3. **Warm dark, not cold dark.** Use the navy surfaces from DESIGN.md (#0f172a, #1e293b), not GitHub's cold gray (#0d1117, #161b22). The current implementation uses cold grays -- this needs to change.
4. **Status at a glance.** Session state (running, waiting, idle, error) should be visible in peripheral vision. Color-coded dots, not labels.
5. **Mobile is monitoring.** On mobile, you mostly watch. The sidebar becomes a session picker, the terminal fills the screen. Don't cram desktop features into mobile.

## Layout

### Desktop (>1024px)

```
+------------------------------------------------------+
|  [logo] Agent of Empires    [status] 6 sessions   [?]|  <- 48px header
+--------+---------------------------------------------+
|        |  Session Title  ·  claude  ·  main  ● Run   |  <- 40px content header
| SESS.  |                                              |
| LIST   |                                              |
|        |  ┌─────────────────────────────────────────┐ |
| ● Huns |  │                                         │ |
| ◐ Goth |  │       [xterm.js terminal pane]          │ |
| ○ Celt |  │         fills remaining space           │ |
| ● Frnk |  │                                         │ |
|        |  │                                         │ |
|        |  └─────────────────────────────────────────┘ |
+--------+---------------------------------------------+
  280px                    flex-1
```

- **Sidebar:** Fixed 280px. Session list with status dots, tool, branch. Selected item has left accent border (brand-600 amber, not blue).
- **Content:** Flex-1. Content header (40px) + terminal (fills remaining).
- **Header:** 48px. Logo left, session count right. Minimal.

### Tablet (768-1024px)

Same layout but sidebar collapses to 220px. Terminal font size reduces to 13px.

### Mobile (<768px)

Two-state: session list OR terminal. Swipe or tap to switch. Back arrow in terminal header returns to list. No split view.

## Colors (Web-Specific Tokens)

Adapt the DESIGN.md palette for web rendering. The key change: replace the current cold GitHub-style grays with the warm navy surfaces.

```css
:root {
  /* Surfaces -- warm navy (from DESIGN.md) */
  --bg-primary: #0f172a;          /* surface-900 */
  --bg-elevated: #1e293b;         /* surface-800 */
  --bg-nav: #172033;              /* surface-850 */
  --bg-hover: #26324b;            /* elevated hover */
  --bg-active: #2d3a56;           /* active/selected */

  /* Borders */
  --border: #334155;              /* surface-700 */
  --border-subtle: #1e293b;       /* surface-800 */

  /* Text */
  --text-primary: #e2e8f0;        /* surface-200, NOT pure white */
  --text-secondary: #94a3b8;      /* slate-400 */
  --text-muted: #64748b;          /* slate-500 */

  /* Brand -- amber/copper */
  --brand: #d97706;               /* brand-600 */
  --brand-hover: #f59e0b;         /* brand-500 */
  --brand-subtle: rgba(217, 119, 6, 0.1);

  /* Accent -- teal */
  --accent: #0d9488;              /* accent-600 */
  --accent-hover: #14b8a6;        /* accent-500 */

  /* Semantic */
  --status-running: #22c55e;
  --status-waiting: #fbbf24;
  --status-idle: #64748b;
  --status-error: #ef4444;
  --status-starting: #f59e0b;
  --status-stopped: #475569;

  /* Terminal */
  --terminal-bg: #020617;         /* surface-950, deepest */
  --terminal-fg: #e2e8f0;
  --terminal-cursor: #d97706;     /* brand cursor, not blue */
}
```

## Typography

```css
/* Load from DESIGN.md font sources */
@import url('https://api.fontshare.com/v2/css?f[]=satoshi@400,500,600,700&display=swap');
@import url('https://fonts.googleapis.com/css2?family=DM+Sans:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap');

:root {
  --font-display: 'Satoshi', system-ui, sans-serif;
  --font-body: 'DM Sans', system-ui, sans-serif;
  --font-mono: 'JetBrains Mono', ui-monospace, monospace;
}
```

| Element | Font | Size | Weight |
|---------|------|------|--------|
| Header title | Satoshi | 14px | 600 |
| Session title (sidebar) | DM Sans | 13px | 500 |
| Session meta (tool, branch) | DM Sans | 11px | 400 |
| Content header title | DM Sans | 14px | 600 |
| Status labels | JetBrains Mono | 11px | 400 |
| Terminal | JetBrains Mono | 14px | 400 |
| Buttons | DM Sans | 12px | 500 |
| Section labels | JetBrains Mono | 11px | 600, uppercase, tracking-wider |

## Components

### Session Item (Sidebar)

```
┌─────────────────────────┐
│ ● Huns                  │  <- status dot + title (DM Sans 500)
│   claude · feature/auth │  <- tool + branch (11px, muted)
└─────────────────────────┘
```

- **Default:** transparent bg, --text-primary title, --text-muted meta
- **Hover:** --bg-hover background
- **Active:** --bg-active background, 2px left border in --brand
- **Status dot:** 6px circle, color from --status-* tokens
- **Padding:** 8px 12px
- **Gap between items:** 2px

### Action Buttons

- **Primary:** --brand bg, white text, 6px radius, 12px font
- **Danger:** transparent bg, --status-error border + text
- **Ghost:** transparent bg, --text-secondary text, hover: --bg-hover
- **Size:** 28px height, 12px horizontal padding
- **All buttons:** `cursor: pointer`, 150ms transition

### Terminal View

- **Background:** --terminal-bg (#020617) -- the darkest surface, creates depth
- **Font:** JetBrains Mono, 14px, line-height 1.3
- **Cursor:** --brand color (amber), blink enabled
- **Selection:** rgba(217, 119, 6, 0.2) -- amber tint, not blue
- **Scrollbar:** thin, --border color, transparent track
- **Content header above terminal:** --bg-nav bg, shows session title + tool + status

### Status Indicators

Consistent across sidebar, header, and any future views:

| Status | Color | Dot | Description |
|--------|-------|-----|-------------|
| Running | --status-running | ● | Filled circle, green |
| Waiting | --status-waiting | ◐ | Half-filled, amber |
| Idle | --status-idle | ○ | Open circle, gray |
| Error | --status-error | ✕ | X mark, red |
| Starting | --status-starting | ◌ | Dotted circle, amber |
| Stopped | --status-stopped | ■ | Filled square, dark gray |

### Empty States

When no session is selected:

```
+---------------------------------------------+
|                                              |
|          [terminal icon, 48px, muted]        |
|                                              |
|       Select a session to connect            |  <- DM Sans 16px, --text-muted
|    Click any session in the sidebar          |  <- 13px, --text-muted
|                                              |
+---------------------------------------------+
```

When no sessions exist:

```
+---------------------------------------------+
|                                              |
|          [rocket icon, 48px, --brand]        |
|                                              |
|           No sessions yet                    |
|    Create one:  aoe add /path/to/project     |  <- code style, JetBrains Mono
|                                              |
+---------------------------------------------+
```

## Spacing

Use the DESIGN.md 4px base unit. Key measurements:

| Element | Value |
|---------|-------|
| Header height | 48px |
| Content header height | 40px |
| Sidebar width (desktop) | 280px |
| Sidebar padding | 6px horizontal, 0 vertical |
| Session item padding | 8px 12px |
| Session item gap | 2px |
| Button height | 28px |
| Button padding | 0 12px |
| Section label padding | 12px 14px |

## Motion

Minimal. This is a workspace tool, not a marketing page.

- **Sidebar hover:** background-color 100ms ease
- **Session selection:** instant (no transition -- feels responsive)
- **Terminal connect:** no animation. The PTY stream starts immediately.
- **Status dot color change:** 300ms ease (smooth, not jarring)
- **Mobile view switch:** 200ms slide (list -> terminal)

## Anti-Patterns (What NOT to Do)

These are specific to the web dashboard -- not the marketing site:

1. **No cards.** Sessions are list items, not cards. The sidebar is a list, not a grid.
2. **No colored backgrounds on sections.** The terminal is the only colored area (deep navy). Everything else is --bg-primary or --bg-elevated.
3. **No rounded everything.** Border radius: 6px on buttons, 6px on session items. 0 on the sidebar, header, and terminal container. Radius hierarchy matters.
4. **No blue accents.** The current implementation uses blue (#58a6ff) for active states and links. Replace with --brand (amber) for selection and --accent (teal) for informational.
5. **No pure white text.** Use --text-primary (#e2e8f0) for body, not #ffffff.
6. **No decorative elements.** No icons in the header. No badges. No gradients. The density of real information IS the design.

## Future Components (Design Guidance)

As the web dashboard expands, these components will be needed. Design direction for each:

### Session Creation Form
- **Pattern:** Slide-over panel from the right (not a modal)
- **Fields:** Project path (with filesystem picker), agent selector, branch, sandbox toggle, extra args
- **Agent selector:** Grid of agent icons (small, 32px, monochrome). Selected = --brand border.
- **Not a wizard.** One screen, all fields visible. Advanced options collapsed by default.

### Diff Viewer
- **Pattern:** Replace the terminal pane with a diff view (toggle, not overlay)
- **Style:** GitHub-style unified diff. Green/red with muted backgrounds. JetBrains Mono.
- **Header:** File path + line counts. Navigation between files.

### Settings Panel
- **Pattern:** Full-page view (replaces sidebar + content). Back button returns to dashboard.
- **Sections:** Profile management, theme, sound, sandbox defaults.
- **Not nested.** Flat list of settings with section headers. No tabs within settings.

### Notification System
- **Pattern:** Small toast in bottom-right. Auto-dismiss after 5s.
- **Content:** "Session 'Huns' is waiting for input", "Session 'Goths' finished"
- **Color:** --bg-elevated with left border in status color.

## Implementation Notes

### Tailwind CSS Configuration

The current Tailwind setup uses default classes. Extend with the design tokens:

```ts
// tailwind.config.ts
export default {
  theme: {
    extend: {
      colors: {
        brand: { 400: '#fbbf24', 500: '#f59e0b', 600: '#d97706', 700: '#b45309' },
        accent: { 500: '#14b8a6', 600: '#0d9488', 700: '#0f766e' },
        surface: { 700: '#334155', 800: '#1e293b', 850: '#172033', 900: '#0f172a', 950: '#020617' },
      },
      fontFamily: {
        display: ['Satoshi', 'system-ui', 'sans-serif'],
        body: ['DM Sans', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'ui-monospace', 'monospace'],
      },
    },
  },
}
```

### xterm.js Theme

```ts
const terminalTheme = {
  background: '#020617',      // surface-950
  foreground: '#e2e8f0',      // surface-200
  cursor: '#d97706',          // brand-600
  cursorAccent: '#020617',
  selectionBackground: 'rgba(217, 119, 6, 0.2)',  // brand amber tint
  black: '#0f172a',
  red: '#ef4444',
  green: '#22c55e',
  yellow: '#fbbf24',
  blue: '#0d9488',            // teal, not blue
  magenta: '#a78bfa',
  cyan: '#14b8a6',
  white: '#e2e8f0',
  brightBlack: '#475569',
  brightRed: '#f87171',
  brightGreen: '#4ade80',
  brightYellow: '#fde68a',
  brightBlue: '#2dd4bf',
  brightMagenta: '#c4b5fd',
  brightCyan: '#5eead4',
  brightWhite: '#f8fafc',
};
```

### Font Loading

Add to `web/index.html` `<head>`:

```html
<link rel="preconnect" href="https://api.fontshare.com" crossorigin>
<link rel="preconnect" href="https://fonts.googleapis.com" crossorigin>
<link href="https://api.fontshare.com/v2/css?f[]=satoshi@400,500,600,700&display=swap" rel="stylesheet">
<link href="https://fonts.googleapis.com/css2?family=DM+Sans:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
```

## Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-08 | Warm navy surfaces replace cold GitHub grays | DESIGN.md establishes warm navy (#0f172a) as the surface color. The current web dashboard uses cold grays (#0d1117) from the MVP. Consistency with brand requires the warm palette. |
| 2026-04-08 | Amber selection/cursor replaces blue | Blue (#58a6ff) is generic. Amber (#d97706) is the brand color. Selected session, terminal cursor, and active states all use brand amber. |
| 2026-04-08 | Satoshi for header, DM Sans for UI, JetBrains Mono for terminal | Matches DESIGN.md font stack. Gives the web dashboard typographic personality instead of system fonts. |
| 2026-04-08 | No cards in sidebar | Sessions are list items. Cards add unnecessary borders and padding. The sidebar is a dense, scannable list -- like a file tree, not a deck of cards. |
| 2026-04-08 | Terminal gets the deepest surface (#020617) | Creates visual depth. Sidebar and header are mid-tone (#0f172a, #172033). Terminal is the darkest. This makes the terminal feel like a window INTO something. |
| 2026-04-08 | Mobile is monitor-first | On phones, you mostly check on agents. The UI should prioritize session status viewing and terminal reading. Creating sessions stays on desktop/CLI. |
| 2026-04-08 | Classified as APP UI, not marketing | Applied App UI rules: calm surfaces, dense typography, utility copy. Not the Landing Page rules from DESIGN.md. |
