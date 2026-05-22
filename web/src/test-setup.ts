import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// React 19's scheduler can leave work pending past the end of a test file.
// When jsdom is torn down between files, any remaining work fires in a
// setImmediate callback and crashes with "ReferenceError: window is not defined"
// (originating in node_modules/react-dom/cjs/react-dom-client.development.js).
// Unmounting every rendered tree after each test prevents that pending work.
afterEach(() => {
  cleanup();
});
