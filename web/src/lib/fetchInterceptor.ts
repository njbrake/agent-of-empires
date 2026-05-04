import { isServerDown } from "./connectionState";
import { reportError } from "./toastBus";
import { clearToken, getToken, saveToken } from "./token";

/** Dispatched on `window` when the auth token is rejected or missing. App.tsx
 *  listens for this to show the token entry page instead of just a toast. */
export const TOKEN_EXPIRED_EVENT = "aoe:token-expired";

/** Dispatched on `window` when the token is valid but the passphrase login
 *  session is missing or expired. App.tsx listens to show the LoginPage
 *  instead of the TokenEntryPage, so a valid token isn't wrongly cleared. */
export const LOGIN_REQUIRED_EVENT = "aoe:login-required";

/** Classify a 401 body as `login_required` or `unauthorized`. Clones the
 *  response so downstream readers (fetchJson, etc.) can still parse the
 *  body. Non-401 returns null. */
export async function classifyAuthError(
  res: Response,
): Promise<"login_required" | "unauthorized" | null> {
  if (res.status !== 401) return null;
  try {
    const data = (await res.clone().json()) as { error?: unknown };
    if (data && data.error === "login_required") return "login_required";
  } catch {
    // Body wasn't JSON or already consumed; fall through to unauthorized.
  }
  return "unauthorized";
}

/**
 * Install a global fetch wrapper that:
 * 1. Injects `Authorization: Bearer <token>` when we have a stored token.
 *    The PWA needs this because iOS `start_url` strips the `?token=` query
 *    param on home-screen relaunch, and cookies can be lost across the
 *    Safari→standalone context switch.
 * 2. Reads `X-Aoe-Token` from same-origin responses and updates localStorage,
 *    so PWA clients stay in sync when the server rotates the token (the
 *    cookie flow gets this via `Set-Cookie`).
 * 3. Clears the stored token on 401 from `/api/*` so the PWA doesn't keep
 *    re-sending a dead token and wedging the user into a silent loop.
 * 4. Surfaces 5xx responses and network failures as user-visible toasts.
 *    4xx is intentionally silent because many endpoints treat client errors
 *    as part of normal validation (e.g. the wizard filesystem browser 400s
 *    on invalid paths while typing).
 *
 * Safe to call multiple times; only the first call installs the wrapper.
 */
export function installFetchErrorToasts(): void {
  if ((window as unknown as { __aoeFetchPatched?: boolean }).__aoeFetchPatched) {
    return;
  }
  (window as unknown as { __aoeFetchPatched?: boolean }).__aoeFetchPatched = true;

  const original = window.fetch.bind(window);

  window.fetch = async (input, init) => {
    const rawUrl =
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url;
    const path = toPath(rawUrl);
    const isApi = path.startsWith("/api/");
    const sameOrigin = isSameOrigin(rawUrl);

    const patchedInit = attachAuthHeader(sameOrigin, init);

    try {
      const res = await original(input, patchedInit);
      if (sameOrigin) {
        const rotated = res.headers.get("x-aoe-token");
        if (rotated) saveToken(rotated);
      }
      if (res.status === 401 && isApi) {
        const authError = await classifyAuthError(res);
        if (authError === "login_required") {
          handleLoginRequired();
        } else {
          handleTokenAuthFailure();
        }
      }
      if (isApi && res.status >= 500 && !isServerDown()) {
        reportError(`Server error ${res.status} from ${path}`);
      }
      return res;
    } catch (err) {
      // Ignore aborts (triggered by deliberate cleanup).
      if (
        err instanceof DOMException &&
        (err.name === "AbortError" || err.name === "TimeoutError")
      ) {
        throw err;
      }
      // When the server is known to be down, suppress per-request toasts.
      // The DisconnectBanner handles the user-facing notification instead.
      if (isApi && !isServerDown()) {
        reportError(
          `Network error contacting ${path}. Check your connection.`,
        );
      }
      throw err;
    }
  };
}

// 401 with no `login_required` body: token is dead, missing, or revoked.
// Clear localStorage (idempotent if no token) and show the token entry
// page. Dedupe so a burst of concurrent 401s produces one event.
let tokenExpiredDispatched = false;
function handleTokenAuthFailure(): void {
  clearToken();
  if (tokenExpiredDispatched) return;
  tokenExpiredDispatched = true;
  window.dispatchEvent(new CustomEvent(TOKEN_EXPIRED_EVENT));
}

// On 401 `login_required` the token is fine; only the second factor is
// missing. Don't clear the token. Dedupe so a burst of concurrent 401s
// produces one event.
let loginRequiredDispatched = false;
function handleLoginRequired(): void {
  if (loginRequiredDispatched) return;
  loginRequiredDispatched = true;
  window.dispatchEvent(new CustomEvent(LOGIN_REQUIRED_EVENT));
}

/** Reset the dedup flags so a new 401 after re-authentication will be
 *  caught again. Called when the user submits a new token or completes
 *  the passphrase login. */
export function resetTokenExpired(): void {
  tokenExpiredDispatched = false;
  loginRequiredDispatched = false;
}

// Inject Authorization header without clobbering anything the caller set.
// Skips cross-origin URLs so we never leak the token off-site.
function attachAuthHeader(
  sameOrigin: boolean,
  init: RequestInit | undefined,
): RequestInit | undefined {
  if (!sameOrigin) return init;
  const token = getToken();
  if (!token) return init;

  const headers = new Headers(init?.headers);
  if (!headers.has("Authorization")) {
    headers.set("Authorization", `Bearer ${token}`);
  }
  return { ...(init ?? {}), headers };
}

function isSameOrigin(url: string): boolean {
  if (url.startsWith("/")) return true;
  try {
    return new URL(url, window.location.origin).origin === window.location.origin;
  } catch {
    return false;
  }
}

/** Normalize any fetch input to a pathname so `/api/` checks work regardless
 *  of whether the caller passed a string, URL, or Request. */
function toPath(url: string): string {
  if (url.startsWith("/")) return url;
  try {
    return new URL(url, window.location.origin).pathname;
  } catch {
    return url;
  }
}
