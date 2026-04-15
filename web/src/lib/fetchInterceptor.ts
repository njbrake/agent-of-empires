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
    const url =
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url;

    try {
      const res = await original(input, init);
      if (res.status >= 500 && url.startsWith("/api/")) {
        reportError(describeServerError(url, res.status));
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
      if (url.startsWith("/api/")) {
        reportError(
          `Network error contacting ${shortPath(url)}. Check your connection.`,
        );
      }
      throw err;
    }
  };
}

function shortPath(url: string): string {
  try {
    const u = new URL(url, window.location.origin);
    return u.pathname;
  } catch {
    return url;
  }
}

function describeServerError(url: string, status: number): string {
  return `Server error ${status} from ${shortPath(url)}`;
}
