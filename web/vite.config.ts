/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import istanbul from "vite-plugin-istanbul";

const collectCoverage = process.env.AOE_COVERAGE === "1";

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    ...(collectCoverage
      ? [
          istanbul({
            include: "src/**/*",
            exclude: [
              "node_modules",
              "dist",
              "**/*.test.{ts,tsx}",
              "**/__tests__/**",
            ],
            extension: [".ts", ".tsx"],
            requireEnv: false,
            forceBuildInstrument: true,
          }),
        ]
      : []),
  ],
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
    setupFiles: ["./src/test-setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "json", "html", "lcov"],
      reportsDirectory: "./coverage/vitest",
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/**/*.d.ts",
        "src/main.tsx",
        "src/test-setup.ts",
        "src/**/__tests__/**",
        "src/**/*.test.{ts,tsx}",
      ],
    },
  },
});
