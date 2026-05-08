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
      { title: "Docker Sandbox", href: "/guides/sandbox/" },
      { title: "Web Dashboard", href: "/guides/web-dashboard/" },
      { title: "Cockpit (Native Agent Rendering)", href: "/docs/cockpit/" },
      { title: "Remote Phone Access", href: "/guides/remote-phone-access/" },
      { title: "Repo Config & Hooks", href: "/guides/repo-config/" },
      { title: "Git Worktrees", href: "/guides/worktrees/" },
      { title: "Multi-Repo Workspaces", href: "/guides/multi-repo-workspaces/" },
      { title: "Diff View", href: "/guides/diff-view/" },
      { title: "tmux Status Bar", href: "/guides/tmux-status-bar/" },
      { title: "Agent Command Overrides", href: "/guides/agent-override/" },
      { title: "Session Resume (Claude)", href: "/guides/session-resume/" },
      { title: "Sound Effects", href: "/docs/sounds/" },
    ],
  },
  {
    title: "Reference",
    items: [
      { title: "CLI Reference", href: "/docs/cli/reference/" },
      { title: "HTTP API Reference", href: "/docs/api/" },
      { title: "Configuration", href: "/docs/guides/configuration/" },
    ],
  },
  {
    title: "Contributing",
    items: [
      { title: "Development", href: "/docs/development/" },
      { title: "Adding a New Agent", href: "/docs/development/adding-agents/" },
    ],
  },
];

export function getFlatNavItems(): NavItem[] {
  return docsNav.flatMap((section) => section.items);
}
