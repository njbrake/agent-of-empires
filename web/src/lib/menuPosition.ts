export interface ClampMenuArgs {
  x: number;
  y: number;
  menuWidth: number;
  menuHeight: number;
  viewportWidth: number;
  viewportHeight: number;
  margin?: number;
}

/** Clamp a `position: fixed` floating menu's top-left so the menu fits
 *  inside the viewport with at least `margin` pixels of breathing room
 *  on every side. Used by the sidebar's right-click / long-press
 *  context menus so opening one near the bottom or right edge does not
 *  push items off-screen. When the menu is taller or wider than the
 *  viewport (minus margins) the position collapses to `margin` rather
 *  than going negative; the menu's own scrolling is left to the
 *  caller's stylesheet. See #1601. */
export function clampMenuPosition({
  x,
  y,
  menuWidth,
  menuHeight,
  viewportWidth,
  viewportHeight,
  margin = 8,
}: ClampMenuArgs): { x: number; y: number } {
  const maxX = Math.max(margin, viewportWidth - menuWidth - margin);
  const maxY = Math.max(margin, viewportHeight - menuHeight - margin);
  const nextX = Math.min(Math.max(x, margin), maxX);
  const nextY = Math.min(Math.max(y, margin), maxY);
  return { x: nextX, y: nextY };
}
