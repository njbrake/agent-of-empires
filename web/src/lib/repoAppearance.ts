import { safeGetItem, safeRemoveItem, safeSetItem } from "./safeStorage";

const STORAGE_KEY = "aoe-repo-appearance-v1";

export type RepoColor = "amber" | "teal" | "sky" | "violet" | "rose" | "slate";

export interface RepoAppearance {
  alias?: string;
  color?: RepoColor;
}

export type RepoAppearanceUpdate = {
  alias?: string | null;
  color?: RepoColor | null;
};

export const REPO_COLOR_OPTIONS: Array<{
  id: RepoColor;
  label: string;
  swatchClass: string;
  headerClass: string;
}> = [
  {
    id: "amber",
    label: "Amber",
    swatchClass: "bg-amber-500",
    headerClass: "bg-amber-950/30 hover:bg-amber-900/30",
  },
  {
    id: "teal",
    label: "Teal",
    swatchClass: "bg-teal-500",
    headerClass: "bg-teal-950/30 hover:bg-teal-900/30",
  },
  {
    id: "sky",
    label: "Sky",
    swatchClass: "bg-sky-500",
    headerClass: "bg-sky-950/30 hover:bg-sky-900/30",
  },
  {
    id: "violet",
    label: "Violet",
    swatchClass: "bg-violet-500",
    headerClass: "bg-violet-950/30 hover:bg-violet-900/30",
  },
  {
    id: "rose",
    label: "Rose",
    swatchClass: "bg-rose-500",
    headerClass: "bg-rose-950/30 hover:bg-rose-900/30",
  },
  {
    id: "slate",
    label: "Slate",
    swatchClass: "bg-slate-500",
    headerClass: "bg-slate-700/30 hover:bg-slate-700/40",
  },
];

const validColors = new Set(REPO_COLOR_OPTIONS.map((option) => option.id));

function normalizeAppearance(value: unknown): RepoAppearance | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as { alias?: unknown; color?: unknown };
  const alias = typeof raw.alias === "string" ? raw.alias.trim() : "";
  const color =
    typeof raw.color === "string" && validColors.has(raw.color as RepoColor)
      ? (raw.color as RepoColor)
      : undefined;
  if (!alias && !color) return null;
  return {
    ...(alias ? { alias } : {}),
    ...(color ? { color } : {}),
  };
}

export function loadRepoAppearances(): Record<string, RepoAppearance> {
  const raw = safeGetItem(STORAGE_KEY);
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const entries = Object.entries(parsed)
      .map(([repoId, value]) => [repoId, normalizeAppearance(value)] as const)
      .filter((entry): entry is readonly [string, RepoAppearance] => entry[1] !== null);
    return Object.fromEntries(entries);
  } catch {
    return {};
  }
}

export function persistRepoAppearances(map: Record<string, RepoAppearance>): void {
  if (Object.keys(map).length === 0) {
    safeRemoveItem(STORAGE_KEY);
    return;
  }
  safeSetItem(STORAGE_KEY, JSON.stringify(map));
}

export function applyRepoAppearanceUpdate(
  current: Record<string, RepoAppearance>,
  repoId: string,
  update: RepoAppearanceUpdate,
): Record<string, RepoAppearance> {
  const nextForRepo: RepoAppearance = { ...(current[repoId] ?? {}) };
  if ("alias" in update) {
    const alias = update.alias?.trim() ?? "";
    if (alias) nextForRepo.alias = alias;
    else delete nextForRepo.alias;
  }
  if ("color" in update) {
    if (update.color && validColors.has(update.color)) nextForRepo.color = update.color;
    else delete nextForRepo.color;
  }

  const next = { ...current };
  if (nextForRepo.alias || nextForRepo.color) next[repoId] = nextForRepo;
  else delete next[repoId];
  return next;
}
