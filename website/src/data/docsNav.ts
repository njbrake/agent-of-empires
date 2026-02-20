export interface NavItem {
  title: string;
  href: string;
}

export interface NavSection {
  title: string;
  items: NavItem[];
}

export const docsNav: NavSection[] = [
  {
    title: "Getting Started",
    items: [
      { title: "Introduction", href: "/docs/" },
      { title: "Installation", href: "/docs/installation/" },
      { title: "Quick Start", href: "/docs/quick-start/" },
    ],
  },
  {
    title: "Guides",
    items: [
      { title: "Workflow", href: "/docs/guides/workflow/" },
      { title: "Docker Sandbox", href: "/docs/guides/sandbox/" },
      { title: "Repo Config & Hooks", href: "/docs/guides/repo-config/" },
      { title: "Git Worktrees", href: "/docs/guides/worktrees/" },
      { title: "Diff View", href: "/docs/guides/diff-view/" },
      { title: "tmux Status Bar", href: "/docs/guides/tmux-status-bar/" },
      { title: "Sound Effects", href: "/docs/sounds/" },
    ],
  },
  {
    title: "Reference",
    items: [
      { title: "CLI Reference", href: "/docs/cli/reference/" },
      { title: "Configuration", href: "/docs/guides/configuration/" },
    ],
  },
  {
    title: "Contributing",
    items: [
      { title: "Development", href: "/docs/development/" },
      { title: "Credits", href: "/docs/credits/" },
    ],
  },
];

export function getFlatNavItems(): NavItem[] {
  return docsNav.flatMap((section) => section.items);
}
