// Playwright `test` wrapper for the mocked suite under `web/tests/`.
//
// Re-exports `@playwright/test`'s `test` and `expect` with one override:
// the `page` fixture captures `window.__coverage__` after each test so
// the merged-LCOV pipeline picks up coverage from the mocked specs the
// same way it does from live specs.
//
// Specs do:
//
//   import { test, expect } from "./helpers/mockedTest";
//
// `vite preview` serves the instrumented bundle when the workflow builds
// with `AOE_COVERAGE=1`. Without that env var, `captureCoverage` is a
// no-op and the override is invisible.

import { test as base, expect } from "@playwright/test";
import { captureCoverage } from "./coverageCapture";

export const test = base.extend({
  page: async ({ page }, use, testInfo) => {
    await use(page);
    await captureCoverage(page, testInfo.titlePath.join(" > "));
  },
});

export { expect };
