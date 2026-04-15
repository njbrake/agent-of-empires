import { reportError } from "./toastBus";

/**
 * Install a global fetch wrapper that surfaces 5xx responses and network
 * failures as user-visible toasts. 4xx is intentionally silent because many
 * endpoints treat client errors as part of normal validation (e.g. the wizard
 * filesystem browser 400s on invalid paths while typing).
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

    try {
      const res = await original(input, init);
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
