// Migrate `?session=<id>` URLs (used before path-based routing) to
// `/session/<id>`. Notification taps from older builds and bookmarks
// still land on the legacy form, so this rewrite runs once at boot
// before the router mounts.

if (typeof window !== "undefined") {
  const params = new URLSearchParams(window.location.search);
  const sessionId = params.get("session");
  if (sessionId) {
    params.delete("session");
    const remaining = params.toString();
    const next = `/session/${encodeURIComponent(sessionId)}${remaining ? `?${remaining}` : ""}${window.location.hash}`;
    window.history.replaceState(null, "", next);
  }
}
