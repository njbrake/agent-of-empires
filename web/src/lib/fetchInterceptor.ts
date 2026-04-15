import { reportError } from "./toastBus";
import { getToken } from "./token";

/**
 * Install a global fetch wrapper that:
 * 1. Injects `Authorization: Bearer <token>` when we have a stored token.
 *    The PWA needs this because iOS `start_url` strips the `?token=` query
 *    param on home-screen relaunch, and cookies can be lost across the
 *    Safari→standalone context switch.
 * 2. Surfaces 5xx responses and network failures as user-visible toasts.
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

    const patchedInit = attachAuthHeader(input, init);

    try {
      const res = await original(input, patchedInit);
      if (isApi && res.status >= 500) {
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
      if (isApi) {
        reportError(
          `Network error contacting ${path}. Check your connection.`,
        );
      }
      throw err;
    }
  };
}

// Inject Authorization header without clobbering anything the caller set.
// Skips cross-origin URLs so we never leak the token off-site.
function attachAuthHeader(
  input: RequestInfo | URL,
  init: RequestInit | undefined,
): RequestInit | undefined {
  const token = getToken();
  if (!token) return init;

  const rawUrl =
    typeof input === "string"
      ? input
      : input instanceof URL
        ? input.toString()
        : input.url;
  if (!isSameOrigin(rawUrl)) return init;

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
