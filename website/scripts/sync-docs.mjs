#!/usr/bin/env node
// Syncs docs/ → website/src/pages/ (guides and docs pages).
//
// Single source of truth: docs/ contains the canonical markdown.
// This script strips the # Title line, rewrites relative links for the
// website URL scheme, and prepends Astro frontmatter.
//
// Generated files are .gitignored; do NOT edit them by hand.

import { readFileSync, writeFileSync, mkdirSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..", "..");
const PAGES_DIR = join(__dirname, "..", "src", "pages");

// All pages to sync. "source" is relative to repo root, "dest" is relative
// to website/src/pages/. Layout path is computed from dest depth.
const PAGES = [
  // --- Guides (docs/guides/ → pages/guides/) ---
  {
    source: "docs/guides/diff-view.md",
    dest: "guides/diff-view.md",
    title: "Diff View",
    description:
      "Review git changes and edit files directly from the Agent of Empires TUI.",
  },
  {
    source: "docs/guides/repo-config.md",
    dest: "guides/repo-config.md",
    title: "Repository Configuration & Hooks",
    description:
      "Per-repo configuration and hooks for Agent of Empires sessions.",
  },
  {
    source: "docs/guides/sandbox.md",
    dest: "guides/sandbox.md",
    title: "Docker Sandbox: Quick Reference",
    description:
      "Run AI coding agents in isolated Docker containers with Agent of Empires.",
  },
  {
    source: "docs/guides/tmux-status-bar.md",
    dest: "guides/tmux-status-bar.md",
    title: "tmux Status Bar",
    description:
      "Configure the tmux status bar to display Agent of Empires session information.",
  },
  {
    source: "docs/guides/web-dashboard.md",
    dest: "guides/web-dashboard.md",
    title: "Web Dashboard (Experimental)",
    description:
      "Remote access to AI coding agent sessions from any browser with Agent of Empires.",
  },
  {
    source: "docs/guides/worktrees.md",
    dest: "guides/worktrees.md",
    title: "Worktrees Reference",
    description:
      "Git worktree commands and configuration reference for Agent of Empires.",
  },

  // --- Docs pages (docs/ → pages/docs/) ---
  {
    source: "docs/index.md",
    dest: "docs/index.md",
    title: "Agent of Empires",
    description:
      "Terminal session manager for AI coding agents on Linux and macOS, built on tmux and written in Rust.",
  },
  {
    source: "docs/installation.md",
    dest: "docs/installation.md",
    title: "Installation",
    description:
      "Install Agent of Empires on Linux or macOS via the install script, Homebrew, or from source.",
  },
  {
    source: "docs/quick-start.md",
    dest: "docs/quick-start.md",
    title: "Quick Start",
    description:
      "Get up and running with Agent of Empires in minutes. Create sessions, attach to agents, and use worktrees.",
  },
  {
    source: "docs/development.md",
    dest: "docs/development.md",
    title: "Development",
    description: "Build, run, and test Agent of Empires from source.",
  },
  {
    source: "docs/sounds.md",
    dest: "docs/sounds.md",
    title: "Sound Effects",
    description:
      "Configure audio feedback for agent state transitions in Agent of Empires.",
  },
  {
    source: "docs/guides/configuration.md",
    dest: "docs/guides/configuration.md",
    title: "Configuration Reference",
    description:
      "Complete configuration reference for Agent of Empires settings, profiles, and repo config.",
  },
  {
    source: "docs/cli/reference.md",
    dest: "docs/cli/reference.md",
    title: "CLI Reference",
    description:
      "Complete command-line reference for the aoe CLI tool.",
  },
];

// Every known docs path → website URL, used for link rewriting.
const URL_MAP = {
  // Docs pages
  "docs/index.md": "/docs/",
  "docs/installation.md": "/docs/installation/",
  "docs/quick-start.md": "/docs/quick-start/",
  "docs/sounds.md": "/docs/sounds/",
  "docs/development.md": "/docs/development/",
  "docs/guides/configuration.md": "/docs/guides/configuration/",
  "docs/cli/reference.md": "/docs/cli/reference/",
  // Guides
  "docs/guides/diff-view.md": "/guides/diff-view/",
  "docs/guides/repo-config.md": "/guides/repo-config/",
  "docs/guides/sandbox.md": "/guides/sandbox/",
  "docs/guides/tmux-status-bar.md": "/guides/tmux-status-bar/",
  "docs/guides/web-dashboard.md": "/guides/web-dashboard/",
  "docs/guides/worktrees.md": "/guides/worktrees/",
};

const GITHUB_BASE =
  "https://github.com/njbrake/agent-of-empires/blob/main/";

function rewriteLinks(content, sourceDir) {
  // Rewrite markdown links to .md files: [text](target.md) or [text](target.md#anchor)
  content = content.replace(
    /\]\(([^)]+\.md(?:#[^)]*)?)\)/g,
    (_match, link) => {
      if (link.startsWith("http://") || link.startsWith("https://")) {
        return `](${link})`;
      }
      const hashIdx = link.indexOf("#");
      const targetFile = hashIdx >= 0 ? link.slice(0, hashIdx) : link;
      const anchor = hashIdx >= 0 ? link.slice(hashIdx) : "";
      const resolved = join(sourceDir, targetFile)
        .replace(/\\/g, "/")
        .replace(/^\.\//, "");
      const websiteUrl = URL_MAP[resolved];
      if (websiteUrl) {
        return `](${websiteUrl}${anchor})`;
      }
      return `](${GITHUB_BASE}${resolved}${anchor})`;
    }
  );

  // Rewrite HTML href links to .md or .html files (e.g., <a href="installation.html">)
  content = content.replace(
    /href="([^"]+\.(?:md|html)(?:#[^"]*)?)"/g,
    (_match, link) => {
      if (link.startsWith("http://") || link.startsWith("https://")) {
        return `href="${link}"`;
      }
      const hashIdx = link.indexOf("#");
      const targetFile = hashIdx >= 0 ? link.slice(0, hashIdx) : link;
      const anchor = hashIdx >= 0 ? link.slice(hashIdx) : "";
      // Normalize .html to .md for lookup
      const targetMd = targetFile.replace(/\.html$/, ".md");
      const resolved = join(sourceDir, targetMd)
        .replace(/\\/g, "/")
        .replace(/^\.\//, "");
      const websiteUrl = URL_MAP[resolved];
      if (websiteUrl) {
        return `href="${websiteUrl}${anchor}"`;
      }
      return _match;
    }
  );

  // Rewrite relative image/asset paths to absolute (assets/ → /assets/)
  // The build copies docs/assets/* to website/public/assets/
  content = content.replace(/\]\(assets\//g, "](/assets/");

  return content;
}

function computeLayoutPath(dest) {
  // Layout is at website/src/layouts/Docs.astro.
  // A page at website/src/pages/guides/foo.md needs ../../layouts/Docs.astro
  // A page at website/src/pages/docs/cli/ref.md needs ../../../layouts/Docs.astro
  const segments = dirname(dest).split("/").filter((s) => s !== ".");
  const depth = segments.length + 1; // +1 to go from pages/ up to src/
  return "../".repeat(depth) + "layouts/Docs.astro";
}

function escapeYaml(str) {
  if (/[:"']/.test(str)) {
    return `"${str.replace(/"/g, '\\"')}"`;
  }
  return str;
}

console.log("Syncing docs to website...");

for (const page of PAGES) {
  const sourcePath = join(ROOT, page.source);
  let content = readFileSync(sourcePath, "utf8");

  // Strip the leading # Title line (first non-empty line starting with #)
  content = content.replace(/^# .+\n\n?/, "");

  // Rewrite links
  const sourceDir = dirname(page.source);
  content = rewriteLinks(content, sourceDir);

  // Prepend Astro frontmatter
  const layout = computeLayoutPath(page.dest);
  const frontmatter = [
    "---",
    `layout: ${layout}`,
    `title: ${escapeYaml(page.title)}`,
    `description: ${escapeYaml(page.description)}`,
    "---",
    "",
    "",
  ].join("\n");

  const destPath = join(PAGES_DIR, page.dest);
  mkdirSync(dirname(destPath), { recursive: true });
  writeFileSync(destPath, frontmatter + content);

  console.log(`  ${page.source} -> ${page.dest}`);
}

// Verify every synced page appears in docsNav.ts
const navPath = join(__dirname, "..", "src", "data", "docsNav.ts");
const navSource = readFileSync(navPath, "utf8");
const navHrefs = new Set([...navSource.matchAll(/href:\s*"([^"]+)"/g)].map((m) => m[1]));
let missing = 0;
for (const page of PAGES) {
  const url = "/" + page.dest.replace(/\.md$/, "/").replace(/\/index\/$/, "/");
  if (!navHrefs.has(url)) {
    console.error(`  WARNING: ${url} (from ${page.source}) is not in docsNav.ts`);
    missing++;
  }
}
if (missing > 0) {
  console.error(`\n${missing} page(s) missing from sidebar navigation (website/src/data/docsNav.ts)`);
  process.exit(1);
}

console.log("Done.");
