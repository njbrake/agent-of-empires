// Browser-side log relay to /api/client-log.
//
// Captures window.onerror, unhandledrejection, React ErrorBoundary, and
// explicit calls via `clog.trace/debug/info/warn/error(target, message, fields)`
// or the legacy `reportError(err, ctx)` shape.
//
// Throttling: token-bucket (10 cap, 10/s refill) on entries. Batches
// flush every 2s, on size threshold, on visibilitychange=hidden, and
// on pagehide. Unload-time flush uses navigator.sendBeacon with a JSON
// Blob since sendBeacon can't carry the Authorization header.
//
// Level gating: the backend exposes GET /api/client-log/policy. Until a
// policy arrives we use a conservative fallback ({"web.client.*": "info",
// "web.client.error": "error"}). When the backend filter says
// `web.client.input` is `warn`, then `clog.trace("web.client.input", ...)`
// is a no-op — the network call never fires. This is what prevents the
// rate-limited relay from being saturated when default_level = trace.
//
// URL sanitization: never log `window.location.href` raw, because we
// embed the auth token in `?token=` and don't want it on disk.
//
// Target whitelist: clog only allows targets prefixed with `web.client`.
// Anything else is rewritten to `web.client`.

export type ClientLogLevel = "trace" | "error" | "warn" | "info" | "debug";

export interface ClientLogEntry {
  level: ClientLogLevel;
  message: string;
  stack?: string;
  componentStack?: string;
  target?: string;
  sessionId?: string;
  requestId?: string;
  fields?: Record<string, unknown>;
  rid: string;
  path: string;
  userAgent: string;
  ts: number;
  dropped?: number;
}

// Per-page-load correlation id. Sent on every clog event and (eventually)
// on every fetch via the apiClient wrapper. Lets the backend group
// frontend events with the corresponding http.request span by request_id.
let pageRid = "";
function getPageRid(): string {
  if (pageRid) return pageRid;
  try {
    pageRid =
      typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : Math.random().toString(36).slice(2);
  } catch {
    pageRid = Math.random().toString(36).slice(2);
  }
  return pageRid;
}

// Numeric level ordering: lower = more verbose.
const LEVEL_RANK: Record<ClientLogLevel, number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
};

// Conservative fallback policy used until the server policy arrives.
// `info` baseline so high-volume targets (input, terminal) stay quiet by
// default; user must explicitly enable trace via the backend filter.
let policy: { default: ClientLogLevel; targets: Record<string, ClientLogLevel> } = {
  default: "info",
  targets: { "web.client.error": "error" },
};
let policyVersion = 0;

function effectiveLevel(target: string): ClientLogLevel {
  // Last-wins match against the most specific known prefix.
  let best: ClientLogLevel = policy.default;
  let bestLen = -1;
  for (const [t, lvl] of Object.entries(policy.targets)) {
    if (target === t || target.startsWith(`${t}.`)) {
      if (t.length > bestLen) {
        bestLen = t.length;
        best = lvl;
      }
    }
  }
  return best;
}

function isEnabled(target: string, level: ClientLogLevel): boolean {
  return LEVEL_RANK[level] >= LEVEL_RANK[effectiveLevel(target)];
}

function normalizeTarget(target?: string): string {
  if (!target) return "web.client";
  if (target === "web.client" || target.startsWith("web.client.")) {
    // Cap at 64 chars to mirror server-side rule.
    return target.length > 64 ? target.slice(0, 64) : target;
  }
  return "web.client";
}

const SENSITIVE = /token|passphrase|secret|password/i;
function redactFields(fields?: Record<string, unknown>): Record<string, unknown> | undefined {
  if (!fields) return undefined;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(fields)) {
    if (SENSITIVE.test(k)) {
      out[k] = "[redacted]";
    } else {
      out[k] = v;
    }
  }
  return out;
}

/**
 * Refresh the frontend logging policy from the backend.
 *
 * Pulled at install time and after every settings-save / log-level change
 * so trace-level client events do not flood the network when the backend
 * filter would drop them. Falls back silently if the endpoint is missing
 * or returns garbage — that just keeps the conservative defaults.
 */
export async function refreshClientLogPolicy(): Promise<void> {
  try {
    const resp = await fetch("/api/client-log/policy", { credentials: "include" });
    if (!resp.ok) return;
    const body = (await resp.json()) as {
      version?: number;
      default_level?: string;
      targets?: Record<string, string>;
    };
    const def = body.default_level as ClientLogLevel | undefined;
    if (def && def in LEVEL_RANK) {
      const targets: Record<string, ClientLogLevel> = {};
      for (const [k, v] of Object.entries(body.targets ?? {})) {
        if (v in LEVEL_RANK) targets[k] = v as ClientLogLevel;
      }
      policy = { default: def, targets };
      policyVersion = body.version ?? policyVersion + 1;
    }
  } catch {
    // Network failure; keep prior policy.
  }
}

/** Read-only view of the current policy for debugging. */
export function currentClientLogPolicy() {
  return { ...policy, version: policyVersion };
}

const ENDPOINT = "/api/client-log";
const RATE_CAP = 10;
const RATE_REFILL_PER_SEC = 10;
const FLUSH_INTERVAL_MS = 2000;
const MAX_BATCH = 20;
const MAX_BATCH_BYTES = 48 * 1024;

let installed = false;
let queue: ClientLogEntry[] = [];
let dropped = 0;
let tokens = RATE_CAP;
let lastRefill = Date.now();
let isReporting = false;

function sanitizedPath(): string {
  try {
    const u = new URL(window.location.href);
    u.searchParams.delete("token");
    return `${u.pathname}${u.search}${u.hash}`;
  } catch {
    return "/";
  }
}

function refillTokens(): void {
  const now = Date.now();
  const delta = (now - lastRefill) / 1000;
  if (delta <= 0) return;
  tokens = Math.min(RATE_CAP, tokens + delta * RATE_REFILL_PER_SEC);
  lastRefill = now;
}

function tryConsumeToken(): boolean {
  refillTokens();
  if (tokens >= 1) {
    tokens -= 1;
    return true;
  }
  return false;
}

function normalizeError(err: unknown): { message: string; stack?: string } {
  if (err instanceof Error) {
    return { message: err.message || String(err), stack: err.stack };
  }
  if (typeof err === "string") return { message: err };
  try {
    return { message: JSON.stringify(err) };
  } catch {
    return { message: String(err) };
  }
}

function enqueue(entry: ClientLogEntry): void {
  if (isReporting) return;
  if (!tryConsumeToken()) {
    dropped += 1;
    return;
  }
  queue.push(entry);
  if (queue.length >= MAX_BATCH) {
    void flush(false);
  }
}

async function flush(viaBeacon: boolean): Promise<void> {
  if (queue.length === 0 && dropped === 0) return;
  if (isReporting) return;
  isReporting = true;
  try {
    const batch = queue;
    queue = [];
    if (dropped > 0) {
      batch.push({
        level: "warn",
        message: `log relay dropped ${dropped} entries (rate-limited)`,
        target: "web.client.error",
        rid: getPageRid(),
        path: sanitizedPath(),
        userAgent: navigator.userAgent,
        ts: Date.now(),
        dropped,
      });
      dropped = 0;
    }
    // Trim oversized payloads to fit the keepalive budget.
    const body = trimToBudget(batch);
    const json = JSON.stringify({ entries: body });

    if (viaBeacon && typeof navigator.sendBeacon === "function") {
      const blob = new Blob([json], { type: "application/json" });
      navigator.sendBeacon(ENDPOINT, blob);
      return;
    }
    await fetch(ENDPOINT, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: json,
      keepalive: true,
      credentials: "include",
    });
  } catch {
    // Drop the batch on failure; don't recurse into the logger.
  } finally {
    isReporting = false;
  }
}

function trimToBudget(batch: ClientLogEntry[]): ClientLogEntry[] {
  let totalBytes = 0;
  const out: ClientLogEntry[] = [];
  for (const entry of batch) {
    const size = JSON.stringify(entry).length;
    if (out.length >= MAX_BATCH || totalBytes + size > MAX_BATCH_BYTES) {
      dropped += batch.length - out.length;
      break;
    }
    totalBytes += size;
    out.push(entry);
  }
  return out;
}

export function reportError(
  err: unknown,
  ctx?: Partial<ClientLogEntry>,
): void {
  const { message, stack } = normalizeError(err);
  const target = normalizeTarget(ctx?.target ?? "web.client.error");
  const level = ctx?.level ?? "error";
  if (!isEnabled(target, level)) return;
  enqueue({
    level,
    message,
    stack: ctx?.stack ?? stack,
    componentStack: ctx?.componentStack,
    target,
    sessionId: ctx?.sessionId,
    requestId: ctx?.requestId,
    fields: redactFields(ctx?.fields),
    rid: getPageRid(),
    path: sanitizedPath(),
    userAgent: navigator.userAgent,
    ts: Date.now(),
  });
}

function emit(
  level: ClientLogLevel,
  target: string,
  message: string,
  fields?: Record<string, unknown>,
): void {
  const t = normalizeTarget(target);
  if (!isEnabled(t, level)) return;
  enqueue({
    level,
    message,
    target: t,
    fields: redactFields(fields),
    requestId: typeof fields?.request_id === "string" ? (fields.request_id as string) : undefined,
    sessionId: typeof fields?.session_id === "string" ? (fields.session_id as string) : undefined,
    rid: getPageRid(),
    path: sanitizedPath(),
    userAgent: navigator.userAgent,
    ts: Date.now(),
  });
}

/**
 * Central client-side logger. Routes events to /api/client-log under
 * `web.client.*` targets, gated by the server-derived policy so trace-level
 * frontend events never reach the network unless the backend filter would
 * keep them. Always prefer this over `console.log` for anything that needs
 * to land in `debug.log` alongside backend events.
 *
 * Targets must start with `web.client` or `web.client.*`. Anything else is
 * silently rewritten to `web.client` (forge protection).
 *
 * Field name redaction strips `token`/`passphrase`/`secret`/`password`
 * keys before send.
 */
export const clog = {
  trace(target: string, message: string, fields?: Record<string, unknown>) {
    emit("trace", target, message, fields);
  },
  debug(target: string, message: string, fields?: Record<string, unknown>) {
    emit("debug", target, message, fields);
  },
  info(target: string, message: string, fields?: Record<string, unknown>) {
    emit("info", target, message, fields);
  },
  warn(target: string, message: string, fields?: Record<string, unknown>) {
    emit("warn", target, message, fields);
  },
  error(target: string, message: string, fields?: Record<string, unknown>) {
    emit("error", target, message, fields);
  },
};

export function installClientLogger(): void {
  if (installed) return;
  installed = true;

  // Fetch policy now (and again on user-driven settings/log-level changes).
  void refreshClientLogPolicy();

  window.addEventListener("error", (e) => {
    reportError(e.error ?? e.message, { target: "web.client.error" });
  });

  window.addEventListener("unhandledrejection", (e) => {
    reportError(e.reason, { target: "web.client.error" });
  });

  setInterval(() => void flush(false), FLUSH_INTERVAL_MS);

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      void flush(true);
    }
  });

  window.addEventListener("pagehide", () => {
    void flush(true);
  });
}
