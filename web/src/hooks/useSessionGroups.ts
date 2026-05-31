import { useCallback, useMemo, useState } from "react";
import type { Workspace } from "../lib/types";
import { safeGetItem, safeRemoveItem, safeSetItem } from "../lib/safeStorage";
import { buildSessionGroups, type SidebarGroup } from "../lib/sidebarGroups";
import { useIdleDecayWindowMs } from "../lib/idleDecay";

// Distinct from the repo axis prefix (`aoe-repo-collapsed-`) so collapse
// state is per-axis: collapsing a user group never changes a repo group's
// state and vice versa. See #1234.
const COLLAPSED_KEY_PREFIX = "aoe-group-collapsed-";

function loadCollapsed(id: string): boolean {
  return safeGetItem(`${COLLAPSED_KEY_PREFIX}${id}`) === "1";
}

export function useSessionGroups(workspaces: Workspace[]): {
  groups: SidebarGroup[];
  toggleGroupCollapsed: (groupId: string) => void;
} {
  const idleDecayWindowMs = useIdleDecayWindowMs();
  const [collapsedMap, setCollapsedMap] = useState<Record<string, boolean>>({});

  const groups = useMemo(
    () =>
      buildSessionGroups(workspaces, {
        idleDecayWindowMs,
        isCollapsed: (id) => collapsedMap[id] ?? loadCollapsed(id),
      }),
    [workspaces, idleDecayWindowMs, collapsedMap],
  );

  const toggleGroupCollapsed = useCallback((groupId: string) => {
    setCollapsedMap((prev) => {
      const current = prev[groupId] ?? loadCollapsed(groupId);
      const next = !current;
      if (next) {
        safeSetItem(`${COLLAPSED_KEY_PREFIX}${groupId}`, "1");
      } else {
        safeRemoveItem(`${COLLAPSED_KEY_PREFIX}${groupId}`);
      }
      return { ...prev, [groupId]: next };
    });
  }, []);

  return { groups, toggleGroupCollapsed };
}
