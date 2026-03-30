# Design System -- Agent of Empires

## Product Context
- **What this is:** Terminal session manager for AI coding agents (Claude Code, Gemini CLI, OpenCode, Codex, Mistral Vibe, etc.)
- **Who it's for:** Developers who run multiple AI coding agents in parallel and want a single dashboard to manage them
- **Space/industry:** Developer tools, terminal utilities, AI coding infrastructure
- **Project type:** Open source CLI/TUI tool with a marketing/docs website (Astro + Tailwind)
- **Peers:** Warp, Zed, Ghostty, tmux ecosystem tools

## Aesthetic Direction
- **Direction:** Industrial Warmth
- **Decoration level:** Intentional -- subtle surface gradients, warm glow accents, no gratuitous ornamentation
- **Mood:** Terminal-native, engineer-made, warm. Like a well-made tool with visible craft. Not a glossy Linear clone, not brutalist either. The amber copper palette is the soul; everything reinforces it.
- **What we avoid:** Purple/violet gradients, centered-everything layouts, uniform 3-column grids with colored icon circles, generic stock hero sections, the "Linear style" dark-mode template that every dev tool ships

## Typography
- **Display/Hero:** Satoshi (900, 700, 600) -- geometric like Inter but with distinctive letterforms (double-storey "a", geometric "g", wider "e"). Feels engineered, which matches a Rust/terminal product. Loaded from fontshare.com.
- **Body:** DM Sans (400, 500, 600) -- clean, excellent legibility, great tabular numerals for docs and data. Not overused. Pairs well with Satoshi's geometry.
- **UI/Labels:** DM Sans (same as body) at 13-14px
- **Data/Tables:** DM Sans with tabular-nums feature, or JetBrains Mono for code-adjacent data
- **Code:** JetBrains Mono (400, 500) -- already in use, proven, excellent at small sizes
- **Monospace accents:** JetBrains Mono at 11-12px for section labels, version badges, metadata, terminal-style UI elements. This reinforces the terminal-native identity.
- **Loading:** Satoshi from `https://api.fontshare.com/v2/css?f[]=satoshi@400,500,600,700,900&display=swap`, DM Sans + JetBrains Mono from Google Fonts
- **Scale:** 11 / 12 / 13 / 14 / 16 / 18 / 20 / 24 / 32 / 48 / 56 / 64 / 80px
- **Why not Inter?** It's the default pick for every dev tool since 2020. Works fine, zero personality. Satoshi has the same geometric clarity with actual character.

## Color

### Approach: Refined Copper + Muted Teal
Most dev tools are cold (blue, purple, teal-only). AoE is warm. The amber/copper primary against slate/navy surfaces reads "professional terminal tool with a point of view." The muted teal accent ties directly to the agent nodes in the logo and creates complementary tension with the amber.

### Brand -- Amber/Copper
| Token     | Hex     | Usage |
|-----------|---------|-------|
| brand-50  | #fffbeb | Light backgrounds, hover states in light mode |
| brand-100 | #fef3c7 | Subtle brand tints |
| brand-200 | #fde68a | Light mode emphasis backgrounds |
| brand-300 | #fcd34d | Decorative, star ratings |
| brand-400 | #fbbf24 | Active states, selected items, gradient start |
| brand-500 | #f59e0b | Primary brand in dark contexts, inline code color |
| brand-600 | #d97706 | **Primary brand anchor.** CTAs, links, section labels |
| brand-700 | #b45309 | Button backgrounds, gradient end, dark-on-light brand |
| brand-800 | #92400e | Heavy emphasis, dark brand |
| brand-900 | #78350f | Brand on light surfaces |

### Accent -- Muted Teal
| Token      | Hex     | Usage |
|------------|---------|-------|
| accent-50  | #f0fdfa | Light teal backgrounds |
| accent-100 | #ccfbf1 | Subtle teal tints |
| accent-200 | #99f6e4 | Light mode teal emphasis |
| accent-300 | #5eead4 | Decorative teal |
| accent-400 | #2dd4bf | Bright teal accents |
| accent-500 | #14b8a6 | Teal in dark contexts |
| accent-600 | #0d9488 | **Primary accent.** Branch names, secondary links, agent node color, info states |
| accent-700 | #0f766e | Dark teal emphasis |
| accent-800 | #115e59 | Heavy teal |
| accent-900 | #134e4a | Teal on light surfaces |

### Surfaces -- Warm Navy
| Token       | Hex     | Usage |
|-------------|---------|-------|
| surface-50  | #f8fafc | Light mode background |
| surface-100 | #f1f5f9 | Light mode elevated surfaces |
| surface-200 | #e2e8f0 | Light mode borders, dividers |
| surface-700 | #334155 | Dark mode borders, dividers |
| surface-800 | #1e293b | Dark mode elevated surfaces, card backgrounds |
| surface-850 | #172033 | Dark mode slightly elevated (nav bar, terminal header) |
| surface-900 | #0f172a | Dark mode primary background |
| surface-950 | #020617 | Dark mode deepest background |

### Semantic
| Name    | Hex     | Usage |
|---------|---------|-------|
| Success | #22c55e | Running status, confirmation, session started |
| Warning | #f59e0b | Waiting for input, caution states (shares brand-500) |
| Error   | #ef4444 | Docker not running, session failed, destructive actions |
| Info    | #0d9488 | Active session count, informational (shares accent-600) |

### Dark Mode
Default. Deep navy surfaces (#020617 to #0f172a), white/light gray text, brand amber for emphasis.

### Light Mode
Inverted surfaces (#f8fafc to #ffffff), dark text (#0f172a to #334155), brand shifts to brand-700/800 for sufficient contrast on light backgrounds.

## Spacing
- **Base unit:** 4px
- **Density:** Comfortable
- **Scale:** 2xs(2) xs(4) sm(8) md(16) lg(24) xl(32) 2xl(48) 3xl(64)

## Layout
- **Approach:** Editorial with grid discipline -- left-aligned hero, asymmetric feature cards, generous whitespace between sections
- **Grid:** Content max-width 1200px. Features use asymmetric grids (e.g., 1.4fr + 1fr) rather than uniform columns.
- **Max content width:** 1200px (container), 720px (prose/docs)
- **Border radius:** sm:4px, md:8px, lg:12px, full:9999px (pills/badges)
- **Terminal-native accents:** Monospace section labels, version badges, and metadata throughout the site reinforce the terminal identity
- **Hero pattern:** Left-aligned title + subtitle + CTA, not centered-everything. Reads as authoritative and intentional.
- **Feature cards:** Asymmetric grid with one featured card spanning 2 rows alongside smaller supporting cards. Creates visual hierarchy instead of flat 3-column uniformity.

## Motion
- **Approach:** Minimal-functional
- **Easing:** enter(ease-out / cubic-bezier(0.16, 1, 0.3, 1)) exit(ease-in / cubic-bezier(0.7, 0, 0.84, 0)) move(ease-in-out / cubic-bezier(0.45, 0, 0.55, 1))
- **Duration:** micro(75ms) short(150ms) medium(300ms)
- **Scroll animations:** Subtle entrance (fade + 12px translate, 0.4s ease-out). No decorative motion. A terminal tool that's restrained in motion reads as confident.

## Logo
- **Concept:** Terminal window frame + hexagonal circuit network + crown + orbiting agent nodes. Communicates "commanding agents from a terminal."
- **Full mark:** Terminal frame with hex, crown, 5 agent nodes, circuit lines. Used for hero sections, social preview, README.
- **Tight mark:** Hex + crown + 3 agent nodes (no terminal frame, no circuit detail). Used for favicon, nav logo, small contexts (under 32px).
- **Colors:** Crown uses brand copper gradient (#d97706 to #92400e). Agent nodes use accent teal (#0d9488). Background hex fill uses surface-900 (#0f172a). Terminal frame uses surface-800 (#1e293b).
- **Refinement direction:** Simplify circuit lines and junction dots that become noise at small sizes. Make the crown more prominent within the hex.

## TUI (ratatui)

The TUI layout and information architecture are solid. These recommendations refine the visual treatment without changing functionality.

### Empire Theme (new default)

Replace the Phosphor theme as default. Phosphor's bright lime green on dark green reads as "hacker terminal." The Empire theme uses the design system palette and reads as "professional tool."

| Token             | Phosphor (current)          | Empire (proposed)                    |
|-------------------|-----------------------------|--------------------------------------|
| background        | RGB(16, 20, 18) green-gray  | RGB(15, 23, 42) warm navy `#0f172a`  |
| border            | RGB(45, 70, 55) dark green  | RGB(51, 65, 85) slate `#334155`      |
| terminal_border   | RGB(70, 130, 180) blue      | RGB(13, 148, 136) teal `#0d9488`     |
| selection         | RGB(30, 50, 40) dark green  | RGB(38, 50, 75) elevated `#26324b`   |
| session_selection | RGB(60, 60, 60) gray        | RGB(55, 65, 92) slate `#37415c`      |
| title             | RGB(57, 255, 20) lime green | RGB(251, 191, 36) amber `#fbbf24`    |
| text              | RGB(180, 255, 180) lt green | RGB(203, 213, 225) cool gray `#cbd5e1`|
| dimmed            | RGB(80, 120, 90) muted grn  | RGB(100, 116, 139) slate `#64748b`   |
| hint              | RGB(100, 160, 120) grn      | RGB(148, 163, 184) lt slate `#94a3b8`|
| running           | RGB(0, 255, 180) bright cyn | RGB(34, 197, 94) green `#22c55e`     |
| waiting           | RGB(255, 180, 60) orange    | RGB(251, 191, 36) amber `#fbbf24`    |
| idle              | RGB(60, 100, 70) muted grn  | RGB(100, 116, 139) slate `#64748b`   |
| error             | RGB(255, 100, 80) coral     | RGB(239, 68, 68) red `#ef4444`       |
| terminal_active   | RGB(130, 170, 255) blue     | RGB(13, 148, 136) teal `#0d9488`     |
| group             | RGB(100, 220, 160) grn      | RGB(203, 213, 225) cool gray `#cbd5e1`|
| search            | RGB(180, 255, 200) lt grn   | RGB(251, 191, 36) amber `#fbbf24`    |
| accent            | RGB(57, 255, 20) lime green | RGB(217, 119, 6) copper `#d97706`    |
| branch            | RGB(100, 160, 200) blue     | RGB(13, 148, 136) teal `#0d9488`     |
| sandbox           | RGB(200, 122, 255) purple   | RGB(148, 163, 184) lt slate `#94a3b8`|
| help_key          | RGB(255, 180, 60) orange    | RGB(217, 119, 6) copper `#d97706`    |
| diff_add          | RGB(0, 255, 180) bright cyn | RGB(34, 197, 94) green `#22c55e`     |
| diff_delete       | RGB(255, 100, 80) coral     | RGB(239, 68, 68) red `#ef4444`       |
| diff_modified     | RGB(255, 180, 60) orange    | RGB(251, 191, 36) amber `#fbbf24`    |
| diff_header       | RGB(100, 160, 200) blue     | RGB(13, 148, 136) teal `#0d9488`     |

Keep Phosphor, Tokyo Night Storm, Catppuccin Latte, and Dracula as options. Add Empire and make it the default.

### Rounded Borders

Switch all `Block` widgets from sharp corners (`┌┐└┘`) to rounded (`╭╮╰╯`):

```rust
Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)  // add this line
```

Apply to: list panel, preview panel, all dialogs, help overlay, settings panels. This is the single highest-impact visual change for modernizing the TUI. ~15 lines across the codebase.

### Inner Padding

Add 1 character of horizontal padding inside panels so content doesn't butt against borders:

```rust
Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .padding(Padding::horizontal(1))  // add this line
```

Apply to: list panel, preview panel. Not needed on dialogs (they already manage internal spacing). The `block.inner()` call automatically accounts for padding.

### Single Panel Seam

Eliminate the double-border where the list and preview panels meet. Currently both panels have `Borders::ALL`, creating a heavy `││` seam. Instead:

- List panel: `Borders::TOP | Borders::LEFT | Borders::BOTTOM` (drop right border)
- Preview panel: keep `Borders::ALL` (its left border becomes the shared separator)

This makes the two panels feel like one cohesive surface with a divider rather than two separate boxes.

### What to Leave Alone

- **Status icons** (●◐○✕■◌✗) -- clean, universally readable, not "hacky"
- **Information density** in the session list -- AoE is a dashboard, density is correct
- **Dialog structure** -- functional, well-proportioned, dialogs should be boxed
- **Status bar layout** -- the key/description/separator pattern is learnable and functional
- **The other 4 themes** -- keep them all as options for users who prefer them

## Decisions Log
| Date       | Decision | Rationale |
|------------|----------|-----------|
| 2026-03-22 | Initial design system created | Created by /design-consultation based on product context, competitive research (Warp, Zed, Railway, Ghostty, Cursor, Linear), and analysis of existing website |
| 2026-03-22 | Satoshi over Inter for display | Inter is the default for every dev tool since 2020. Satoshi has the same geometric clarity with distinctive letterforms that give AoE typographic personality. |
| 2026-03-22 | DM Sans for body over Inter | Clean, great tabular numerals, not overused. Pairs naturally with Satoshi's geometry. |
| 2026-03-22 | Teal accent replaces sky blue | Sky blue (#0ea5e9) was Tailwind's default. Teal (#0d9488) ties directly to the logo's agent nodes and creates better complementary tension with amber. |
| 2026-03-22 | Left-aligned editorial hero layout | Breaks from the centered-everything pattern that makes dev tool sites interchangeable. Reads as authoritative and intentional. |
| 2026-03-22 | Asymmetric feature grid | Uniform 3-column grids with colored icon circles are a generic pattern. Asymmetric cards with a featured item create visual hierarchy. |
| 2026-03-22 | Monospace section labels throughout | JetBrains Mono at 11px for labels, version badges, and metadata reinforces terminal-native identity without being heavy-handed. |
| 2026-03-22 | Empire theme as new TUI default | Phosphor's lime green reads as "hacker." Empire uses the design system's amber/copper/teal palette for a professional feel. Phosphor stays as an option. |
| 2026-03-22 | Rounded borders in TUI | Sharp box-drawing corners feel dated. Rounded corners (╭╮╰╯) are the single highest-impact modernization for a ratatui app. |
| 2026-03-22 | Inner padding in TUI panels | 1 char horizontal padding prevents content from touching borders. Gives breathing room without sacrificing density. |
| 2026-03-22 | Single panel seam | Double-border between list and preview panels looks heavy. One shared divider line reads as a cohesive surface. |
