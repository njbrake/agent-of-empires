/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  // Vitest unit tests live alongside source as `*.test.ts(x)`. Playwright
  // suites under `tests/` use the same `.spec.ts` extension Playwright
  // expects but aren't valid vitest tests, so we explicitly exclude them.
  test: {
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    exclude: ["tests/**", "node_modules/**", "dist/**"],
  },
});
