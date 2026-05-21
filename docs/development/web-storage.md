# Web localStorage convention

All `localStorage.setItem` writes in `web/src/**` go through `safeSetItem`
from `web/src/lib/safeStorage.ts`. Bare `localStorage.setItem` and
`window.localStorage.setItem` are banned by ESLint (`no-restricted-syntax`).

## Why

Unguarded `localStorage.setItem` calls throw `QuotaExceededError` when
the browser quota is full and `SecurityError` in private mode. When
those throws happen inside a React `setState` updater (as in a resize
handler), the exception surfaces through the commit phase and blanks
the dashboard. #1345 was the user-visible bug that motivated the
convention.

The helpers swallow these throws so non-critical writes never crash
the app.

## API

```ts
import {
  safeSetItem,
  safeGetItem,
  safeRemoveItem,
  isQuotaExceededError,
} from "../lib/safeStorage";
```

- `safeSetItem(key, value): boolean`, returns `true` on success, `false`
  on any storage throw (quota, security, disabled).
- `safeGetItem(key): string | null`, returns `null` on throw.
- `safeRemoveItem(key): void`, never throws.
- `isQuotaExceededError(err): boolean`, classifies a caught error.
  Cross-browser: matches `QuotaExceededError`, Firefox's
  `NS_ERROR_DOM_QUOTA_REACHED`, legacy DOMException codes 22 and 1014.

The boolean return on `safeSetItem` lets callers branch on persistence
pressure without unwrapping a discriminated union. Most call sites
ignore it (UI preference writes are fire-and-forget). Two surfaces use
it today:

- `useCockpit.ts::persistState` reacts to `false` by evicting the
  single oldest `aoe:cockpit-state:v1:*` entry and retrying exactly
  once. Drafts (`cockpit:draft:*`) are never in the eviction set.
- `cockpitDrafts.ts::setDraft` reacts to `false` on a non-empty write
  by surfacing a per-session "Storage full: unsent draft not saved"
  toast, deduped fire-once per session and reset on the next
  successful write.

## When to add a new write

1. Route through `safeSetItem`. If your write is critical and must
   hard-fail on quota (auth tokens, device binding secrets), keep raw
   `window.localStorage.setItem` and add an inline
   `eslint-disable-next-line no-restricted-syntax` with a comment
   explaining the rethrow contract.
2. If the write fills up a cache that grows unbounded over time
   (per-session entries, append-only logs), add an eviction policy in
   the surrounding module. Whitelist-filter by your own key prefix so
   the policy never touches other modules' keys.
3. If the write holds user-authored data that is NOT replayable from
   the server (drafts, unsent forms), surface a toast on failure so the
   user knows their text is at risk. Dedupe to avoid storms.

## Documented exceptions

Two modules deliberately keep raw `localStorage.setItem` with inline
lint disables:

- `web/src/lib/token.ts`: token persistence falls back to cookie + URL
  on throw; the catch path is load-bearing for the auth flow.
- `web/src/lib/deviceBinding.ts`: must hard-fail on quota so callers
  can surface the error to the user; silently swallowing the throw
  would leave the user with an invalid device binding.

Both files document the contract in a block comment above the
`eslint-disable-next-line` directive.

## Test fixtures

Two patterns coexist for testing the helper behavior:

- `// @vitest-environment jsdom` at the top of a test file enables a
  real `window.localStorage`. Most component and hook tests use this.
- Module-level `installFakeLocalStorage()` polyfills `globalThis.localStorage`
  with a Map-backed Storage for node-env tests (see
  `web/src/components/diff/comments/storage.test.ts`). The helpers
  resolve storage via `globalThis.localStorage` so both fixtures work
  unchanged.

To exercise the failure path, `vi.spyOn(Storage.prototype, "setItem")`
with a `mockImplementation` that throws `new DOMException(..., "QuotaExceededError")`
is the canonical mock.
