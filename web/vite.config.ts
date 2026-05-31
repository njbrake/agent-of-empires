/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import istanbul from "vite-plugin-istanbul";

const collectCoverage = process.env.AOE_COVERAGE === "1";

// `cargo xtask dev` runs the Rust server and this dev server together and
// passes the ports through these env vars. The server backend defaults to
// 8081 in debug builds; the Vite dev server defaults to 5173.
const servePort = process.env.AOE_SERVE_PORT ?? "8081";
const webPort = Number(process.env.AOE_WEB_PORT ?? "5173");
const serveTarget = `http://127.0.0.1:${servePort}`;

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
  // REST under /api, WebSocket PTY/cockpit relays under /sessions. Proxy both
  // to the running `aoe serve` so the dev server's /api and /ws calls resolve.
  server: {
    port: webPort,
    proxy: {
      "/api": { target: serveTarget, changeOrigin: true },
      "/sessions": { target: serveTarget, ws: true, changeOrigin: true },
    },
  },
  // Vitest unit tests live alongside source as `*.test.ts(x)`. Playwright
  // suites under `tests/` use the same `.spec.ts` extension Playwright
  // expects but aren't valid vitest tests, so we explicitly exclude them.
  test: {
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    // Type-level tests (`*.types.test.ts`) run under the typecheck runner
    // below, not the runtime runner, so keep them out of `include`.
    exclude: [
      "tests/**",
      "node_modules/**",
      "dist/**",
      "src/**/*.types.test.ts",
    ],
    // `expectTypeOf` assertions in `*.types.test.ts` are checked by tsc.
    // A failing assertion surfaces as a type error. Scoped to the
    // dedicated type-test files so the rest of the suite stays fast.
    typecheck: {
      enabled: true,
      include: ["src/**/*.types.test.ts"],
      tsconfig: "./tsconfig.vitest.json",
    },
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
