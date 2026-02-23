export interface GuideNavItem {
  title: string;
  href: string;
}

export const guidesNav: GuideNavItem[] = [
  { title: "Managing Agents", href: "/guides/manage-ai-coding-agents/" },
  { title: "Git Worktrees", href: "/guides/git-worktrees-ai-development/" },
  { title: "Docker Sandbox", href: "/guides/docker-sandbox-ai-agents/" },
  { title: "tmux Workflow", href: "/guides/tmux-ai-coding-workflow/" },
];
