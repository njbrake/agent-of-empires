// Persists the auth token across iOS PWA launches.
//
// iOS manifests use `start_url` when launching from the home screen, which
// strips any `?token=...` that was on the URL when the user tapped "Add to
// Home Screen". Cookies may also be lost across the Safari→standalone
// context switch. localStorage survives both, so we stash the token there
// and send it via `Authorization: Bearer` on every request.

const STORAGE_KEY = "aoe_auth_token";

function captureFromUrl(): void {
  if (typeof window === "undefined") return;
  const url = new URL(window.location.href);
  const token = url.searchParams.get("token");
  if (!token) return;

  try {
    window.localStorage.setItem(STORAGE_KEY, token);
  } catch {
    // Private mode / storage disabled: fall back to the token staying in the
    // URL and cookie for this session only. Nothing else to do.
    return;
  }

  url.searchParams.delete("token");
  const clean = url.pathname + (url.search ? url.search : "") + url.hash;
  window.history.replaceState(null, "", clean || "/");
}

captureFromUrl();

export function getToken(): string | null {
  try {
    return window.localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function clearToken(): void {
  try {
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    // nothing to do
  }
}
